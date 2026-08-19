use crate::contract::invalid;
use crate::{ScoreMetric, SearchHit, SearchRequest, VectorCandidate, VectorQuery};
use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::{Result, RuntimeChange, RuntimeMutation, RuntimeRef, ScopeId, VectorValue};

pub fn candidates_from_changes(changes: &[RuntimeChange], scope: &ScopeId) -> Vec<VectorCandidate> {
    changes
        .iter()
        .filter(|change| &change.scope == scope)
        .filter_map(|change| match &change.mutation {
            RuntimeMutation::Vector { vector } => Some(VectorCandidate {
                scope: change.scope.clone(),
                source_cursor: change.cursor,
                vector: vector.clone(),
            }),
            _ => None,
        })
        .collect()
}

pub fn search_changes_exact(
    request: &SearchRequest,
    changes: &[RuntimeChange],
) -> Result<Vec<SearchHit>> {
    search_exact(request, candidates_from_changes(changes, &request.scope))
}

/// Deterministic exact oracle over canonical vector versions.
///
/// Latest transaction-visible version wins per vector identity. Valid-time
/// retirement, field selection, and payload filtering are applied to that
/// version before scoring. Shape drift within one searched field fails closed.
pub fn search_exact(
    request: &SearchRequest,
    candidates: impl IntoIterator<Item = VectorCandidate>,
) -> Result<Vec<SearchHit>> {
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    search_exact_ref(request, &candidates)
}

/// Borrowing form of the exact oracle for hot query paths. It has identical
/// semantics without copying the canonical candidate corpus per request.
pub fn search_exact_ref<'a>(
    request: &SearchRequest,
    candidates: impl IntoIterator<Item = &'a VectorCandidate>,
) -> Result<Vec<SearchHit>> {
    request.validate()?;
    let latest = materialize_visible_refs(request, candidates)?;
    let mut hits = Vec::new();
    for candidate in latest {
        let vector = &candidate.vector;
        let score = score_query(&request.query, &vector.value, request.metric)?;
        hits.push(SearchHit {
            reference: vector.reference.clone(),
            subject: vector.subject.clone(),
            source_cursor: candidate.source_cursor,
            score,
        });
    }
    hits.sort_by(SearchHit::compare_best_first);
    hits.truncate(request.top_k);
    Ok(hits)
}

fn materialize_visible_refs<'a>(
    request: &SearchRequest,
    candidates: impl IntoIterator<Item = &'a VectorCandidate>,
) -> Result<Vec<&'a VectorCandidate>> {
    let mut latest = BTreeMap::<&'a RuntimeRef, &'a VectorCandidate>::new();
    let mut versions = BTreeSet::new();
    for candidate in candidates {
        if candidate.scope != request.scope || candidate.source_cursor > request.read.commit_cursor
        {
            continue;
        }
        candidate.validate()?;
        let version = (&candidate.vector.reference, candidate.source_cursor);
        if !versions.insert(version) {
            return invalid("duplicate vector identity/source-cursor version");
        }
        if candidate.vector.valid_from > request.valid_at {
            continue;
        }
        let identity = &candidate.vector.reference;
        if latest
            .get(&identity)
            .is_none_or(|current| current.source_cursor < candidate.source_cursor)
        {
            latest.insert(identity, candidate);
        }
    }

    let mut visible = Vec::new();
    for candidate in latest.into_values() {
        let vector = &candidate.vector;
        if vector.field != request.field
            || vector
                .valid_to
                .is_some_and(|valid_to| request.valid_at >= valid_to)
            || request
                .filter
                .as_ref()
                .is_some_and(|filter| !filter.matches(candidate.filter_properties()))
        {
            continue;
        }
        visible.push(candidate);
    }
    Ok(visible)
}

pub(crate) fn validate_candidate_versions<'a>(
    candidates: impl IntoIterator<Item = &'a VectorCandidate>,
) -> Result<()> {
    let mut versions = BTreeSet::new();
    for candidate in candidates {
        candidate.validate()?;
        if !versions.insert((candidate.vector.reference.clone(), candidate.source_cursor)) {
            return invalid("duplicate vector identity/source-cursor version");
        }
    }
    Ok(())
}

