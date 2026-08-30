use crate::contract::invalid;
use crate::{
    search_exact_ref, AccessPathKind, CompactDenseSegment, HnswIndex, HnswKernel,
    ImmutableVectorSegment, QuantizationMethod, QuantizedKernel, QuantizedSegment, SearchHit,
    SearchPlan, SearchRequest, TurboQuantSegment, VectorCandidate, VectorCatalog, VectorPlanner,
    VectorProjectionDescriptor,
};
use rrd_core::{
    digest, Error, ProjectionId, ProjectionStamp, ProjectionState, Result,
    DATA_RUNTIME_CONTRACT_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum VectorArtifact {
    ExactSegment(ImmutableVectorSegment),
    CompactDense(CompactDenseSegment),
    Hnsw(HnswIndex),
    Quantized(QuantizedSegment),
    TurboQuant(TurboQuantSegment),
}

/// Exact on-disk codec identity for a cataloged vector artifact. A projection
/// descriptor alone cannot distinguish the JSON exact segment from the compact
/// dense representation because both intentionally plan as `ExactSegment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorArtifactKind {
    ExactSegment,
    CompactDense,
    Hnsw,
    ScalarQuantized,
    ProductQuantized,
    BinaryQuantized,
    TurboQuant,
}

impl VectorArtifactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExactSegment => "exact_segment",
            Self::CompactDense => "compact_dense",
            Self::Hnsw => "hnsw",
            Self::ScalarQuantized => "scalar_quantized",
            Self::ProductQuantized => "product_quantized",
            Self::BinaryQuantized => "binary_quantized",
            Self::TurboQuant => "turboquant",
        }
    }

    pub const fn media_type(self) -> &'static str {
        match self {
            Self::ExactSegment => "application/vnd.rrflow.vector-exact-segment+json",
            Self::CompactDense => "application/vnd.rrflow.vector-compact-dense",
            Self::Hnsw => "application/vnd.rrflow.vector-hnsw+json",
            Self::ScalarQuantized => "application/vnd.rrflow.vector-quantized-scalar",
            Self::ProductQuantized => "application/vnd.rrflow.vector-quantized-product",
            Self::BinaryQuantized => "application/vnd.rrflow.vector-quantized-binary",
            Self::TurboQuant => "application/vnd.rrflow.vector-turboquant",
        }
    }
}

impl VectorArtifact {
    pub fn kind(&self) -> VectorArtifactKind {
        match self {
            Self::ExactSegment(_) => VectorArtifactKind::ExactSegment,
            Self::CompactDense(_) => VectorArtifactKind::CompactDense,
            Self::Hnsw(_) => VectorArtifactKind::Hnsw,
            Self::Quantized(segment) => match segment.descriptor().method {
                QuantizationMethod::Scalar => VectorArtifactKind::ScalarQuantized,
                QuantizationMethod::Product { .. } => VectorArtifactKind::ProductQuantized,
                QuantizationMethod::Binary => VectorArtifactKind::BinaryQuantized,
            },
            Self::TurboQuant(_) => VectorArtifactKind::TurboQuant,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::ExactSegment(segment) => segment.as_bytes(),
            Self::CompactDense(segment) => segment.as_bytes(),
            Self::Hnsw(index) => index.as_bytes(),
            Self::Quantized(segment) => segment.as_bytes(),
            Self::TurboQuant(segment) => segment.as_bytes(),
        }
    }

    pub fn from_bytes(kind: VectorArtifactKind, bytes: &[u8]) -> Result<Self> {
        match kind {
            VectorArtifactKind::ExactSegment => {
                ImmutableVectorSegment::from_bytes(bytes).map(Self::ExactSegment)
            }
            VectorArtifactKind::CompactDense => {
                CompactDenseSegment::from_bytes(bytes).map(Self::CompactDense)
            }
            VectorArtifactKind::Hnsw => HnswIndex::from_bytes(bytes).map(Self::Hnsw),
            VectorArtifactKind::ScalarQuantized
            | VectorArtifactKind::ProductQuantized
            | VectorArtifactKind::BinaryQuantized => {
                let artifact = QuantizedSegment::from_bytes(bytes).map(Self::Quantized)?;
                if artifact.kind() != kind {
                    return invalid("quantized artifact method differs from its codec kind");
                }
                Ok(artifact)
            }
            VectorArtifactKind::TurboQuant => {
                TurboQuantSegment::from_bytes(bytes).map(Self::TurboQuant)
            }
        }
    }

