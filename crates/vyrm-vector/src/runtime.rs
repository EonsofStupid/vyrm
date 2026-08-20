use crate::contract::invalid;
use crate::{
    search_exact_ref, AccessPathKind, CompactDenseSegment, HnswIndex, ImmutableVectorSegment,
    SearchHit, SearchPlan, SearchRequest, VectorCandidate, VectorCatalog, VectorPlanner,
    VectorProjectionDescriptor,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::{
    digest, Error, ProjectionId, ProjectionStamp, ProjectionState, Result,
    DATA_RUNTIME_CONTRACT_VERSION,
};

#[derive(Debug, Clone, PartialEq)]
pub enum VectorArtifact {
    ExactSegment(ImmutableVectorSegment),
    CompactDense(CompactDenseSegment),
    Hnsw(HnswIndex),
}

impl VectorArtifact {
    pub fn descriptor(&self) -> VectorProjectionDescriptor {
        match self {
            Self::ExactSegment(segment) => segment.descriptor().clone().into(),
            Self::CompactDense(segment) => segment.descriptor().clone().into(),
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

impl From<CompactDenseSegment> for VectorArtifact {
    fn from(segment: CompactDenseSegment) -> Self {
        Self::CompactDense(segment)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchExecution {
    pub plan: SearchPlan,
    pub hits: Vec<SearchHit>,
}

/// A sealed, request-bound planner result. Private fields prevent callers from
/// substituting a cheaper or stale access path between planning and execution.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreparedVectorSearch {
    request_digest: String,
    catalog_revision: u64,
    ef_search: usize,
    plan_digest: String,
    selected_stamp: ProjectionStamp,
    plan: SearchPlan,
}

impl PreparedVectorSearch {
    pub fn request_digest(&self) -> &str {
        &self.request_digest
    }

    pub const fn catalog_revision(&self) -> u64 {
        self.catalog_revision
    }

    pub const fn ef_search(&self) -> usize {
        self.ef_search
    }

    pub fn plan_digest(&self) -> &str {
        &self.plan_digest
    }

    pub fn selected_stamp(&self) -> &ProjectionStamp {
        &self.selected_stamp
    }

    pub fn plan(&self) -> &SearchPlan {
        &self.plan
    }

    fn validate(&self) -> Result<()> {
        self.selected_stamp.validate()?;
        if self.ef_search == 0 || self.ef_search > 1_000_000 {
            return invalid("prepared vector ef_search must be in 1..=1000000");
        }
        if self.plan_digest
            != prepared_plan_digest(
                &self.request_digest,
                self.catalog_revision,
                self.ef_search,
                &self.selected_stamp,
                &self.plan,
            )?
        {
            return invalid("prepared vector plan digest does not match its coordinates");
        }
        Ok(())
    }
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

    pub fn prepare_search(
        &self,
        request: &SearchRequest,
        ef_search: usize,
    ) -> Result<PreparedVectorSearch> {
        self.prepare_search_at(request, request.read.commit_cursor, ef_search)
    }

    pub fn prepare_search_at(
        &self,
        request: &SearchRequest,
        required_source_cursor: u64,
        ef_search: usize,
    ) -> Result<PreparedVectorSearch> {
        if ef_search == 0 || ef_search > 1_000_000 {
            return invalid("vector ef_search must be in 1..=1000000");
        }
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
                        VectorArtifact::ExactSegment(_) | VectorArtifact::CompactDense(_) => None,
                    })
                    .unwrap_or(descriptor.nodes.max(1) as u64),
            };
            paths.push(descriptor.candidate_path(estimated_cost));
        }
        let plan = VectorPlanner::new(self.canonical.len() as u64).plan_at(
            request,
            required_source_cursor,
            paths,
        )?;
        let selected_stamp = if plan.selected.id.as_str() == crate::EXACT_SCAN_PROJECTION_ID {
            ProjectionStamp {
                contract_version: DATA_RUNTIME_CONTRACT_VERSION,
                id: plan.selected.id.clone(),
                generation: plan.selected.generation,
                source_cursor: plan.selected.source_cursor,
                config_digest: "00".repeat(32),
                artifact_digest: "00".repeat(32),
                state: ProjectionState::Ready,
            }
        } else {
            self.catalog
                .entries
                .get(&plan.selected.id)
                .map(|descriptor| descriptor.stamp().clone())
                .ok_or_else(|| Error::InvalidRuntime {
                    reason: "selected vector projection is absent from the catalog".into(),
                })?
        };
        let request_digest = request.digest()?;
        let plan_digest = prepared_plan_digest(
            &request_digest,
            self.catalog.revision,
            ef_search,
            &selected_stamp,
            &plan,
        )?;
        let prepared = PreparedVectorSearch {
            request_digest,
            catalog_revision: self.catalog.revision,
            ef_search,
            plan_digest,
            selected_stamp,
            plan,
        };
        prepared.validate()?;
        Ok(prepared)
    }

    pub fn execute_search(
        &self,
        request: &SearchRequest,
        prepared: &PreparedVectorSearch,
    ) -> Result<SearchExecution> {
        prepared.validate()?;
        if request.digest()? != prepared.request_digest {
            return invalid("prepared vector plan belongs to another request");
        }
        if self.catalog.revision != prepared.catalog_revision {
            return invalid("prepared vector plan uses a stale catalog revision");
        }
        let expected = self.prepare_search_at(
            request,
            prepared.plan.required_source_cursor,
            prepared.ef_search,
        )?;
        if &expected != prepared {
            return invalid("prepared vector plan no longer matches planner output");
        }
        let plan = &prepared.plan;
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
                        segment.search_at(request, plan.required_source_cursor)?
                    }
                    (AccessPathKind::ExactSegment, VectorArtifact::CompactDense(segment)) => {
                        segment.search_at(
                            request,
                            crate::DenseKernel::Auto,
                            plan.required_source_cursor,
                        )?
                    }
                    (AccessPathKind::Hnsw, VectorArtifact::Hnsw(index)) => {
                        index.search_at(request, prepared.ef_search, plan.required_source_cursor)?
                    }
                    _ => return invalid("selected vector access path has the wrong artifact kind"),
                }
            }
        };
        Ok(SearchExecution {
            plan: plan.clone(),
            hits,
        })
    }

    pub fn search(&self, request: &SearchRequest, ef_search: usize) -> Result<SearchExecution> {
        let prepared = self.prepare_search(request, ef_search)?;
        self.execute_search(request, &prepared)
    }
}