pub(crate) fn score_query(
    query: &VectorQuery,
    candidate: &VectorValue,
    metric: ScoreMetric,
) -> Result<f64> {
    match (query, candidate) {
        (VectorQuery::Dense { values: left }, VectorValue::Dense { values: right }) => {
            score_dense(left, right, metric)
        }
        (
            VectorQuery::Sparse {
                dimensions: left_dimensions,
                indices: left_indices,
                values: left_values,
            },
            VectorValue::Sparse {
                dimensions: right_dimensions,
                indices: right_indices,
                values: right_values,
            },
        ) => {
            if left_dimensions != right_dimensions {
                return invalid("sparse query and candidate dimensions differ");
            }
            score_sparse(
                left_indices,
                left_values,
                right_indices,
                right_values,
                metric,
            )
        }
        (
            VectorQuery::MultiDense {
                dimensions,
                vectors: left,
                ..
            },
            VectorValue::MultiDense {
                dimensions: right_dimensions,
                vectors: right,
            },
        ) => {
            if dimensions != right_dimensions {
                return invalid("multi-vector query and candidate dimensions differ");
            }
            let mut total = 0.0;
            for query_row in left {
                let best = right
                    .iter()
                    .map(|candidate_row| score_dense(query_row, candidate_row, metric))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .max_by(f64::total_cmp)
                    .ok_or_else(|| vyrm_core::Error::InvalidRuntime {
                        reason: "multi-vector candidate has no rows".into(),
                    })?;
                total += best;
            }
            Ok(total)
        }
        _ => invalid("query and candidate vector kinds differ"),
    }
}

pub(crate) fn score_dense_candidate(
    query: &[f32],
    candidate: &VectorValue,
    metric: ScoreMetric,
) -> Result<f64> {
    match candidate {
        VectorValue::Dense { values } => score_dense(query, values, metric),
        _ => invalid("dense query requires a dense candidate"),
    }
}

fn score_dense(left: &[f32], right: &[f32], metric: ScoreMetric) -> Result<f64> {
    if left.len() != right.len() {
        return invalid("dense query and candidate dimensions differ");
    }
    let dot = || {
        left.iter()
            .zip(right)
            .map(|(left, right)| f64::from(*left) * f64::from(*right))
            .sum::<f64>()
    };
    match metric {
        ScoreMetric::Dot => Ok(dot()),
        ScoreMetric::Cosine => {
            let left_norm = squared_norm(left).sqrt();
            let right_norm = squared_norm(right).sqrt();
            if right_norm == 0.0 {
                Ok(0.0)
            } else {
                Ok(dot() / (left_norm * right_norm))
            }
        }
        ScoreMetric::Euclidean => Ok(-left
            .iter()
            .zip(right)
            .map(|(left, right)| (f64::from(*left) - f64::from(*right)).powi(2))
            .sum::<f64>()
            .sqrt()),
        ScoreMetric::Manhattan => Ok(-left
            .iter()
            .zip(right)
            .map(|(left, right)| (f64::from(*left) - f64::from(*right)).abs())
            .sum::<f64>()),
    }
}

fn squared_norm(values: &[f32]) -> f64 {
    values.iter().map(|value| f64::from(*value).powi(2)).sum()
}

