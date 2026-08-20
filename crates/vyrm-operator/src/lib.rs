//! Project-scoped external operator knowledge.
//!
//! Vyrm owns policy, evidence, and freshness decisions. An adapter owns its
//! external transaction and returns bounded identities plus plan evidence. No
//! type in this crate claims a transaction spanning Vyrm and the external
//! system.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::{
    digest, Error, ProjectionFamily, ProjectionStamp, ProjectionState, ProjectionWork, Result,
    ScopeId,
};
use vyrm_vector::{EmbeddingModelBinding, ScoreMetric, SearchMode, SearchRequest, VectorRuntime};

pub const OPERATOR_KNOWLEDGE_CONTRACT_VERSION: u16 = 1;
const MAX_NAME_BYTES: usize = 160;
const MAX_HITS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorAccessPath {
    Exact,
    Hnsw,
    IvfFlat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorVectorKind {
    Dense,
    Sparse,
    MultiDense,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IterativeScanMode {
    Off,
    StrictOrder,
    RelaxedOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorAdapterDescriptor {
    pub contract_version: u16,
    pub adapter: String,
    pub implementation_digest: String,
    pub max_dimensions: u32,
    pub vector_kinds: BTreeSet<OperatorVectorKind>,
    pub search_capabilities: BTreeMap<OperatorAccessPath, BTreeSet<ScoreMetric>>,
    pub supports_tenant_filter: bool,
    pub supports_payload_filter: bool,
    pub supports_stable_revision: bool,
}

impl OperatorAdapterDescriptor {
    pub fn validate(&self) -> Result<()> {
        validate_version(self.contract_version)?;
        validate_name("operator adapter", &self.adapter)?;
        validate_digest("operator implementation", &self.implementation_digest)?;
        if self.max_dimensions == 0 {
            return invalid("operator adapter max dimensions must be greater than zero");
        }
        if self.vector_kinds.is_empty()
            || self.search_capabilities.is_empty()
            || self.search_capabilities.values().any(BTreeSet::is_empty)
        {
            return invalid("operator adapter must declare vector kinds and path-specific metrics");
        }
        Ok(())
    }
}

/// Immutable project-to-external-source binding. Human-readable database,
/// schema, table, column, and tenant names stay in deployment configuration;
/// only their canonical digests cross the kernel boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorKnowledgeBinding {
    pub contract_version: u16,
    pub adapter: String,
    pub project_id: String,
    pub member: String,
    pub scope: ScopeId,
    pub config_digest: String,
    pub source_identity_digest: String,
    pub relation_digest: String,
    pub tenant_digest: String,
    pub model: EmbeddingModelBinding,
    pub dimensions: u32,
    pub projection: ProjectionStamp,
}

impl OperatorKnowledgeBinding {
    pub fn validate(&self) -> Result<()> {
        validate_version(self.contract_version)?;
        validate_name("operator adapter", &self.adapter)?;
        validate_name("operator project", &self.project_id)?;
        validate_name("operator member", &self.member)?;
        validate_digest("operator config", &self.config_digest)?;
        validate_digest("operator source identity", &self.source_identity_digest)?;
        validate_digest("operator relation", &self.relation_digest)?;
        validate_digest("operator tenant", &self.tenant_digest)?;
        self.model.validate()?;
        if self.dimensions == 0 {
            return invalid("operator binding dimensions must be greater than zero");
        }
        self.projection.validate()?;
        if self.projection.source_cursor == 0
            || self.projection.state != ProjectionState::Ready
            || self.projection.config_digest != self.config_digest
        {
            return invalid(
                "operator projection must be ready, configuration-bound, and cover a non-zero source cursor",
            );
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        content_digest(b"vyrm-operator-binding-v1\0", self)
    }
}

/// The exact external visibility coordinate observed by one search. Snapshot
/// identity is mandatory; WAL position is optional supporting evidence and is
/// never treated as an equivalent snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSourceRevision {
    pub contract_version: u16,
    pub adapter: String,
    pub project_id: String,
    pub source_identity_digest: String,
    pub snapshot_digest: String,
    pub catalog_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wal_lsn_digest: Option<String>,
}

impl OperatorSourceRevision {
    pub fn validate(&self) -> Result<()> {
        validate_version(self.contract_version)?;
        validate_name("operator revision adapter", &self.adapter)?;
        validate_name("operator revision project", &self.project_id)?;
        validate_digest("operator source identity", &self.source_identity_digest)?;
        validate_digest("operator snapshot", &self.snapshot_digest)?;
        validate_digest("operator catalog", &self.catalog_digest)?;
        if let Some(revision) = &self.stable_revision {
            validate_name("operator stable revision", revision)?;
        }
        if let Some(lsn) = &self.wal_lsn_digest {
            validate_digest("operator WAL LSN", lsn)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        content_digest(b"vyrm-operator-source-revision-v1\0", self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSearchControls {
    pub requested_path: OperatorAccessPath,
    pub iterative_scan: IterativeScanMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hnsw_ef_search: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hnsw_max_scan_tuples: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ivfflat_probes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ivfflat_max_probes: Option<u32>,
}

impl OperatorSearchControls {
    pub fn exact() -> Self {
        Self {
            requested_path: OperatorAccessPath::Exact,
            iterative_scan: IterativeScanMode::Off,
            hnsw_ef_search: None,
            hnsw_max_scan_tuples: None,
            ivfflat_probes: None,
            ivfflat_max_probes: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self.requested_path {
            OperatorAccessPath::Exact => {
                if self.iterative_scan != IterativeScanMode::Off
                    || self.hnsw_ef_search.is_some()
                    || self.hnsw_max_scan_tuples.is_some()
                    || self.ivfflat_probes.is_some()
                    || self.ivfflat_max_probes.is_some()
                {
                    return invalid("exact operator search cannot declare ANN controls");
                }
            }
            OperatorAccessPath::Hnsw => {
                bounded_positive("hnsw ef_search", self.hnsw_ef_search, 1_000_000)?;
                bounded_positive(
                    "hnsw max_scan_tuples",
                    self.hnsw_max_scan_tuples,
                    100_000_000,
                )?;
                if self.ivfflat_probes.is_some() || self.ivfflat_max_probes.is_some() {
                    return invalid("HNSW operator search cannot declare IVFFlat controls");
                }
            }
            OperatorAccessPath::IvfFlat => {
                bounded_positive("ivfflat probes", self.ivfflat_probes, 1_000_000)?;
                bounded_positive("ivfflat max_probes", self.ivfflat_max_probes, 1_000_000)?;
                if let (Some(probes), Some(max_probes)) =
                    (self.ivfflat_probes, self.ivfflat_max_probes)
                {
                    if max_probes < probes {
                        return invalid("ivfflat max_probes cannot be lower than probes");
                    }
                }
                if self.hnsw_ef_search.is_some() || self.hnsw_max_scan_tuples.is_some() {
                    return invalid("IVFFlat operator search cannot declare HNSW controls");
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSearchRequest {
    pub contract_version: u16,
    pub binding_digest: String,
    /// Lowest canonical Vyrm cursor the external projection must cover.
    pub required_source_cursor: u64,
    pub search: SearchRequest,
    pub controls: OperatorSearchControls,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_stable_revision: Option<String>,
}

impl OperatorSearchRequest {
    pub fn validate(&self) -> Result<()> {
        validate_version(self.contract_version)?;
        validate_digest("operator binding", &self.binding_digest)?;
        self.search.validate()?;
        if self.required_source_cursor == 0
            || self.required_source_cursor > self.search.read.commit_cursor
        {
            return invalid(
                "operator required source cursor must be non-zero and within the read stamp",
            );
        }
        self.controls.validate()?;
        match (self.search.mode, self.controls.requested_path) {
            (SearchMode::Exact, OperatorAccessPath::Exact)
            | (
                SearchMode::AllowApproximate { .. } | SearchMode::RequireApproximate { .. },
                OperatorAccessPath::Hnsw | OperatorAccessPath::IvfFlat,
            ) => {}
            _ => {
                return invalid("operator vector mode and requested external access path disagree")
            }
        }
        if let Some(revision) = &self.expected_stable_revision {
            validate_name("expected operator revision", revision)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        content_digest(b"vyrm-operator-search-request-v1\0", self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSearchHit {
    pub external_id: String,
    pub subject_id: String,
    pub score: f64,
}

impl OperatorSearchHit {
    fn validate(&self) -> Result<()> {
        validate_name("operator hit identity", &self.external_id)?;
        validate_name("operator hit subject", &self.subject_id)?;
        if !self.score.is_finite() {
            return invalid("operator hit score must be finite");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorPlanEvidence {
    pub selected_path: OperatorAccessPath,
    pub plan_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason_digest: Option<String>,
    pub controls: OperatorSearchControls,
    pub filter_applied_after_ann: bool,
    pub ordering_exact: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidates_examined: Option<u64>,
}

impl OperatorPlanEvidence {
    fn validate(&self) -> Result<()> {
        validate_digest("operator plan", &self.plan_digest)?;
        if let Some(index) = &self.index_digest {
            validate_digest("operator index", index)?;
        }
        if let Some(reason) = &self.fallback_reason_digest {
            validate_digest("operator fallback reason", reason)?;
        }
        self.controls.validate()?;
        if self.controls.requested_path != self.selected_path
            && self.fallback_reason_digest.is_none()
        {
            return invalid("operator access-path fallback requires a reason digest");
        }
        if self.selected_path == OperatorAccessPath::Exact && self.filter_applied_after_ann {
            return invalid("exact operator search cannot report post-ANN filtering");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSearchResult {
    pub contract_version: u16,
    pub request_digest: String,
    pub revision: OperatorSourceRevision,
    pub plan: OperatorPlanEvidence,
    pub hits: Vec<OperatorSearchHit>,
    pub elapsed_micros: u64,
}

impl OperatorSearchResult {
    pub fn validate(&self) -> Result<()> {
        validate_version(self.contract_version)?;
        validate_digest("operator request", &self.request_digest)?;
        self.revision.validate()?;
        self.plan.validate()?;
        if self.hits.len() > MAX_HITS {
            return invalid("operator result exceeds the hit budget");
        }
        let mut seen = BTreeSet::new();
        for hit in &self.hits {
            hit.validate()?;
            if !seen.insert((&hit.external_id, &hit.subject_id)) {
                return invalid("operator result contains a duplicate hit identity");
            }
        }
        Ok(())
    }
}

pub trait OperatorKnowledgeAdapter {
    fn descriptor(&self) -> &OperatorAdapterDescriptor;

    fn search(
        &mut self,
        binding: &OperatorKnowledgeBinding,
        request: &OperatorSearchRequest,
    ) -> Result<OperatorSearchResult>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorSyncOperation {
    UpsertVector,
}

/// Idempotent work derived from Vyrm's already-committed projection outbox.
/// Payload bytes remain caller-owned; the durable identity binds the exact
/// canonical mutation and payload digest that an adapter must apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSyncWork {
    pub contract_version: u16,
    pub id: String,
    pub binding_digest: String,
    pub scope: ScopeId,
    pub source_cursor: u64,
    pub source_commit_id: String,
    pub source_commit_ordinal: u64,
    pub source_outbox_id: String,
    pub source_change_digest: String,
    pub payload_digest: String,
    pub operation: OperatorSyncOperation,
}

impl OperatorSyncWork {
    pub fn for_vector(
        binding: &OperatorKnowledgeBinding,
        source: &ProjectionWork,
        source_change_digest: impl Into<String>,
        payload_digest: impl Into<String>,
    ) -> Result<Self> {
        binding.validate()?;
        source.validate()?;
        if source.family != ProjectionFamily::Vector || source.scope != binding.scope {
            return invalid("operator sync source is not vector work for the bound scope");
        }
        let source_change_digest = source_change_digest.into();
        let payload_digest = payload_digest.into();
        validate_digest("operator sync source change", &source_change_digest)?;
        validate_digest("operator sync payload", &payload_digest)?;
        let mut work = Self {
            contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
            id: String::new(),
            binding_digest: binding.digest()?,
            scope: source.scope.clone(),
            source_cursor: source.source_cursor,
            source_commit_id: source.commit_id.clone(),
            source_commit_ordinal: source.commit_ordinal,
            source_outbox_id: source.id.to_string(),
            source_change_digest,
            payload_digest,
            operation: OperatorSyncOperation::UpsertVector,
        };
        work.id = work.expected_id()?;
        work.validate()?;
        Ok(work)
    }

    pub fn validate(&self) -> Result<()> {
        validate_version(self.contract_version)?;
        for (kind, value) in [
            ("operator sync identity", &self.id),
            ("operator sync binding", &self.binding_digest),
            ("operator sync commit", &self.source_commit_id),
            ("operator sync outbox", &self.source_outbox_id),
            ("operator sync source change", &self.source_change_digest),
            ("operator sync payload", &self.payload_digest),
        ] {
            validate_digest(kind, value)?;
        }
        if self.source_cursor == 0 || self.id != self.expected_id()? {
            return invalid("operator sync identity or source cursor is invalid");
        }
        Ok(())
    }

    fn expected_id(&self) -> Result<String> {
        content_digest(
            b"vyrm-operator-sync-work-v1\0",
            &(
                &self.binding_digest,
                &self.scope,
                self.source_cursor,
                &self.source_commit_id,
                self.source_commit_ordinal,
                &self.source_outbox_id,
                &self.source_change_digest,
                &self.payload_digest,
                self.operation,
            ),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSyncReceipt {
    pub contract_version: u16,
    pub work_id: String,
    pub revision: OperatorSourceRevision,
    pub applied_now: bool,
    pub idempotent_replay: bool,
}

impl OperatorSyncReceipt {
    pub fn validate(&self) -> Result<()> {
        validate_version(self.contract_version)?;
        validate_digest("operator sync receipt work", &self.work_id)?;
        self.revision.validate()?;
        if self.applied_now == self.idempotent_replay {
            return invalid(
                "operator sync receipt must be either a new apply or an idempotent replay",
            );
        }
        Ok(())
    }
}

pub trait OperatorKnowledgeWriter {
    fn descriptor(&self) -> &OperatorAdapterDescriptor;

    fn apply(
        &mut self,
        binding: &OperatorKnowledgeBinding,
        work: &OperatorSyncWork,
        payload: &[u8],
    ) -> Result<OperatorSyncReceipt>;
}

pub fn execute_operator_sync<W: OperatorKnowledgeWriter>(
    writer: &mut W,
    binding: &OperatorKnowledgeBinding,
    work: &OperatorSyncWork,
    payload: &[u8],
) -> Result<OperatorSyncReceipt> {
    binding.validate()?;
    work.validate()?;
    let descriptor = writer.descriptor().clone();
    descriptor.validate()?;
    if descriptor.adapter != binding.adapter || work.binding_digest != binding.digest()? {
        return invalid("operator sync writer or work uses another project binding");
    }
    if work.scope != binding.scope || digest::sha256_hex(payload) != work.payload_digest {
        return invalid("operator sync payload or scope differs from its durable work identity");
    }
    let receipt = writer.apply(binding, work, payload)?;
    if writer.descriptor() != &descriptor {
        return invalid("operator sync writer changed its descriptor during execution");
    }
    receipt.validate()?;
    if receipt.work_id != work.id
        || receipt.revision.adapter != binding.adapter
        || receipt.revision.project_id != binding.project_id
        || receipt.revision.source_identity_digest != binding.source_identity_digest
    {
        return invalid("operator sync receipt differs from its work or project binding");
    }
    Ok(receipt)
}

/// Deterministic idempotency oracle for external writers. It models the
/// adapter-side unique work-id table required by a pgvector implementation.
pub struct ReferenceOperatorWriter {
    descriptor: OperatorAdapterDescriptor,
    binding_digest: String,
    revision: OperatorSourceRevision,
    applied: BTreeMap<String, OperatorSyncReceipt>,
    apply_count: u64,
}

impl ReferenceOperatorWriter {
    pub fn new(
        descriptor: OperatorAdapterDescriptor,
        binding: &OperatorKnowledgeBinding,
        revision: OperatorSourceRevision,
    ) -> Result<Self> {
        descriptor.validate()?;
        binding.validate()?;
        revision.validate()?;
        Ok(Self {
            descriptor,
            binding_digest: binding.digest()?,
            revision,
            applied: BTreeMap::new(),
            apply_count: 0,
        })
    }

    pub const fn apply_count(&self) -> u64 {
        self.apply_count
    }
}

impl OperatorKnowledgeWriter for ReferenceOperatorWriter {
    fn descriptor(&self) -> &OperatorAdapterDescriptor {
        &self.descriptor
    }

    fn apply(
        &mut self,
        binding: &OperatorKnowledgeBinding,
        work: &OperatorSyncWork,
        _payload: &[u8],
    ) -> Result<OperatorSyncReceipt> {
        if binding.digest()? != self.binding_digest {
            return invalid("reference operator writer is bound to another project");
        }
        if let Some(receipt) = self.applied.get(&work.id) {
            let mut replay = receipt.clone();
            replay.applied_now = false;
            replay.idempotent_replay = true;
            return Ok(replay);
        }
        self.apply_count =
            self.apply_count
                .checked_add(1)
                .ok_or_else(|| Error::InvalidRuntime {
                    reason: "reference operator apply count overflow".into(),
                })?;
        let receipt = OperatorSyncReceipt {
            contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
            work_id: work.id.clone(),
            revision: self.revision.clone(),
            applied_now: true,
            idempotent_replay: false,
        };
        self.applied.insert(work.id.clone(), receipt.clone());
        Ok(receipt)
    }
}

/// Validates both sides of the adapter call. An adapter cannot silently change
/// project, model space, source, path, or freshness policy.
pub fn execute_operator_search<A: OperatorKnowledgeAdapter>(
    adapter: &mut A,
    binding: &OperatorKnowledgeBinding,
    request: &OperatorSearchRequest,
) -> Result<OperatorSearchResult> {
    binding.validate()?;
    request.validate()?;
    let descriptor = adapter.descriptor().clone();
    descriptor.validate()?;
    if descriptor.adapter != binding.adapter {
        return invalid("operator adapter does not match the project binding");
    }
    if request.binding_digest != binding.digest()? {
        return invalid("operator request uses another project binding");
    }
    if request.search.scope != binding.scope {
        return invalid("operator request scope differs from its project binding");
    }
    if binding.projection.source_cursor < request.required_source_cursor
        || binding.projection.source_cursor > request.search.read.commit_cursor
    {
        return invalid("operator projection is stale or newer than the requested read stamp");
    }
    if request.search.query.dimensions() != binding.dimensions as usize
        || request.search.query.dimensions() > descriptor.max_dimensions as usize
    {
        return invalid("operator query dimensions differ from the bound model space");
    }
    if request.search.embedding_model.as_ref() != Some(&binding.model) {
        return invalid("operator query does not bind the exact project model space");
    }
    if !descriptor
        .vector_kinds
        .contains(&vector_kind(&request.search.query))
        || !descriptor
            .search_capabilities
            .get(&request.controls.requested_path)
            .is_some_and(|metrics| metrics.contains(&request.search.metric))
    {
        return invalid("operator adapter lacks a requested search capability");
    }
    if !descriptor.supports_tenant_filter {
        return invalid("operator adapter cannot enforce the bound tenant filter");
    }
    if request.search.filter.is_some() && !descriptor.supports_payload_filter {
        return invalid("operator adapter cannot enforce the requested payload filter");
    }
    if request.expected_stable_revision.is_some() && !descriptor.supports_stable_revision {
        return invalid("operator adapter cannot enforce a stable source revision");
    }

    let result = adapter.search(binding, request)?;
    if adapter.descriptor() != &descriptor {
        return invalid("operator adapter changed its descriptor during execution");
    }
    result.validate()?;
    if result.request_digest != request.digest()?
        || result.revision.adapter != binding.adapter
        || result.revision.project_id != binding.project_id
        || result.revision.source_identity_digest != binding.source_identity_digest
        || result.hits.len() > request.search.top_k
    {
        return invalid("operator adapter result differs from the bound request or source");
    }
    if result.plan.controls != request.controls {
        return invalid("operator adapter reported controls other than the sealed request");
    }
    if let Some(expected) = &request.expected_stable_revision {
        if result.revision.stable_revision.as_ref() != Some(expected) {
            return invalid("operator source revision is stale");
        }
    }
    if result.plan.selected_path != request.controls.requested_path
        && (!matches!(request.search.mode, SearchMode::AllowApproximate { .. })
            || result.plan.selected_path != OperatorAccessPath::Exact
            || result.plan.fallback_reason_digest.is_none())
    {
        return invalid("operator adapter performed an unauthorized access-path fallback");
    }
    if !descriptor
        .search_capabilities
        .get(&result.plan.selected_path)
        .is_some_and(|metrics| metrics.contains(&request.search.metric))
    {
        return invalid("operator adapter reported an undeclared selected access path");
    }
    Ok(result)
}

/// Deterministic adapter used as a semantic oracle for external implementations.
pub struct ReferenceOperatorAdapter {
    descriptor: OperatorAdapterDescriptor,
    binding_digest: String,
    revision: OperatorSourceRevision,
    runtime: VectorRuntime,
}

impl ReferenceOperatorAdapter {
    pub fn new(
        descriptor: OperatorAdapterDescriptor,
        binding: &OperatorKnowledgeBinding,
        revision: OperatorSourceRevision,
        runtime: VectorRuntime,
    ) -> Result<Self> {
        descriptor.validate()?;
        binding.validate()?;
        revision.validate()?;
        Ok(Self {
            descriptor,
            binding_digest: binding.digest()?,
            revision,
            runtime,
        })
    }
}

impl OperatorKnowledgeAdapter for ReferenceOperatorAdapter {
    fn descriptor(&self) -> &OperatorAdapterDescriptor {
        &self.descriptor
    }

    fn search(
        &mut self,
        binding: &OperatorKnowledgeBinding,
        request: &OperatorSearchRequest,
    ) -> Result<OperatorSearchResult> {
        if binding.digest()? != self.binding_digest {
            return invalid("reference operator adapter is bound to another project");
        }
        if request.controls.requested_path != OperatorAccessPath::Exact {
            return invalid("reference operator adapter implements the exact oracle only");
        }
        let execution = self.runtime.search(&request.search, 1)?;
        let plan_digest = content_digest(b"vyrm-reference-operator-plan-v1\0", &execution.plan)?;
        Ok(OperatorSearchResult {
            contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
            request_digest: request.digest()?,
            revision: self.revision.clone(),
            plan: OperatorPlanEvidence {
                selected_path: OperatorAccessPath::Exact,
                plan_digest,
                index_digest: None,
                fallback_reason_digest: None,
                controls: request.controls.clone(),
                filter_applied_after_ann: false,
                ordering_exact: true,
                candidates_examined: None,
            },
            hits: execution
                .hits
                .into_iter()
                .map(|hit| OperatorSearchHit {
                    external_id: format!("{}:{}", hit.reference.kind, hit.reference.id),
                    subject_id: format!("{}:{}", hit.subject.kind, hit.subject.id),
                    score: hit.score,
                })
                .collect(),
            elapsed_micros: 0,
        })
    }
}

/// Safe, parameterized pgvector query shape. Identifiers are quoted from a
/// validated deployment binding; query vector, tenant, and limit remain `$1`,
/// `$2`, and `$3` parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgvectorSqlPlan {
    pub settings: Vec<(String, String)>,
    pub explain_sql: String,
    pub search_sql: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PgvectorRelation {
    pub schema: String,
    pub relation: String,
    pub id_column: String,
    pub subject_column: String,
    pub vector_column: String,
    pub tenant_column: String,
}

impl PgvectorRelation {
    pub fn validate(&self) -> Result<()> {
        for (kind, value) in [
            ("pgvector schema", &self.schema),
            ("pgvector relation", &self.relation),
            ("pgvector id column", &self.id_column),
            ("pgvector subject column", &self.subject_column),
            ("pgvector vector column", &self.vector_column),
            ("pgvector tenant column", &self.tenant_column),
        ] {
            validate_pg_identifier(kind, value)?;
        }
        Ok(())
    }

    pub fn build_search(
        &self,
        metric: ScoreMetric,
        controls: &OperatorSearchControls,
    ) -> Result<PgvectorSqlPlan> {
        self.validate()?;
        controls.validate()?;
        if controls.requested_path == OperatorAccessPath::IvfFlat
            && metric == ScoreMetric::Manhattan
        {
            return invalid("pgvector IVFFlat does not support Manhattan distance");
        }
        let operator = match metric {
            ScoreMetric::Euclidean => "<->",
            ScoreMetric::Dot => "<#>",
            ScoreMetric::Cosine => "<=>",
            ScoreMetric::Manhattan => "<+>",
        };
        let schema = quote_identifier(&self.schema);
        let relation = quote_identifier(&self.relation);
        let id = quote_identifier(&self.id_column);
        let subject = quote_identifier(&self.subject_column);
        let vector = quote_identifier(&self.vector_column);
        let tenant = quote_identifier(&self.tenant_column);
        let score = match metric {
            ScoreMetric::Dot => "distance * -1.0",
            ScoreMetric::Cosine => "1.0 - distance",
            ScoreMetric::Euclidean | ScoreMetric::Manhattan => "distance * -1.0",
        };
        let inner = format!(
            "SELECT {id}::text AS external_id, {subject}::text AS subject_id, {vector} {operator} $1::vector AS distance FROM {schema}.{relation} WHERE {tenant}::text = $2 ORDER BY {vector} {operator} $1::vector LIMIT $3"
        );
        let search_sql = format!(
            "SELECT external_id, subject_id, {score} AS score FROM ({inner}) AS vyrm_ranked ORDER BY distance ASC, external_id ASC"
        );
        let mut settings = Vec::new();
        match controls.requested_path {
            OperatorAccessPath::Exact => {
                settings.push(("enable_indexscan".into(), "off".into()));
                settings.push(("enable_bitmapscan".into(), "off".into()));
            }
            OperatorAccessPath::Hnsw => {
                push_setting(&mut settings, "hnsw.ef_search", controls.hnsw_ef_search);
                push_setting(
                    &mut settings,
                    "hnsw.max_scan_tuples",
                    controls.hnsw_max_scan_tuples,
                );
                settings.push((
                    "hnsw.iterative_scan".into(),
                    iterative_name(controls.iterative_scan).into(),
                ));
            }
            OperatorAccessPath::IvfFlat => {
                push_setting(&mut settings, "ivfflat.probes", controls.ivfflat_probes);
                push_setting(
                    &mut settings,
                    "ivfflat.max_probes",
                    controls.ivfflat_max_probes,
                );
                settings.push((
                    "ivfflat.iterative_scan".into(),
                    iterative_name(controls.iterative_scan).into(),
                ));
            }
        }
        Ok(PgvectorSqlPlan {
            settings,
            explain_sql: format!("EXPLAIN (FORMAT JSON) {search_sql}"),
            search_sql,
        })
    }
}

fn iterative_name(mode: IterativeScanMode) -> &'static str {
    match mode {
        IterativeScanMode::Off => "off",
        IterativeScanMode::StrictOrder => "strict_order",
        IterativeScanMode::RelaxedOrder => "relaxed_order",
    }
}

fn vector_kind(query: &vyrm_vector::VectorQuery) -> OperatorVectorKind {
    match query {
        vyrm_vector::VectorQuery::Dense { .. } => OperatorVectorKind::Dense,
        vyrm_vector::VectorQuery::Sparse { .. } => OperatorVectorKind::Sparse,
        vyrm_vector::VectorQuery::MultiDense { .. } => OperatorVectorKind::MultiDense,
    }
}

fn push_setting(settings: &mut Vec<(String, String)>, name: &str, value: Option<u32>) {
    if let Some(value) = value {
        settings.push((name.into(), value.to_string()));
    }
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn validate_pg_identifier(kind: &'static str, value: &str) -> Result<()> {
    validate_name(kind, value)?;
    if value.len() > 63 {
        return invalid(format!(
            "{kind} exceeds PostgreSQL's 63-byte identifier limit"
        ));
    }
    Ok(())
}

fn bounded_positive(kind: &'static str, value: Option<u32>, maximum: u32) -> Result<()> {
    if value.is_some_and(|value| value == 0 || value > maximum) {
        return invalid(format!("{kind} must be in 1..={maximum}"));
    }
    Ok(())
}

fn content_digest<T: Serialize>(domain: &[u8], value: &T) -> Result<String> {
    let encoded = serde_json::to_vec(value).map_err(|error| Error::InvalidRuntime {
        reason: format!("operator contract cannot be encoded: {error}"),
    })?;
    let mut bytes = domain.to_vec();
    bytes.extend_from_slice(&encoded);
    Ok(digest::sha256_hex(&bytes))
}

fn validate_version(version: u16) -> Result<()> {
    if version != OPERATOR_KNOWLEDGE_CONTRACT_VERSION {
        return invalid("unsupported operator-knowledge contract version");
    }
    Ok(())
}

fn validate_digest(kind: &'static str, value: &str) -> Result<()> {
    if value.len() != 64
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return invalid(format!("{kind} must be lowercase SHA-256 hex"));
    }
    Ok(())
}

fn validate_name(kind: &'static str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > MAX_NAME_BYTES || value.as_bytes().contains(&0) {
        return invalid(format!(
            "{kind} must be non-empty, NUL-free, and at most {MAX_NAME_BYTES} bytes"
        ));
    }
    Ok(())
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidRuntime {
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pgvector_sql_is_parameterized_quoted_and_path_explicit() {
        let relation = PgvectorRelation {
            schema: "operator data".into(),
            relation: "knowledge\"items".into(),
            id_column: "id".into(),
            subject_column: "subject".into(),
            vector_column: "embedding".into(),
            tenant_column: "project".into(),
        };
        let controls = OperatorSearchControls {
            requested_path: OperatorAccessPath::Hnsw,
            iterative_scan: IterativeScanMode::StrictOrder,
            hnsw_ef_search: Some(80),
            hnsw_max_scan_tuples: Some(20_000),
            ivfflat_probes: None,
            ivfflat_max_probes: None,
        };
        let plan = relation
            .build_search(ScoreMetric::Cosine, &controls)
            .unwrap();
        assert!(plan
            .search_sql
            .contains("\"operator data\".\"knowledge\"\"items\""));
        assert!(plan.search_sql.contains("$1::vector"));
        assert!(plan.search_sql.contains("$2"));
        assert!(plan.search_sql.contains("LIMIT $3"));
        assert!(!plan.search_sql.contains("strict_order"));
        assert_eq!(
            plan.settings,
            [
                ("hnsw.ef_search".into(), "80".into()),
                ("hnsw.max_scan_tuples".into(), "20000".into()),
                ("hnsw.iterative_scan".into(), "strict_order".into()),
            ]
        );
    }

    #[test]
    fn path_controls_fail_closed_instead_of_cross_applying() {
        let mut controls = OperatorSearchControls::exact();
        controls.hnsw_ef_search = Some(40);
        assert!(controls.validate().is_err());
        let controls = OperatorSearchControls {
            requested_path: OperatorAccessPath::IvfFlat,
            iterative_scan: IterativeScanMode::RelaxedOrder,
            hnsw_ef_search: None,
            hnsw_max_scan_tuples: None,
            ivfflat_probes: Some(100),
            ivfflat_max_probes: Some(10),
        };
        assert!(controls.validate().is_err());

        let relation = PgvectorRelation {
            schema: "public".into(),
            relation: "knowledge".into(),
            id_column: "id".into(),
            subject_column: "subject".into(),
            vector_column: "embedding".into(),
            tenant_column: "project".into(),
        };
        let mut controls = controls;
        controls.ivfflat_max_probes = Some(100);
        let error = relation
            .build_search(ScoreMetric::Manhattan, &controls)
            .unwrap_err();
        assert!(error.to_string().contains("does not support Manhattan"));
    }
}