    /// Opens codecs with a native read-only mmap representation. JSON codecs
    /// deliberately return `None`; callers retain the verified owned-byte
    /// fallback rather than pretending they are physically mapped.
    pub fn open_mmap(kind: VectorArtifactKind, path: impl AsRef<Path>) -> Result<Option<Self>> {
        let path = path.as_ref();
        match kind {
            VectorArtifactKind::CompactDense => CompactDenseSegment::open_mmap(path)
                .map(Self::CompactDense)
                .map(Some),
            VectorArtifactKind::ScalarQuantized
            | VectorArtifactKind::ProductQuantized
            | VectorArtifactKind::BinaryQuantized => {
                let artifact = QuantizedSegment::open_mmap(path).map(Self::Quantized)?;
                if artifact.kind() != kind {
                    return invalid("mapped quantized artifact method differs from its codec kind");
                }
                Ok(Some(artifact))
            }
            VectorArtifactKind::TurboQuant => TurboQuantSegment::open_mmap(path)
                .map(Self::TurboQuant)
                .map(Some),
            VectorArtifactKind::ExactSegment | VectorArtifactKind::Hnsw => Ok(None),
        }
    }

    pub fn descriptor(&self) -> VectorProjectionDescriptor {
        match self {
            Self::ExactSegment(segment) => segment.descriptor().clone().into(),
            Self::CompactDense(segment) => segment.descriptor().clone().into(),
            Self::Hnsw(index) => index.descriptor().clone().into(),
            Self::Quantized(segment) => segment.descriptor().clone().into(),
            Self::TurboQuant(segment) => segment.descriptor().clone().into(),
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

impl From<QuantizedSegment> for VectorArtifact {
    fn from(segment: QuantizedSegment) -> Self {
        Self::Quantized(segment)
    }
}

impl From<TurboQuantSegment> for VectorArtifact {
    fn from(segment: TurboQuantSegment) -> Self {
        Self::TurboQuant(segment)
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
    artifacts: BTreeMap<(ProjectionId, u64), Arc<VectorArtifact>>,
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

    pub fn artifact(&self, id: &ProjectionId, generation: u64) -> Option<&VectorArtifact> {
        self.artifacts
            .get(&(id.clone(), generation))
            .map(Arc::as_ref)
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
        self.artifacts.insert(key, Arc::new(artifact));
        Ok(revision)
    }

    /// Installs the one generation selected by an already-validated durable
    /// lifecycle catalogue during restart.
    pub fn restore_active(&mut self, artifact: impl Into<VectorArtifact>) -> Result<u64> {
        let artifact = artifact.into();
        let descriptor = artifact.descriptor();
        let key = (descriptor.stamp().id.clone(), descriptor.stamp().generation);
        if self.artifacts.contains_key(&key) {
            return invalid("restored vector artifact generation is already installed");
        }
        let revision = self.catalog.restore_active(descriptor)?;
        self.artifacts.insert(key, Arc::new(artifact));
        Ok(revision)
    }

    /// Replays one durable generic-catalogue descriptor without materializing
    /// its immutable object bytes. Revisions remain exact and planning can run
    /// before a residency policy chooses which artifact to load.
    pub fn replay_catalog_descriptor(
        &mut self,
        expected_revision: u64,
        descriptor: VectorProjectionDescriptor,
    ) -> Result<u64> {
        self.catalog.publish(expected_revision, descriptor)
    }

    /// Restores one lifecycle-selected descriptor without loading bytes.
    pub fn restore_active_descriptor(
        &mut self,
        descriptor: VectorProjectionDescriptor,
    ) -> Result<u64> {
        self.catalog.restore_active(descriptor)
    }

    /// Installs bytes selected by a physical residency authority without
    /// changing catalogue identity or revision.
    pub fn install_loaded_artifact(&mut self, artifact: Arc<VectorArtifact>) -> Result<()> {
        let descriptor = artifact.descriptor();
        let key = (descriptor.stamp().id.clone(), descriptor.stamp().generation);
        let published = self
            .catalog
            .entries
            .get(&descriptor.stamp().id)
            .ok_or_else(|| Error::InvalidRuntime {
                reason: "loaded vector artifact has no published descriptor".into(),
            })?;
        if published != &descriptor || self.artifacts.contains_key(&key) {
            return invalid("loaded vector artifact is duplicate or differs from its descriptor");
        }
        self.artifacts.insert(key, artifact);
        Ok(())
    }

    /// Removes one path from this process-local serving view without changing
    /// its durable catalogue revision. Used only when a bounded physical
    /// residency policy cannot admit that path for the current request.
    pub fn suppress_projection(&mut self, id: &ProjectionId) -> bool {
        let Some(descriptor) = self.catalog.entries.remove(id) else {
            return false;
        };
        self.artifacts
            .remove(&(id.clone(), descriptor.stamp().generation));
        true
    }

    /// Removes TurboQuant projections reconstructed from the retired generic
    /// vector-artifact catalogue. Their durable records remain readable for
    /// migration and audit, but only the quantization lifecycle may install a
    /// TurboQuant serving view.
    pub fn suppress_legacy_turboquant(&mut self) -> usize {
        let ids = self
            .catalog
            .entries
            .iter()
            .filter_map(|(id, descriptor)| {
                matches!(descriptor, VectorProjectionDescriptor::TurboQuant { .. })
                    .then_some(id.clone())
            })
            .collect::<Vec<_>>();
        for id in &ids {
            self.catalog.entries.remove(id);
        }
        self.catalog.retired.retain(|descriptor| {
            !matches!(descriptor, VectorProjectionDescriptor::TurboQuant { .. })
        });
        self.artifacts
            .retain(|_, artifact| !matches!(artifact.as_ref(), VectorArtifact::TurboQuant(_)));
        ids.len()
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
                    .and_then(|artifact| match artifact.as_ref() {
                        VectorArtifact::Hnsw(index) => {
                            index.estimated_search_cost(request, ef_search).ok()
                        }
                        VectorArtifact::ExactSegment(_)
                        | VectorArtifact::CompactDense(_)
                        | VectorArtifact::Quantized(_)
                        | VectorArtifact::TurboQuant(_) => None,
                    })
                    .unwrap_or(descriptor.nodes.max(1) as u64),
                VectorProjectionDescriptor::TurboQuant { descriptor } => {
                    descriptor.candidate_versions.max(1) as u64
                }
                VectorProjectionDescriptor::Quantized { descriptor } => {
                    descriptor.candidate_versions.max(1) as u64
                }
            };
            let mut path = descriptor.candidate_path(estimated_cost);
            if let VectorProjectionDescriptor::Hnsw { descriptor } = descriptor {
                let overlay_candidates = self
                    .canonical
                    .iter()
                    .filter(|candidate| {
                        candidate.source_cursor > descriptor.stamp.source_cursor
                            && candidate.source_cursor <= required_source_cursor
                            && hnsw_covers_candidate(descriptor, candidate)
                    })
                    .count();
                if overlay_candidates != 0 {
                    path.overlay_source_cursor = Some(required_source_cursor);
                    path.overlay_candidates =
                        u64::try_from(overlay_candidates).map_err(|_| Error::InvalidRuntime {
                            reason: "HNSW overlay candidate count exceeds u64".into(),
                        })?;
                    path.estimated_candidates = path
                        .estimated_candidates
                        .saturating_add(path.overlay_candidates);
                    path.estimated_cost =
                        path.estimated_cost.saturating_add(path.overlay_candidates);
                }
            }
            paths.push(path);
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
            AccessPathKind::ExactSegment
            | AccessPathKind::Hnsw
            | AccessPathKind::ScalarQuantized
            | AccessPathKind::ProductQuantized
            | AccessPathKind::BinaryQuantized
            | AccessPathKind::TurboQuant => {
                let key = (plan.selected.id.clone(), plan.selected.generation);
                let artifact =
                    self.artifacts
                        .get(&key)
                        .ok_or_else(|| rrd_core::Error::InvalidRuntime {
                            reason: "selected vector artifact bytes are absent".into(),
                        })?;
                let published = self.catalog.entries.get(&plan.selected.id).ok_or_else(|| {
                    rrd_core::Error::InvalidRuntime {
                        reason: "selected vector projection is absent from the catalog".into(),
                    }
                })?;
                if artifact.descriptor() != *published {
                    return invalid("selected vector artifact differs from its catalog descriptor");
                }
                match (plan.selected.kind, artifact.as_ref()) {
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
                        let base = index.search_candidates_with_kernel(
                            request,
                            prepared.ef_search,
                            HnswKernel::Auto,
                        )?;
                        search_exact_ref(
                            request,
                            base.iter().chain(self.canonical.iter().filter(|candidate| {
                                candidate.source_cursor > index.descriptor().stamp.source_cursor
                                    && candidate.source_cursor <= plan.required_source_cursor
                                    && hnsw_covers_candidate(index.descriptor(), candidate)
                            })),
                        )?
                    }
                    (
                        AccessPathKind::ScalarQuantized
                        | AccessPathKind::ProductQuantized
                        | AccessPathKind::BinaryQuantized,
                        VectorArtifact::Quantized(segment),
                    ) => {
                        let approximate = segment.search_candidates_at(
                            request,
                            plan.selected.exact_rerank,
                            plan.required_source_cursor,
                            QuantizedKernel::Auto,
                        )?;
                        let references = approximate
                            .into_iter()
                            .map(|hit| hit.reference)
                            .collect::<BTreeSet<_>>();
                        search_exact_ref(
                            request,
                            self.canonical.iter().filter(|candidate| {
                                references.contains(&candidate.vector.reference)
                            }),
                        )?
                    }
                    (AccessPathKind::TurboQuant, VectorArtifact::TurboQuant(segment)) => {
                        let approximate = segment.search_candidates_at(
                            request,
                            plan.selected.exact_rerank,
                            plan.required_source_cursor,
                        )?;
                        let references = approximate
                            .into_iter()
                            .map(|hit| hit.reference)
                            .collect::<BTreeSet<_>>();
                        search_exact_ref(
                            request,
                            self.canonical.iter().filter(|candidate| {
                                references.contains(&candidate.vector.reference)
                            }),
                        )?
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

fn hnsw_covers_candidate(descriptor: &crate::HnswDescriptor, candidate: &VectorCandidate) -> bool {
    candidate.scope == descriptor.scope
        && candidate.vector.field == descriptor.field
        && candidate.vector.value.dimensions() == descriptor.dimensions
        && candidate.matches_model(descriptor.embedding_model.as_ref())
        && matches!(candidate.vector.value, rrd_core::VectorValue::Dense { .. })
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
    let mut bytes = b"rrflow-prepared-vector-search-v1\0".to_vec();
    bytes.extend_from_slice(&encoded);
    Ok(digest::sha256_hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        HnswConfig, ScoreMetric, SearchMode, TurboQuantBits, TurboQuantSegmentConfig, VectorQuery,
        VectorSegmentConfig,
    };
    use rrd_core::{ReadStamp, RuntimeProperties, RuntimeRef, RuntimeVector, ScopeId, VectorValue};

    fn candidate(scope: &ScopeId, cursor: u64, id: &str, values: Vec<f32>) -> VectorCandidate {
        VectorCandidate {
            scope: scope.clone(),
            source_cursor: cursor,
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", id).unwrap(),
                subject: RuntimeRef::new("document", id).unwrap(),
                collection: None,
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

    #[test]
    fn every_artifact_codec_round_trips_with_exact_descriptor_identity() {
        let scope = ScopeId::new("instance:vector-codecs").unwrap();
        let values = vec![
            candidate(&scope, 1, "a", vec![1.0, 0.0]),
            candidate(&scope, 2, "b", vec![0.0, 1.0]),
        ];
        let segment_config = VectorSegmentConfig {
            id: ProjectionId::new("vector:exact:codec").unwrap(),
            scope: scope.clone(),
            field: "body".into(),
            dimensions: 2,
            metric: ScoreMetric::Dot,
            embedding_model: None,
            filter_properties: BTreeSet::new(),
        };
        let exact = VectorArtifact::from(
            ImmutableVectorSegment::build(segment_config.clone(), 1, 2, values.clone()).unwrap(),
        );
        let compact = VectorArtifact::from(
            CompactDenseSegment::build(segment_config, 1, 2, values.clone()).unwrap(),
        );
        let hnsw = VectorArtifact::from(
            HnswIndex::build(
                HnswConfig {
                    id: ProjectionId::new("vector:hnsw:codec").unwrap(),
                    scope: scope.clone(),
                    field: "body".into(),
                    dimensions: 2,
                    metric: ScoreMetric::Dot,
                    embedding_model: None,
                    m: 2,
                    ef_construction: 4,
                    max_level: 3,
                    seed: 11,
                    filter_properties: BTreeSet::new(),
                },
                1,
                2,
                values.clone(),
            )
            .unwrap(),
        );
        let turboquant = VectorArtifact::from(
            TurboQuantSegment::build(
                TurboQuantSegmentConfig {
                    id: ProjectionId::new("vector:turbo:codec").unwrap(),
                    scope,
                    field: "body".into(),
                    dimensions: 2,
                    metric: ScoreMetric::Dot,
                    bits: TurboQuantBits::Bits4,
                    seed: 11,
                    embedding_model: None,
                    filter_properties: BTreeSet::new(),
                },
                1,
                2,
                values,
            )
            .unwrap(),
        );

        for artifact in [exact, compact, hnsw, turboquant] {
            let reopened =
                VectorArtifact::from_bytes(artifact.kind(), artifact.as_bytes()).unwrap();
            assert_eq!(reopened.kind(), artifact.kind());
            assert_eq!(reopened.descriptor(), artifact.descriptor());
            assert_eq!(reopened.as_bytes(), artifact.as_bytes());
        }
    }

    #[test]
    fn turboquant_is_planner_selected_and_exactly_reranked() {
        let scope = ScopeId::new("instance:turbo-runtime").unwrap();
        let values = vec![
            candidate(&scope, 1, "a", vec![1.0, 0.0]),
            candidate(&scope, 2, "b", vec![0.0, 1.0]),
            candidate(&scope, 3, "c", vec![0.5, 0.5]),
        ];
        let mut runtime = VectorRuntime::new(values.clone()).unwrap();
        let artifact = TurboQuantSegment::build(
            TurboQuantSegmentConfig {
                id: ProjectionId::new("vector:turbo:runtime").unwrap(),
                scope: scope.clone(),
                field: "body".into(),
                dimensions: 2,
                metric: ScoreMetric::Dot,
                bits: TurboQuantBits::Bits4,
                seed: 77,
                embedding_model: None,
                filter_properties: BTreeSet::new(),
            },
            1,
            3,
            values,
        )
        .unwrap();
        runtime.publish(0, artifact).unwrap();
        let execution = runtime
            .search(
                &request(scope, SearchMode::RequireApproximate { exact_rerank: 2 }),
                8,
            )
            .unwrap();
        assert_eq!(execution.plan.selected.kind, AccessPathKind::TurboQuant);
        assert_eq!(execution.hits[0].reference.id.as_str(), "a");
        assert_eq!(execution.hits[0].score, 1.0);
    }

    #[test]
    fn lifecycle_restore_is_revision_neutral_and_legacy_turbo_is_suppressed() {
        let scope = ScopeId::new("instance:quantization-authority").unwrap();
        let values = vec![
            candidate(&scope, 1, "a", vec![1.0, 0.0]),
            candidate(&scope, 2, "b", vec![0.0, 1.0]),
        ];
        let mut runtime = VectorRuntime::new(values.clone()).unwrap();
        let hnsw = HnswIndex::build(
            HnswConfig {
                id: ProjectionId::new("vector:hnsw:authority").unwrap(),
                scope: scope.clone(),
                field: "body".into(),
                dimensions: 2,
                metric: ScoreMetric::Dot,
                embedding_model: None,
                m: 2,
                ef_construction: 4,
                max_level: 3,
                seed: 7,
                filter_properties: BTreeSet::new(),
            },
            1,
            2,
            values.clone(),
        )
        .unwrap();
        assert_eq!(runtime.publish(0, hnsw).unwrap(), 1);
        let legacy = TurboQuantSegment::build(
            TurboQuantSegmentConfig {
                id: ProjectionId::new("vector:legacy-turbo:authority").unwrap(),
                scope: scope.clone(),
                field: "body".into(),
                dimensions: 2,
                metric: ScoreMetric::Dot,
                bits: TurboQuantBits::Bits2,
                seed: 11,
                embedding_model: None,
                filter_properties: BTreeSet::new(),
            },
            1,
            2,
            values.clone(),
        )
        .unwrap();
        assert_eq!(runtime.publish(1, legacy).unwrap(), 2);
        assert_eq!(runtime.suppress_legacy_turboquant(), 1);

        let lifecycle = TurboQuantSegment::build(
            TurboQuantSegmentConfig {
                id: ProjectionId::new("quant-turbo:authority").unwrap(),
                scope: scope.clone(),
                field: "body".into(),
                dimensions: 2,
                metric: ScoreMetric::Dot,
                bits: TurboQuantBits::Bits2,
                seed: 13,
                embedding_model: None,
                filter_properties: BTreeSet::new(),
            },
            1,
            2,
            values.clone(),
        )
        .unwrap();
        assert_eq!(runtime.restore_active(lifecycle).unwrap(), 2);
        assert_eq!(runtime.catalog().revision, 2);

        let exact = ImmutableVectorSegment::build(
            VectorSegmentConfig {
                id: ProjectionId::new("vector:exact:authority").unwrap(),
                scope,
                field: "body".into(),
                dimensions: 2,
                metric: ScoreMetric::Dot,
                embedding_model: None,
                filter_properties: BTreeSet::new(),
            },
            1,
            2,
            values,
        )
        .unwrap();
        assert_eq!(runtime.publish(2, exact).unwrap(), 3);
    }
}