fn score_sparse(
    left_indices: &[u32],
    left_values: &[f32],
    right_indices: &[u32],
    right_values: &[f32],
    metric: ScoreMetric,
) -> Result<f64> {
    let mut left = 0;
    let mut right = 0;
    let mut dot = 0.0;
    let mut squared_distance = 0.0;
    let mut manhattan = 0.0;
    while left < left_indices.len() || right < right_indices.len() {
        match (left_indices.get(left), right_indices.get(right)) {
            (Some(left_index), Some(right_index)) if left_index == right_index => {
                let left_value = f64::from(left_values[left]);
                let right_value = f64::from(right_values[right]);
                dot += left_value * right_value;
                squared_distance += (left_value - right_value).powi(2);
                manhattan += (left_value - right_value).abs();
                left += 1;
                right += 1;
            }
            (Some(left_index), Some(right_index)) if left_index < right_index => {
                let value = f64::from(left_values[left]);
                squared_distance += value.powi(2);
                manhattan += value.abs();
                left += 1;
            }
            (Some(_), Some(_)) => {
                let value = f64::from(right_values[right]);
                squared_distance += value.powi(2);
                manhattan += value.abs();
                right += 1;
            }
            (Some(_), None) => {
                let value = f64::from(left_values[left]);
                squared_distance += value.powi(2);
                manhattan += value.abs();
                left += 1;
            }
            (None, Some(_)) => {
                let value = f64::from(right_values[right]);
                squared_distance += value.powi(2);
                manhattan += value.abs();
                right += 1;
            }
            (None, None) => break,
        }
    }
    match metric {
        ScoreMetric::Dot => Ok(dot),
        ScoreMetric::Cosine => {
            let left_norm = squared_norm(left_values).sqrt();
            let right_norm = squared_norm(right_values).sqrt();
            if right_norm == 0.0 {
                Ok(0.0)
            } else {
                Ok(dot / (left_norm * right_norm))
            }
        }
        ScoreMetric::Euclidean => Ok(-squared_distance.sqrt()),
        ScoreMetric::Manhattan => Ok(-manhattan),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchMode;
    use vyrm_core::{RuntimeProperties, RuntimeValue, ScopeId};

    fn stamp(scope: &ScopeId, cursor: u64) -> vyrm_core::ReadStamp {
        vyrm_core::ReadStamp::new(scope.clone(), None, 0, cursor, Some("11".repeat(32))).unwrap()
    }

    fn candidate(
        scope: &ScopeId,
        cursor: u64,
        id: &str,
        values: Vec<f32>,
        rank: i64,
    ) -> VectorCandidate {
        let mut properties = RuntimeProperties::new();
        properties.insert("rank".into(), RuntimeValue::Integer(rank));
        VectorCandidate {
            scope: scope.clone(),
            source_cursor: cursor,
            vector: vyrm_core::RuntimeVector {
                reference: RuntimeRef::new("embedding", id).unwrap(),
                subject: RuntimeRef::new("document", id).unwrap(),
                field: "body".into(),
                valid_from: 1,
                valid_to: None,
                value: VectorValue::Dense { values },
                provenance: None,
                properties,
            },
        }
    }

    #[test]
    fn exact_search_has_stable_scores_ties_filters_and_temporal_updates() {
        let scope = ScopeId::new("instance:exact").unwrap();
        let request = SearchRequest {
            scope: scope.clone(),
            read: stamp(&scope, 5),
            valid_at: 10,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0],
            },
            metric: ScoreMetric::Cosine,
            top_k: 3,
            mode: crate::SearchMode::Exact,
            filter: Some(crate::FilterExpression::Condition {
                condition: crate::FilterCondition {
                    property: "rank".into(),
                    operator: crate::FilterOperator::Range {
                        gt: None,
                        gte: Some(RuntimeValue::Integer(2)),
                        lt: None,
                        lte: None,
                    },
                },
            }),
        };
        let mut retired = candidate(&scope, 4, "retired", vec![1.0, 0.0], 4);
        retired.vector.valid_to = Some(5);
        let hits = search_exact(
            &request,
            vec![
                candidate(&scope, 1, "b", vec![1.0, 0.0], 2),
                candidate(&scope, 2, "a", vec![1.0, 0.0], 3),
                candidate(&scope, 3, "filtered", vec![1.0, 0.0], 1),
                retired,
                candidate(&scope, 6, "future", vec![1.0, 0.0], 6),
            ],
        )
        .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].reference.id.as_str(), "a");
        assert_eq!(hits[1].reference.id.as_str(), "b");
        assert_eq!(hits[0].score, 1.0);
    }

    #[test]
    fn dense_sparse_and_multivector_metrics_match_hand_calculation() {
        assert_eq!(
            score_dense(&[1.0, 2.0], &[4.0, 6.0], ScoreMetric::Dot).unwrap(),
            16.0
        );
        assert_eq!(
            score_dense(&[1.0, 2.0], &[4.0, 6.0], ScoreMetric::Euclidean).unwrap(),
            -5.0
        );
        assert_eq!(
            score_sparse(&[0, 3], &[1.0, 2.0], &[1, 3], &[4.0, 3.0], ScoreMetric::Dot).unwrap(),
            6.0
        );
        let query = VectorQuery::MultiDense {
            dimensions: 2,
            vectors: vec![vec![1.0, 0.0], vec![0.0, 1.0]],
            comparator: crate::MultiVectorComparator::MaxSim,
        };
        let candidate = VectorValue::MultiDense {
            dimensions: 2,
            vectors: vec![vec![1.0, 0.0], vec![0.5, 0.5]],
        };
        assert_eq!(
            score_query(&query, &candidate, ScoreMetric::Dot).unwrap(),
            1.5
        );
    }

    #[test]
    fn duplicate_identity_cursor_versions_fail_closed() {
        let scope = ScopeId::new("instance:duplicate-vector").unwrap();
        let duplicate = candidate(&scope, 1, "duplicate", vec![1.0, 0.0], 0);
        let request = SearchRequest {
            scope: scope.clone(),
            read: stamp(&scope, 1),
            valid_at: 2,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0],
            },
            metric: ScoreMetric::Dot,
            top_k: 1,
            mode: SearchMode::Exact,
            filter: None,
        };
        assert!(search_exact(&request, [duplicate.clone(), duplicate]).is_err());
    }
}