fn prepared_plan_digest(
    request_digest: &str,
    catalog_revision: u64,
    ef_search: usize,
    selected_stamp: &ProjectionStamp,
    plan: &SearchPlan,
) -> Result<String> {
    let encoded = serde_json::to_vec(&(
        request_digest,
        catalog_revision,
        ef_search,
        selected_stamp,
        plan,
    ))
    .map_err(|error| Error::InvalidRuntime {
        reason: format!("prepared vector plan cannot be encoded: {error}"),
    })?;
    let mut bytes = b"vyrm-prepared-vector-search-v1\0".to_vec();
    bytes.extend_from_slice(&encoded);
    Ok(digest::sha256_hex(&bytes))
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
            embedding_model: None,
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
                embedding_model: None,
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
        let prepared_request = request(
            scope.clone(),
            SearchMode::RequireApproximate { exact_rerank: 2 },
        );
        let prepared = runtime.prepare_search(&prepared_request, 3).unwrap();

        let exact = ImmutableVectorSegment::build(
            VectorSegmentConfig {
                id: ProjectionId::new("vector:exact:body").unwrap(),
                scope: scope.clone(),
                field: "body".into(),
                dimensions: 2,
                metric: ScoreMetric::Dot,
                embedding_model: None,
                filter_properties: BTreeSet::new(),
            },
            1,
            3,
            values,
        )
        .unwrap();
        runtime.publish(1, exact).unwrap();
        assert!(runtime
            .execute_search(&prepared_request, &prepared)
            .unwrap_err()
            .to_string()
            .contains("stale catalog revision"));
        let exact = runtime
            .search(&request(scope, SearchMode::Exact), 3)
            .unwrap();
        assert_eq!(exact.plan.selected.kind, AccessPathKind::ExactScan);
        assert_eq!(exact.hits[0].reference.id.as_str(), "a");
    }
}
