use crate::{ScoreMetric, SearchMode, SearchRequest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use vyrm_core::{ProjectionId, ProjectionStamp, ProjectionState, Result};

pub const EXACT_SCAN_PROJECTION_ID: &str = "vector:exact-scan";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessPathKind {
    ExactScan,
    ExactSegment,
    Hnsw,
}

impl AccessPathKind {
    fn is_approximate(self) -> bool {
        self == Self::Hnsw
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidatePath {
    pub stamp: ProjectionStamp,
    pub kind: AccessPathKind,
    pub field: String,
    pub dimensions: usize,
    pub metric: ScoreMetric,
    #[serde(default)]
    pub filter_properties: BTreeSet<String>,
    pub estimated_candidates: u64,
    pub estimated_cost: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedPath {
    pub id: ProjectionId,
    pub kind: AccessPathKind,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanDecision {
    pub id: ProjectionId,
    pub kind: AccessPathKind,
    pub generation: u64,
    pub source_cursor: u64,
    pub estimated_candidates: u64,
    pub estimated_cost: u64,
    pub exact_rerank: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPlan {
    pub selected: PlanDecision,
    pub rejected: Vec<RejectedPath>,
    pub required_source_cursor: u64,
    pub required_filter_properties: Vec<String>,
    pub approximation_requested: bool,
}

pub struct VectorPlanner {
    exact_scan_candidates: u64,
}

impl VectorPlanner {
    pub const fn new(exact_scan_candidates: u64) -> Self {
        Self {
            exact_scan_candidates,
        }
    }

    pub fn plan(
        &self,
        request: &SearchRequest,
        candidates: impl IntoIterator<Item = CandidatePath>,
    ) -> Result<SearchPlan> {
        request.validate()?;
        let required_properties = request
            .filter
            .as_ref()
            .map(|filter| filter.referenced_properties())
            .unwrap_or_default();
        let required_property_set = required_properties.iter().cloned().collect::<BTreeSet<_>>();
        let approximation_requested = request.mode != SearchMode::Exact;
        let exact_scan = CandidatePath {
            stamp: ProjectionStamp {
                contract_version: vyrm_core::DATA_RUNTIME_CONTRACT_VERSION,
                id: ProjectionId::new(EXACT_SCAN_PROJECTION_ID)?,
                generation: 1,
                source_cursor: request.read.commit_cursor,
                config_digest: "00".repeat(32),
                artifact_digest: "00".repeat(32),
                state: ProjectionState::Ready,
            },
            kind: AccessPathKind::ExactScan,
            field: request.field.clone(),
            dimensions: request.query.dimensions(),
            metric: request.metric,
            filter_properties: required_property_set.clone(),
            estimated_candidates: self.exact_scan_candidates,
            estimated_cost: self.exact_scan_candidates.max(1),
        };

        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        for candidate in std::iter::once(exact_scan).chain(candidates) {
            let reasons = reject_reasons(request, &candidate, &required_property_set);
            if reasons.is_empty() {
                accepted.push(candidate);
            } else {
                rejected.push(RejectedPath {
                    id: candidate.stamp.id,
                    kind: candidate.kind,
                    reasons,
                });
            }
        }

        if request.mode == SearchMode::Exact {
            accepted.retain(|candidate| !candidate.kind.is_approximate());
        }
        if matches!(request.mode, SearchMode::RequireApproximate { .. }) {
            accepted.retain(|candidate| candidate.kind.is_approximate());
        }
        accepted.sort_by(|left, right| {
            left.estimated_cost
                .cmp(&right.estimated_cost)
                .then_with(|| left.stamp.id.cmp(&right.stamp.id))
        });
        let selected =
            accepted
                .into_iter()
                .next()
                .ok_or_else(|| vyrm_core::Error::InvalidRuntime {
                    reason: "no vector access path satisfies the request contract".into(),
                })?;
        let exact_rerank = match request.mode {
            SearchMode::Exact => request.top_k,
            SearchMode::AllowApproximate { exact_rerank }
            | SearchMode::RequireApproximate { exact_rerank } => exact_rerank,
        };
        Ok(SearchPlan {
            selected: PlanDecision {
                id: selected.stamp.id,
                kind: selected.kind,
                generation: selected.stamp.generation,
                source_cursor: selected.stamp.source_cursor,
                estimated_candidates: selected.estimated_candidates,
                estimated_cost: selected.estimated_cost,
                exact_rerank,
            },
            rejected,
            required_source_cursor: request.read.commit_cursor,
            required_filter_properties: required_properties,
            approximation_requested,
        })
    }
}

fn reject_reasons(
    request: &SearchRequest,
    candidate: &CandidatePath,
    required_properties: &BTreeSet<String>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if candidate.stamp.validate().is_err() {
        reasons.push("invalid projection stamp".into());
    }
    if candidate.stamp.state != ProjectionState::Ready {
        reasons.push(format!(
            "projection state is {:?}, not ready",
            candidate.stamp.state
        ));
    }
    if candidate.stamp.source_cursor < request.read.commit_cursor {
        reasons.push(format!(
            "stale source coverage {} < required {}",
            candidate.stamp.source_cursor, request.read.commit_cursor
        ));
    }
    if candidate.field != request.field {
        reasons.push("field mismatch".into());
    }
    if candidate.dimensions != request.query.dimensions() {
        reasons.push("dimension mismatch".into());
    }
    if candidate.metric != request.metric {
        reasons.push("metric mismatch".into());
    }
    if !required_properties.is_subset(&candidate.filter_properties) {
        reasons.push("filter coverage is incomplete".into());
    }
    if candidate.kind.is_approximate() && request.mode == SearchMode::Exact {
        reasons.push("request requires exact candidate generation".into());
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FilterCondition, FilterExpression, FilterOperator, VectorQuery};
    use vyrm_core::{ReadStamp, RuntimeValue, ScopeId};

    fn request(mode: SearchMode) -> SearchRequest {
        let scope = ScopeId::new("instance:planner").unwrap();
        SearchRequest {
            read: ReadStamp::new(scope.clone(), None, 0, 10, Some("11".repeat(32))).unwrap(),
            scope,
            valid_at: 100,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0],
            },
            metric: ScoreMetric::Cosine,
            top_k: 2,
            mode,
            filter: Some(FilterExpression::Condition {
                condition: FilterCondition {
                    property: "tenant".into(),
                    operator: FilterOperator::Equals {
                        value: RuntimeValue::String("a".into()),
                    },
                },
            }),
        }
    }

    fn path(cursor: u64, properties: &[&str], state: ProjectionState) -> CandidatePath {
        CandidatePath {
            stamp: ProjectionStamp {
                contract_version: vyrm_core::DATA_RUNTIME_CONTRACT_VERSION,
                id: ProjectionId::new(format!("vector:hnsw:{cursor}")).unwrap(),
                generation: 1,
                source_cursor: cursor,
                config_digest: "22".repeat(32),
                artifact_digest: "33".repeat(32),
                state,
            },
            kind: AccessPathKind::Hnsw,
            field: "body".into(),
            dimensions: 2,
            metric: ScoreMetric::Cosine,
            filter_properties: properties.iter().map(|value| (*value).into()).collect(),
            estimated_candidates: 20,
            estimated_cost: 20,
        }
    }

    #[test]
    fn stale_or_filter_incomplete_hnsw_is_explained_and_exact_fallback_wins() {
        let plan = VectorPlanner::new(1_000)
            .plan(
                &request(SearchMode::AllowApproximate { exact_rerank: 20 }),
                [
                    path(9, &["tenant"], ProjectionState::Ready),
                    path(10, &[], ProjectionState::Ready),
                ],
            )
            .unwrap();
        assert_eq!(plan.selected.kind, AccessPathKind::ExactScan);
        assert_eq!(plan.rejected.len(), 2);
        assert!(plan.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("stale")));
        assert!(plan.rejected[1]
            .reasons
            .iter()
            .any(|reason| reason.contains("filter")));
    }

    #[test]
    fn require_approximate_denies_when_only_stale_generation_exists() {
        let error = VectorPlanner::new(1_000)
            .plan(
                &request(SearchMode::RequireApproximate { exact_rerank: 20 }),
                [path(9, &["tenant"], ProjectionState::Ready)],
            )
            .unwrap_err();
        assert!(error.to_string().contains("no vector access path"));
    }

    #[test]
    fn ready_covered_hnsw_is_selected_only_when_approximation_is_allowed() {
        let candidate = path(10, &["tenant"], ProjectionState::Ready);
        let approximate = VectorPlanner::new(1_000)
            .plan(
                &request(SearchMode::AllowApproximate { exact_rerank: 20 }),
                [candidate.clone()],
            )
            .unwrap();
        assert_eq!(approximate.selected.kind, AccessPathKind::Hnsw);
        let exact = VectorPlanner::new(1_000)
            .plan(&request(SearchMode::Exact), [candidate])
            .unwrap();
        assert_eq!(exact.selected.kind, AccessPathKind::ExactScan);
    }
}
