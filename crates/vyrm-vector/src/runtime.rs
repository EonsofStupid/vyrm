use crate::contract::invalid;
use crate::{
    search_exact_ref, AccessPathKind, HnswIndex, ImmutableVectorSegment, SearchHit, SearchPlan,
    SearchRequest, VectorCandidate, VectorCatalog, VectorPlanner, VectorProjectionDescriptor,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::{ProjectionId, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum VectorArtifact {
    ExactSegment(ImmutableVectorSegment),
    Hnsw(HnswIndex),
}

impl VectorArtifact {
    pub fn descriptor(&self) -> VectorProjectionDescriptor {
        match self {
            Self::ExactSegment(segment) => segment.descriptor().clone().into(),
            Self::Hnsw(index) => index.descriptor().clone().into(),
        }
    }
}

impl From<ImmutableVectorSegment> for VectorArtifact {
    fn from(segment: ImmutableVectorSegment) -> Self {
        Self::ExactSegment(segment)
    }
}

impl From<HnswIndex> for VectorArtifact {
    fn from(index: HnswIndex) -> Self {
        Self::Hnsw(index)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchExecution {
    pub plan: SearchPlan,
    pub hits: Vec<SearchHit>,
}

/// In-process vector search coordinator.
///
/// Canonical candidates remain the truth path. Rebuildable artifacts are
/// installed through the CAS catalog and are selected only through the typed
/// planner. Execution rechecks the exact published descriptor before touching
/// artifact bytes, preventing stale or substituted generations from serving.
#[derive(Debug, Clone, Default)]
pub struct VectorRuntime {
    canonical: Vec<VectorCandidate>,
    catalog: VectorCatalog,
    artifacts: BTreeMap<(ProjectionId, u64), VectorArtifact>,
}

impl VectorRuntime {
    pub fn new(canonical: impl IntoIterator<Item = VectorCandidate>) -> Result<Self> {
        let canonical = canonical.into_iter().collect::<Vec<_>>();
        for candidate in &canonical {
            candidate.validate()?;
        }
        Ok(Self {
            canonical,
            catalog: VectorCatalog::default(),
            artifacts: BTreeMap::new(),
        })
    }

    pub fn catalog(&self) -> &VectorCatalog {
        &self.catalog
    }

    pub fn publish(
        &mut self,
        expected_revision: u64,
        artifact: impl Into<VectorArtifact>,
    ) -> Result<u64> {
        let artifact = artifact.into();
        let descriptor = artifact.descriptor();
        let key = (descriptor.stamp().id.clone(), descriptor.stamp().generation);
        if self.artifacts.contains_key(&key) {
            return invalid("vector artifact generation is already installed");
        }
        let revision = self.catalog.publish(expected_revision, descriptor)?;
        self.artifacts.insert(key, artifact);
        Ok(revision)
    }

    pub fn quarantine(
        &mut self,
        expected_revision: u64,
        id: &ProjectionId,
        generation: u64,
    ) -> Result<u64> {
        self.catalog.quarantine(expected_revision, id, generation)
    }

    pub fn reclaim_retired(&mut self, protected: &BTreeSet<(ProjectionId, u64)>) -> Vec<String> {
        let reclaimed = self.catalog.reclaim_retired(protected);
        let digests = reclaimed.iter().cloned().collect::<BTreeSet<_>>();
        self.artifacts.retain(|_, artifact| {
            !digests.contains(&artifact.descriptor().stamp().artifact_digest)
        });
        reclaimed
    }

    pub fn search(&self, request: &SearchRequest, ef_search: usize) -> Result<SearchExecution> {
        let mut paths = Vec::with_capacity(self.catalog.entries.len());
        for descriptor in self.catalog.entries.values() {
            let estimated_cost = match descriptor {
                VectorProjectionDescriptor::ExactSegment { descriptor } => {
                    descriptor.candidate_versions.max(1) as u64
                }
                VectorProjectionDescriptor::Hnsw { descriptor } => self
                    .artifacts
                    .get(&(descriptor.stamp.id.clone(), descriptor.stamp.generation))
                    .and_then(|artifact| match artifact {
                        VectorArtifact::Hnsw(index) => {
                            index.estimated_search_cost(request, ef_search).ok()
                        }
                        VectorArtifact::ExactSegment(_) => None,
                    })
                    .unwrap_or(descriptor.nodes.max(1) as u64),
            };
            paths.push(descriptor.candidate_path(estimated_cost));
        }
        let plan = VectorPlanner::new(self.canonical.len() as u64).plan(request, paths)?;
        let hits = match plan.selected.kind {
            AccessPathKind::ExactScan => search_exact_ref(request, &self.canonical)?,
            AccessPathKind::ExactSegment | AccessPathKind::Hnsw => {
                let key = (plan.selected.id.clone(), plan.selected.generation);
                let artifact =
                    self.artifacts
                        .get(&key)
                        .ok_or_else(|| vyrm_core::Error::InvalidRuntime {
                            reason: "selected vector artifact bytes are absent".into(),
                        })?;
                let published = self.catalog.entries.get(&plan.selected.id).ok_or_else(|| {
                    vyrm_core::Error::InvalidRuntime {
                        reason: "selected vector projection is absent from the catalog".into(),
                    }
                })?;
                if artifact.descriptor() != *published {
                    return invalid("selected vector artifact differs from its catalog descriptor");
                }
                match (plan.selected.kind, artifact) {
                    (AccessPathKind::ExactSegment, VectorArtifact::ExactSegment(segment)) => {
                        segment.search(request)?
                    }
                    (AccessPathKind::Hnsw, VectorArtifact::Hnsw(index)) => {
                        index.search(request, ef_search)?
                    }
                    _ => return invalid("selected vector access path has the wrong artifact kind"),
                }
            }
        };
        Ok(SearchExecution { plan, hits })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HnswConfig, ScoreMetric, SearchMode, VectorQuery, VectorSegmentConfig};
    use vyrm_core::{
        ReadStamp, RuntimeProperties, RuntimeRef, RuntimeVector, ScopeId, VectorValue,
    };

    fn candidate(scope: &ScopeId, cursor: u64, id: &str, values: Vec<f32>) -> VectorCandidate {
        VectorCandidate {
            scope: scope.clone(),
            source_cursor: cursor,
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", id).unwrap(),
                subject: RuntimeRef::new("document", id).unwrap(),
                field: "body".into(),
                valid_from: 1,
                valid_to: None,
                value: VectorValue::Dense { values },
                provenance: None,
                properties: RuntimeProperties::new(),
            },
        }
    }

    fn request(scope: ScopeId, mode: SearchMode) -> SearchRequest {
        SearchRequest {
            read: ReadStamp::new(scope.clone(), None, 0, 3, Some("11".repeat(32))).unwrap(),
            scope,
            valid_at: 2,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0],
            },
            metric: ScoreMetric::Dot,
            top_k: 1,
            mode,
            filter: None,
        }
    }

    #[test]
    fn planner_decision_is_rechecked_at_execution() {
        let scope = ScopeId::new("instance:vector-runtime").unwrap();
        let values = vec![
            candidate(&scope, 1, "a", vec![1.0, 0.0]),
            candidate(&scope, 2, "b", vec![0.0, 1.0]),
            candidate(&scope, 3, "c", vec![0.5, 0.5]),
        ];
        let mut runtime = VectorRuntime::new(values.clone()).unwrap();
        let hnsw = HnswIndex::build(
            HnswConfig {
                id: ProjectionId::new("vector:hnsw:body").unwrap(),
                scope: scope.clone(),
                field: "body".into(),
                dimensions: 2,
                metric: ScoreMetric::Dot,
                m: 4,
                ef_construction: 8,
                max_level: 4,
                seed: 9,
                filter_properties: BTreeSet::new(),
            },
            1,
            3,
            values.clone(),
        )
        .unwrap();
        runtime.publish(0, hnsw).unwrap();
        let approximate = runtime
            .search(
                &request(
                    scope.clone(),
                    SearchMode::RequireApproximate { exact_rerank: 2 },
                ),
                3,
            )
            .unwrap();
        assert_eq!(approximate.plan.selected.kind, AccessPathKind::Hnsw);
        assert_eq!(approximate.hits[0].reference.id.as_str(), "a");

        let exact = ImmutableVectorSegment::build(
            VectorSegmentConfig {
                id: ProjectionId::new("vector:exact:body").unwrap(),
                scope: scope.clone(),
                field: "body".into(),
                dimensions: 2,
                metric: ScoreMetric::Dot,
                filter_properties: BTreeSet::new(),
            },
            1,
            3,
            values,
        )
        .unwrap();
        runtime.publish(1, exact).unwrap();
        let exact = runtime
            .search(&request(scope, SearchMode::Exact), 3)
            .unwrap();
        assert_eq!(exact.plan.selected.kind, AccessPathKind::ExactScan);
        assert_eq!(exact.hits[0].reference.id.as_str(), "a");
    }
}
