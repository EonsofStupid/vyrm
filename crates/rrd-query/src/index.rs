use crate::{
    bind, execute, plan, Bm25Artifact, Bm25Config, Catalog, Error, ExecutionBudget, Parameters,
    QueryRow, Result,
};
use crate::{CursorExpr, Filter, Projection, Query, Source, TemporalSelector, TimeExpr};
use rrd_core::{
    digest, DataTransaction, ProjectionId, ProjectionStamp, ProjectionState, ReadStamp,
    RuntimeMutation, RuntimeRecord, RuntimeValue, ScopeId, DATA_RUNTIME_CONTRACT_VERSION,
};
use rrd_store::{ControlTransition, Durability, Engine};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const INDEX_CATALOGUE_CONTRACT_VERSION: u16 = 1;
pub const INDEX_ARTIFACT_CONTRACT_VERSION: u16 = 1;
const MAX_INDEX_FIELDS: usize = 16;
const MAX_INDEX_ARTIFACT_ROWS: usize = 1_000_000;
const MAX_INDEX_OPERATION_RECEIPTS: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IndexKind {
    #[default]
    Scalar,
    Count,
    Geo,
    MaterializedView,
    AggregateCount,
    Bm25 {
        #[serde(default)]
        config: Bm25Config,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexDefinition {
    pub id: ProjectionId,
    pub source: Source,
    pub fields: Vec<String>,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub kind: IndexKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Filter>,
}

impl IndexDefinition {
    pub fn validate(&self) -> Result<()> {
        if matches!(self.source, Source::Traversal { .. }) {
            return Err(Error::Catalog(
                "recursive traversal cannot be an index source".into(),
            ));
        }
        let allows_empty_fields = matches!(self.kind, IndexKind::Count);
        if (!allows_empty_fields && self.fields.is_empty()) || self.fields.len() > MAX_INDEX_FIELDS
        {
            return Err(Error::Catalog(format!(
                "this index kind requires 1..={MAX_INDEX_FIELDS} fields"
            )));
        }
        if matches!(self.kind, IndexKind::Bm25 { .. }) && self.fields.len() != 1 {
            return Err(Error::Catalog(
                "a BM25 index requires exactly one text field".into(),
            ));
        }
        if let IndexKind::Bm25 { config } = &self.kind {
            if self.unique {
                return Err(Error::Catalog("a BM25 index cannot be unique".into()));
            }
            config.validate()?;
        }
        if self.unique && !matches!(self.kind, IndexKind::Scalar) {
            return Err(Error::Catalog(
                "only scalar and compound scalar indexes can enforce uniqueness".into(),
            ));
        }
        if self.unique && !matches!(self.source, Source::Record { .. }) {
            return Err(Error::Catalog(
                "unique indexes require an authoritative record source".into(),
            ));
        }
        if matches!(self.kind, IndexKind::Geo) && !matches!(self.source, Source::Geo { .. }) {
            return Err(Error::Catalog("geo indexes require a geo source".into()));
        }
        if !self.filters.is_empty()
            && !matches!(
                self.kind,
                IndexKind::Count | IndexKind::MaterializedView | IndexKind::AggregateCount
            )
        {
            return Err(Error::Catalog(
                "only count, grouped-count, and materialized-view indexes accept predicates".into(),
            ));
        }
        let unique = self.fields.iter().collect::<BTreeSet<_>>();
        if unique.len() != self.fields.len() {
            return Err(Error::Catalog("index fields must be unique".into()));
        }
        Ok(())
    }

    pub fn config_digest(&self) -> Result<String> {
        self.validate()?;
        Ok(digest::sha256_hex(&serde_json::to_vec(self)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    pub definition: IndexDefinition,
    pub stamp: ProjectionStamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_valid_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_rows: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_artifact: Option<IndexArtifactReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance: Option<IndexMaintenanceEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytics_total_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytics_group_count: Option<u64>,
    /// A unique definition becomes an integrity constraint only after its
    /// first complete build proves the existing authoritative state. Rebuild,
    /// quarantine, and read-path staleness do not disable that constraint.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unique_validated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexArtifactReference {
    pub generation: u64,
    pub source_cursor: u64,
    pub valid_at: u64,
    pub artifact_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexMaintenanceEvidence {
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_source_cursor: Option<u64>,
    pub source_cursor: u64,
    pub inserted_rows: u64,
    pub updated_rows: u64,
    pub removed_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexCountGroup {
    pub values: Vec<RuntimeValue>,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexAnalyticsArtifact {
    pub total_count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<IndexCountGroup>,
}

impl IndexEntry {
    pub fn is_usable_at(&self, source_cursor: u64, valid_at: u64) -> bool {
        self.stamp.state == ProjectionState::Ready
            && self.stamp.source_cursor == source_cursor
            && self.built_valid_at == Some(valid_at)
            && self.artifact_rows.is_some()
            && self.maintenance.is_some()
            && self
                .definition
                .config_digest()
                .is_ok_and(|digest| self.stamp.config_digest == digest)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexArtifact {
    pub contract_version: u16,
    pub scope: ScopeId,
    pub definition: IndexDefinition,
    pub generation: u64,
    pub source_cursor: u64,
    pub read_cursor: u64,
    pub schema_revision: u64,
    pub valid_at: u64,
    pub rows: Vec<QueryRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25: Option<Bm25Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytics: Option<IndexAnalyticsArtifact>,
    pub maintenance: IndexMaintenanceEvidence,
}

impl IndexArtifact {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != INDEX_ARTIFACT_CONTRACT_VERSION {
            return Err(Error::Integrity(format!(
                "unsupported index artifact contract version {}",
                self.contract_version
            )));
        }
        self.definition.validate()?;
        if self.generation == 0 || self.source_cursor == 0 || self.read_cursor < self.source_cursor
        {
            return Err(Error::Integrity(
                "index artifact generation/source cursor/read cursor are invalid".into(),
            ));
        }
        if self.rows.len() > MAX_INDEX_ARTIFACT_ROWS {
            return Err(Error::Integrity("index artifact row limit exceeded".into()));
        }
        if self
            .rows
            .windows(2)
            .any(|rows| rows[0].identity >= rows[1].identity)
        {
            return Err(Error::Integrity(
                "index artifact row identities must be unique and sorted".into(),
            ));
        }
        if self.maintenance.source_cursor != self.source_cursor
            || (self.maintenance.mode != "full_build"
                && self.maintenance.mode != "incremental_reconciliation")
        {
            return Err(Error::Integrity(
                "index maintenance evidence does not cover this artifact".into(),
            ));
        }
        match (&self.definition.kind, &self.bm25, &self.analytics) {
            (IndexKind::Scalar | IndexKind::Geo | IndexKind::MaterializedView, None, None) => {}
            (IndexKind::Count | IndexKind::AggregateCount, None, Some(analytics)) => {
                validate_analytics(&self.definition, analytics)?;
            }
            (IndexKind::Bm25 { config }, Some(artifact), None) => {
                artifact.validate()?;
                if &artifact.config != config
                    || artifact.source_cursor != self.source_cursor
                    || artifact.schema_revision != self.schema_revision
                    || artifact.valid_at != self.valid_at
                {
                    return Err(Error::Integrity(
                        "BM25 artifact coordinates differ from the containing query index".into(),
                    ));
                }
            }
            _ => {
                return Err(Error::Integrity(
                    "query index kind and specialized artifact disagree".into(),
                ))
            }
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let artifact: Self = serde_json::from_slice(bytes)?;
        artifact.validate()?;
        if artifact.encode()? != bytes {
            return Err(Error::Integrity(
                "index artifact bytes are not canonical".into(),
            ));
        }
        Ok(artifact)
    }

    pub fn digest(&self) -> Result<String> {
        Ok(digest::sha256_hex(&self.encode()?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexCatalogue {
    pub contract_version: u16,
    pub scope: ScopeId,
    pub revision: u64,
    pub entries: BTreeMap<ProjectionId, IndexEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub operations: BTreeMap<String, IndexOperationReceipt>,
}

impl IndexCatalogue {
    pub(crate) fn empty(scope: ScopeId) -> Self {
        Self {
            contract_version: INDEX_CATALOGUE_CONTRACT_VERSION,
            scope,
            revision: 0,
            entries: BTreeMap::new(),
            operations: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.contract_version != INDEX_CATALOGUE_CONTRACT_VERSION {
            return Err(Error::Integrity(format!(
                "unsupported index catalogue contract version {}",
                self.contract_version
            )));
        }
        for (id, entry) in &self.entries {
            validate_index_entry(id, entry)?;
        }
        if self.operations.len() > MAX_INDEX_OPERATION_RECEIPTS {
            return Err(Error::Integrity(
                "index operation receipt limit exceeded".into(),
            ));
        }
        for (key, receipt) in &self.operations {
            if key.is_empty()
                || receipt.operation_digest.len() != 64
                || !receipt
                    .operation_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err(Error::Integrity(
                    "index operation receipt identity is invalid".into(),
                ));
            }
            validate_index_entry(&receipt.entry.definition.id, &receipt.entry)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexOperationReceipt {
    pub operation_digest: String,
    pub entry: IndexEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexMutationContext {
    pub at: u64,
    pub actor: String,
    pub request_id: String,
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexArtifactPublication {
    pub generation: u64,
    pub source_cursor: u64,
    pub valid_at: u64,
    pub artifact_rows: u64,
    pub artifact_digest: String,
    pub maintenance: IndexMaintenanceEvidence,
    pub analytics_total_count: Option<u64>,
    pub analytics_group_count: Option<u64>,
}

impl IndexMutationContext {
    fn validate(&self) -> Result<()> {
        if self.at == 0
            || self.actor.is_empty()
            || self.request_id.is_empty()
            || self.operation_id.is_empty()
        {
            return Err(Error::Catalog(
                "index mutation context requires time, actor, request, and operation IDs".into(),
            ));
        }
        Ok(())
    }
}

pub struct IndexCatalogueRepository<'a, E: Engine + ?Sized> {
    engine: &'a E,
    scope: ScopeId,
    key: String,
}

impl<'a, E: Engine> IndexCatalogueRepository<'a, E> {
    pub fn new(engine: &'a E, scope: ScopeId) -> Self {
        let key = format!("server/state/index-catalogue/{scope}");
        Self { engine, scope, key }
    }

    pub fn load(&self) -> Result<IndexCatalogue> {
        let Some(bytes) = self.engine.control_record(&self.key)? else {
            return Ok(IndexCatalogue::empty(self.scope.clone()));
        };
        let catalogue: IndexCatalogue = serde_json::from_slice(&bytes)?;
        if catalogue.scope != self.scope {
            return Err(Error::Integrity(
                "index catalogue scope differs from its control key".into(),
            ));
        }
        catalogue.validate()?;
        Ok(catalogue)
    }

    pub fn operation_receipt(
        &self,
        idempotency_key: &str,
        operation_digest: &str,
    ) -> Result<Option<IndexOperationReceipt>> {
        let catalogue = self.load()?;
        let Some(receipt) = catalogue.operations.get(idempotency_key) else {
            return Ok(None);
        };
        if receipt.operation_digest != operation_digest {
            return Err(Error::Catalog(
                "index idempotency key is bound to a different operation".into(),
            ));
        }
        Ok(Some(receipt.clone()))
    }

    pub fn record_operation(
        &self,
        context: &IndexMutationContext,
        idempotency_key: String,
        operation_digest: String,
        entry: IndexEntry,
    ) -> Result<IndexCatalogue> {
        context.validate()?;
        if idempotency_key.is_empty()
            || operation_digest.len() != 64
            || !operation_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(Error::Catalog(
                "index operation requires a canonical idempotency key and SHA-256".into(),
            ));
        }
        validate_index_entry(&entry.definition.id, &entry)?;
        self.update(context, "index.operation_recorded", move |catalogue| {
            if let Some(existing) = catalogue.operations.get(&idempotency_key) {
                if existing.operation_digest == operation_digest && existing.entry == entry {
                    return Ok(());
                }
                return Err(Error::Catalog(
                    "index idempotency key is bound to a different operation".into(),
                ));
            }
            if catalogue.operations.len() >= MAX_INDEX_OPERATION_RECEIPTS {
                return Err(Error::Budget(
                    "index operation receipt limit exceeded".into(),
                ));
            }
            catalogue.operations.insert(
                idempotency_key,
                IndexOperationReceipt {
                    operation_digest,
                    entry,
                },
            );
            Ok(())
        })
    }

    pub fn create(
        &self,
        context: &IndexMutationContext,
        query_catalogue: &Catalog,
        definition: IndexDefinition,
    ) -> Result<IndexCatalogue> {
        context.validate()?;
        definition.validate()?;
        if query_catalogue.read.scope != self.scope {
            return Err(Error::Catalog(
                "query and index catalogues belong to different scopes".into(),
            ));
        }
        validate_fields(query_catalogue, &definition)?;
        self.update(context, "index.created", move |catalogue| {
            if catalogue.entries.contains_key(&definition.id) {
                return Err(Error::Catalog(format!(
                    "index {} already exists",
                    definition.id
                )));
            }
            let config_digest = definition.config_digest()?;
            let id = definition.id.clone();
            catalogue.entries.insert(
                id.clone(),
                IndexEntry {
                    definition,
                    stamp: ProjectionStamp {
                        contract_version: DATA_RUNTIME_CONTRACT_VERSION,
                        id,
                        generation: 1,
                        source_cursor: 0,
                        config_digest,
                        artifact_digest: digest::sha256_hex(&[]),
                        state: ProjectionState::Building,
                    },
                    built_valid_at: None,
                    artifact_rows: None,
                    prior_artifact: None,
                    maintenance: None,
                    analytics_total_count: None,
                    analytics_group_count: None,
                    unique_validated: false,
                },
            );
            Ok(())
        })
    }

    pub fn begin_rebuild(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
    ) -> Result<IndexCatalogue> {
        context.validate()?;
        let id = id.clone();
        self.update(context, "index.rebuild_started", move |catalogue| {
            let entry = catalogue
                .entries
                .get_mut(&id)
                .ok_or_else(|| Error::Catalog(format!("unknown index {id}")))?;
            entry.stamp.generation = entry
                .stamp
                .generation
                .checked_add(1)
                .ok_or_else(|| Error::Integrity("index generation overflow".into()))?;
            entry.prior_artifact = (entry.stamp.state == ProjectionState::Ready
                && entry.maintenance.is_some())
            .then(|| IndexArtifactReference {
                generation: entry.stamp.generation - 1,
                source_cursor: entry.stamp.source_cursor,
                valid_at: entry
                    .built_valid_at
                    .expect("ready index has valid-time coverage"),
                artifact_digest: entry.stamp.artifact_digest.clone(),
            });
            entry.stamp.source_cursor = 0;
            entry.stamp.artifact_digest = digest::sha256_hex(&[]);
            entry.stamp.state = ProjectionState::Building;
            entry.built_valid_at = None;
            entry.artifact_rows = None;
            entry.maintenance = None;
            entry.analytics_total_count = None;
            entry.analytics_group_count = None;
            Ok(())
        })
    }

    pub fn build(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
        valid_at: u64,
        budget: &ExecutionBudget,
    ) -> Result<IndexCatalogue> {
        context.validate()?;
        let catalogue = self.load()?;
        let entry = catalogue
            .entries
            .get(id)
            .ok_or_else(|| Error::Catalog(format!("unknown index {id}")))?;
        if entry.stamp.state != ProjectionState::Building {
            return Err(Error::Catalog(format!("index {id} is not building")));
        }
        let generation = entry.stamp.generation;
        let definition = entry.definition.clone();
        let query_catalogue = Catalog::capture(self.engine, &self.scope)?;
        let mut query = Query::new(
            definition.source.clone(),
            TemporalSelector {
                valid_at: TimeExpr::Literal(valid_at),
                known_at: CursorExpr::Head,
            },
        );
        query.filters = definition.filters.clone();
        query.projection = Projection::All;
        let bound = bind(&query, &Parameters::new(), &query_catalogue)?;
        let physical = plan(&bound)?;
        let execution = execute(self.engine, &physical, budget)?;
        if execution.truncated {
            return Err(Error::Budget(
                "index build execution was truncated by its budget".into(),
            ));
        }
        let rows = execution
            .batches
            .into_iter()
            .flat_map(|batch| batch.rows)
            .collect::<Vec<_>>();
        validate_unique_rows(&definition, &rows)?;
        validate_unique_definition(self.engine, &query_catalogue.read, &definition)?;
        let bm25 = match &definition.kind {
            IndexKind::Scalar
            | IndexKind::Count
            | IndexKind::Geo
            | IndexKind::MaterializedView
            | IndexKind::AggregateCount => None,
            IndexKind::Bm25 { config } => {
                let field = &definition.fields[0];
                let documents = rows
                    .iter()
                    .filter_map(|row| match row.values.get(field) {
                        Some(rrd_core::RuntimeValue::String(text)) => {
                            Some(Ok((row.identity.clone(), text.clone())))
                        }
                        Some(rrd_core::RuntimeValue::Null) | None => None,
                        Some(_) => Some(Err(Error::Catalog(format!(
                            "BM25 field {field:?} produced a non-string value"
                        )))),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Some(Bm25Artifact::build(
                    config.clone(),
                    bound.source_cursor,
                    bound.schema_revision,
                    valid_at,
                    documents,
                )?)
            }
        };
        let analytics = match definition.kind {
            IndexKind::Count | IndexKind::AggregateCount => {
                Some(build_analytics(&definition, &rows)?)
            }
            _ => None,
        };
        let prior = entry.prior_artifact.clone();
        let maintenance = maintenance_evidence(
            self.engine,
            &self.scope,
            id,
            prior.as_ref(),
            bound.source_cursor,
            &rows,
        )?;
        let artifact = IndexArtifact {
            contract_version: INDEX_ARTIFACT_CONTRACT_VERSION,
            scope: self.scope.clone(),
            definition,
            generation,
            source_cursor: bound.source_cursor,
            read_cursor: query_catalogue.read.commit_cursor,
            schema_revision: bound.schema_revision,
            valid_at,
            rows,
            bm25,
            analytics,
            maintenance,
        };
        let bytes = artifact.encode()?;
        let artifact_digest = digest::sha256_hex(&bytes);
        let name = index_artifact_name(&self.scope, id, generation, &artifact_digest);
        if let Some(existing) = self.engine.get_projection(&name)? {
            if existing != bytes {
                return Err(Error::Integrity(
                    "content-addressed index artifact name contains different bytes".into(),
                ));
            }
        } else {
            self.engine
                .put_projection_with(&name, &bytes, Durability::Authoritative)?;
        }
        let stored = self
            .engine
            .get_projection(&name)?
            .ok_or_else(|| Error::Integrity("published index artifact is unreadable".into()))?;
        if digest::sha256_hex(&stored) != artifact_digest {
            return Err(Error::Integrity(
                "published index artifact digest does not verify".into(),
            ));
        }
        let stored_artifact = IndexArtifact::decode(&stored)?;
        if stored_artifact != artifact {
            return Err(Error::Integrity(
                "published index artifact does not decode to the built artifact".into(),
            ));
        }
        self.publish_ready(
            context,
            id,
            &IndexArtifactPublication {
                generation,
                source_cursor: artifact.source_cursor,
                valid_at,
                artifact_rows: u64::try_from(artifact.rows.len())
                    .map_err(|_| Error::Budget("index artifact row count exceeds u64".into()))?,
                artifact_digest,
                maintenance: artifact.maintenance.clone(),
                analytics_total_count: artifact.analytics.as_ref().map(|value| value.total_count),
                analytics_group_count: artifact
                    .analytics
                    .as_ref()
                    .map(|value| value.groups.len())
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|_| Error::Budget("analytics group count exceeds u64".into()))?,
            },
        )
    }

    pub fn publish_ready(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
        publication: &IndexArtifactPublication,
    ) -> Result<IndexCatalogue> {
        context.validate()?;
        let scope_head = self.engine.runtime_read_stamp(&self.scope)?.commit_cursor;
        if publication.source_cursor == 0 || publication.source_cursor > scope_head {
            return Err(Error::Catalog(
                "index source cursor must name an existing authoritative change".into(),
            ));
        }
        let id = id.clone();
        let publication = publication.clone();
        self.update(context, "index.ready", move |catalogue| {
            let entry = catalogue
                .entries
                .get_mut(&id)
                .ok_or_else(|| Error::Catalog(format!("unknown index {id}")))?;
            if entry.stamp.generation != publication.generation
                || entry.stamp.state != ProjectionState::Building
            {
                return Err(Error::Catalog(
                    "index publication is stale or not building".into(),
                ));
            }
            entry.stamp.source_cursor = publication.source_cursor;
            entry.stamp.artifact_digest = publication.artifact_digest;
            entry.stamp.state = ProjectionState::Ready;
            entry.built_valid_at = Some(publication.valid_at);
            entry.artifact_rows = Some(publication.artifact_rows);
            entry.prior_artifact = None;
            entry.maintenance = Some(publication.maintenance);
            entry.analytics_total_count = publication.analytics_total_count;
            entry.analytics_group_count = publication.analytics_group_count;
            if entry.definition.unique {
                entry.unique_validated = true;
            }
            entry
                .stamp
                .validate()
                .map_err(|error| Error::Catalog(format!("invalid ready index stamp: {error}")))?;
            Ok(())
        })
    }

    pub fn quarantine(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
    ) -> Result<IndexCatalogue> {
        self.set_state(
            context,
            id,
            ProjectionState::Quarantined,
            "index.quarantined",
        )
    }

    pub fn retire(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
    ) -> Result<IndexCatalogue> {
        self.set_state(context, id, ProjectionState::Retiring, "index.retiring")
    }

    fn set_state(
        &self,
        context: &IndexMutationContext,
        id: &ProjectionId,
        state: ProjectionState,
        action: &str,
    ) -> Result<IndexCatalogue> {
        context.validate()?;
        let id = id.clone();
        self.update(context, action, move |catalogue| {
            let entry = catalogue
                .entries
                .get_mut(&id)
                .ok_or_else(|| Error::Catalog(format!("unknown index {id}")))?;
            entry.stamp.state = state;
            Ok(())
        })
    }

    fn update(
        &self,
        context: &IndexMutationContext,
        action: &str,
        mutate: impl FnOnce(&mut IndexCatalogue) -> Result<()>,
    ) -> Result<IndexCatalogue> {
        let expected = self.engine.control_record(&self.key)?;
        let mut catalogue = match &expected {
            Some(bytes) => serde_json::from_slice(bytes)?,
            None => IndexCatalogue::empty(self.scope.clone()),
        };
        catalogue.validate()?;
        mutate(&mut catalogue)?;
        catalogue.revision = catalogue
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::Integrity("index catalogue revision overflow".into()))?;
        catalogue.validate()?;
        let replacement = serde_json::to_vec(&catalogue)?;
        self.engine.commit_catalog_transition(
            &self.scope,
            &ControlTransition {
                key: self.key.clone(),
                expected,
                replacement: Some(replacement),
                at: context.at,
                actor: context.actor.clone(),
                action: action.into(),
                request_id: context.request_id.clone(),
                operation_id: context.operation_id.clone(),
            },
        )?;
        Ok(catalogue)
    }
}

fn validate_index_entry(id: &ProjectionId, entry: &IndexEntry) -> Result<()> {
    entry.definition.validate()?;
    entry
        .stamp
        .validate()
        .map_err(|error| Error::Integrity(format!("invalid index stamp for {id}: {error}")))?;
    if id != &entry.definition.id
        || id != &entry.stamp.id
        || entry.stamp.config_digest != entry.definition.config_digest()?
    {
        return Err(Error::Integrity(format!(
            "index identity or configuration digest disagrees for {id}"
        )));
    }
    if entry.stamp.state == ProjectionState::Ready
        && (entry.built_valid_at.is_none() || entry.artifact_rows.is_none())
    {
        return Err(Error::Integrity(format!(
            "ready index {id} is missing artifact coverage"
        )));
    }
    let analytics_kind = matches!(
        entry.definition.kind,
        IndexKind::Count | IndexKind::AggregateCount
    );
    if entry.stamp.state == ProjectionState::Ready
        && entry.maintenance.is_some()
        && (analytics_kind
            != (entry.analytics_total_count.is_some() && entry.analytics_group_count.is_some()))
    {
        return Err(Error::Integrity(format!(
            "ready index {id} has inconsistent analytics summaries"
        )));
    }
    if entry.stamp.state != ProjectionState::Building && entry.prior_artifact.is_some() {
        return Err(Error::Integrity(format!(
            "non-building index {id} retained a prior artifact pointer"
        )));
    }
    if entry.unique_validated && !entry.definition.unique {
        return Err(Error::Integrity(format!(
            "non-unique index {id} claims unique-constraint validation"
        )));
    }
    Ok(())
}

pub(crate) fn index_artifact_name(
    scope: &ScopeId,
    id: &ProjectionId,
    generation: u64,
    artifact_digest: &str,
) -> String {
    format!("query-index/{scope}/{id}/{generation}/{artifact_digest}")
}

fn validate_fields(catalogue: &Catalog, definition: &IndexDefinition) -> Result<()> {
    let mut query = Query::new(
        definition.source.clone(),
        TemporalSelector {
            valid_at: TimeExpr::Literal(0),
            known_at: CursorExpr::Head,
        },
    );
    query.filters = definition.filters.clone();
    query.projection = if definition.fields.is_empty() {
        Projection::All
    } else {
        Projection::Fields(definition.fields.clone())
    };
    let bound = bind(&query, &Parameters::new(), catalogue)?;
    if let IndexKind::Bm25 { .. } = definition.kind {
        let field = &definition.fields[0];
        let accepted = super::plan::field_types_for_bound_source(&bound.source, catalogue, field)?;
        if !accepted.contains(&rrd_core::RuntimeValueType::String) {
            return Err(Error::Catalog(format!(
                "BM25 field {field:?} is not a string field"
            )));
        }
    }
    Ok(())
}

fn validate_unique_rows(definition: &IndexDefinition, rows: &[QueryRow]) -> Result<()> {
    if !definition.unique {
        return Ok(());
    }
    let mut keys = BTreeMap::<Vec<u8>, &str>::new();
    for row in rows {
        let values = definition
            .fields
            .iter()
            .map(|field| row.values.get(field).cloned().unwrap_or(RuntimeValue::Null))
            .collect::<Vec<_>>();
        if values.contains(&RuntimeValue::Null) {
            continue;
        }
        let key = serde_json::to_vec(&values)?;
        if let Some(existing) = keys.insert(key, &row.identity) {
            return Err(Error::Catalog(format!(
                "unique index {} rejects duplicate rows {existing:?} and {:?}",
                definition.id, row.identity
            )));
        }
    }
    Ok(())
}

/// Checks every installed engine-owned unique constraint against the exact
/// prospective transaction state. Callers serialize this check with the
/// authoritative commit; rebuilding, quarantined, retiring, or stale data
/// indexes remain constraints even though planners reject them as access paths.
pub fn validate_unique_indexes<E: Engine>(
    engine: &E,
    transaction: &DataTransaction,
    _default_valid_at: u64,
) -> Result<()> {
    let catalogue = IndexCatalogueRepository::new(engine, transaction.read.scope.clone()).load()?;
    let unique = catalogue
        .entries
        .values()
        .filter(|entry| entry.definition.unique && entry.unique_validated)
        .collect::<Vec<_>>();
    if unique.is_empty() {
        return Ok(());
    }
    let mut records = current_records_at_read(engine, &transaction.read)?;
    for mutation in &transaction.commit.mutations {
        match mutation {
            RuntimeMutation::Record { record } => {
                records.insert(record.reference.clone(), record.clone());
            }
            RuntimeMutation::Retire { retirement } if retirement.model.is_record_like() => {
                records.remove(&retirement.reference);
            }
            _ => {}
        }
    }
    for entry in unique {
        let Source::Record { kind } = &entry.definition.source else {
            return Err(Error::Integrity(format!(
                "unique index {} does not name a record source",
                entry.definition.id
            )));
        };
        validate_unique_records(
            &entry.definition,
            records
                .values()
                .filter(|record| &record.reference.kind == kind)
                .collect(),
        )?;
    }
    Ok(())
}

fn validate_unique_definition<E: Engine>(
    engine: &E,
    read: &ReadStamp,
    definition: &IndexDefinition,
) -> Result<()> {
    if !definition.unique {
        return Ok(());
    }
    let Source::Record { kind } = &definition.source else {
        return Err(Error::Integrity(format!(
            "unique index {} does not name a record source",
            definition.id
        )));
    };
    validate_unique_records(
        definition,
        current_records_at_read(engine, read)?
            .values()
            .filter(|record| &record.reference.kind == kind)
            .collect(),
    )
}

fn current_records_at_read<E: Engine>(
    engine: &E,
    read: &ReadStamp,
) -> Result<BTreeMap<rrd_core::RuntimeRef, RuntimeRecord>> {
    if read.commit_cursor == 0 {
        return Ok(BTreeMap::new());
    }
    let limit = usize::try_from(read.commit_cursor)
        .map_err(|_| Error::Budget("unique-index replay cursor exceeds usize".into()))?;
    let page = engine.runtime_read_changes(read, 0, limit)?;
    if page.through_cursor != read.commit_cursor {
        return Err(Error::Integrity(
            "unique-index validation did not reach the transaction read cursor".into(),
        ));
    }
    let mut records = BTreeMap::new();
    for change in page.changes {
        match change.mutation {
            RuntimeMutation::Record { record } => {
                records.insert(record.reference.clone(), record);
            }
            RuntimeMutation::Retire { retirement } if retirement.model.is_record_like() => {
                records.remove(&retirement.reference);
            }
            _ => {}
        }
    }
    Ok(records)
}

fn validate_unique_records(
    definition: &IndexDefinition,
    records: Vec<&RuntimeRecord>,
) -> Result<()> {
    let mut groups = BTreeMap::<Vec<u8>, Vec<&RuntimeRecord>>::new();
    for record in records {
        let values = definition
            .fields
            .iter()
            .map(|field| match field.as_str() {
                "id" => RuntimeValue::String(record.reference.id.to_string()),
                "kind" => RuntimeValue::String(record.reference.kind.to_string()),
                "valid_from" => RuntimeValue::Unsigned(record.valid_from),
                "valid_to" => record
                    .valid_to
                    .map_or(RuntimeValue::Null, RuntimeValue::Unsigned),
                _ => record
                    .properties
                    .get(field)
                    .cloned()
                    .unwrap_or(RuntimeValue::Null),
            })
            .collect::<Vec<_>>();
        if values.contains(&RuntimeValue::Null) {
            continue;
        }
        groups
            .entry(serde_json::to_vec(&values)?)
            .or_default()
            .push(record);
    }
    for group in groups.values() {
        for (index, left) in group.iter().enumerate() {
            if group[index + 1..].iter().any(|right| {
                windows_overlap(
                    left.valid_from,
                    left.valid_to,
                    right.valid_from,
                    right.valid_to,
                )
            }) {
                return Err(Error::Catalog(format!(
                    "unique index {} rejects overlapping duplicate records",
                    definition.id
                )));
            }
        }
    }
    Ok(())
}

fn windows_overlap(
    left_from: u64,
    left_to: Option<u64>,
    right_from: u64,
    right_to: Option<u64>,
) -> bool {
    left_from < right_to.unwrap_or(u64::MAX) && right_from < left_to.unwrap_or(u64::MAX)
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn build_analytics(
    definition: &IndexDefinition,
    rows: &[QueryRow],
) -> Result<IndexAnalyticsArtifact> {
    let total_count =
        u64::try_from(rows.len()).map_err(|_| Error::Budget("index count exceeds u64".into()))?;
    let groups = if matches!(definition.kind, IndexKind::AggregateCount) {
        let mut grouped = BTreeMap::<Vec<u8>, (Vec<RuntimeValue>, u64)>::new();
        for row in rows {
            let values = definition
                .fields
                .iter()
                .map(|field| row.values.get(field).cloned().unwrap_or(RuntimeValue::Null))
                .collect::<Vec<_>>();
            let key = serde_json::to_vec(&values)?;
            let group = grouped.entry(key).or_insert((values, 0));
            group.1 = group
                .1
                .checked_add(1)
                .ok_or_else(|| Error::Budget("grouped count overflow".into()))?;
        }
        grouped
            .into_values()
            .map(|(values, count)| IndexCountGroup { values, count })
            .collect()
    } else {
        Vec::new()
    };
    Ok(IndexAnalyticsArtifact {
        total_count,
        groups,
    })
}

fn validate_analytics(
    definition: &IndexDefinition,
    analytics: &IndexAnalyticsArtifact,
) -> Result<()> {
    if matches!(definition.kind, IndexKind::Count) && !analytics.groups.is_empty() {
        return Err(Error::Integrity(
            "a count index cannot contain groups".into(),
        ));
    }
    if matches!(definition.kind, IndexKind::AggregateCount) {
        let grouped_total = analytics.groups.iter().try_fold(0_u64, |sum, group| {
            if group.values.len() != definition.fields.len() || group.count == 0 {
                return Err(Error::Integrity("grouped-count entry is invalid".into()));
            }
            sum.checked_add(group.count)
                .ok_or_else(|| Error::Integrity("grouped-count total overflow".into()))
        })?;
        if grouped_total != analytics.total_count {
            return Err(Error::Integrity(
                "grouped-count total does not match its groups".into(),
            ));
        }
    }
    Ok(())
}

fn maintenance_evidence<E: Engine>(
    engine: &E,
    scope: &ScopeId,
    id: &ProjectionId,
    prior: Option<&IndexArtifactReference>,
    source_cursor: u64,
    rows: &[QueryRow],
) -> Result<IndexMaintenanceEvidence> {
    let Some(prior) = prior else {
        return Ok(IndexMaintenanceEvidence {
            mode: "full_build".into(),
            prior_source_cursor: None,
            source_cursor,
            inserted_rows: u64::try_from(rows.len())
                .map_err(|_| Error::Budget("index row count exceeds u64".into()))?,
            updated_rows: 0,
            removed_rows: 0,
        });
    };
    let name = index_artifact_name(scope, id, prior.generation, &prior.artifact_digest);
    let bytes = engine
        .get_projection(&name)?
        .ok_or_else(|| Error::Integrity("prior index artifact is missing".into()))?;
    let artifact = IndexArtifact::decode(&bytes)?;
    if artifact.source_cursor != prior.source_cursor || artifact.valid_at != prior.valid_at {
        return Err(Error::Integrity(
            "prior index artifact coordinates disagree with the catalogue".into(),
        ));
    }
    let old = artifact
        .rows
        .iter()
        .map(|row| (row.identity.as_str(), row))
        .collect::<BTreeMap<_, _>>();
    let new = rows
        .iter()
        .map(|row| (row.identity.as_str(), row))
        .collect::<BTreeMap<_, _>>();
    let inserted_rows = new
        .keys()
        .filter(|identity| !old.contains_key(*identity))
        .count();
    let removed_rows = old
        .keys()
        .filter(|identity| !new.contains_key(*identity))
        .count();
    let updated_rows = new
        .iter()
        .filter(|(identity, row)| old.get(*identity).is_some_and(|old| *old != **row))
        .count();
    Ok(IndexMaintenanceEvidence {
        mode: "incremental_reconciliation".into(),
        prior_source_cursor: Some(prior.source_cursor),
        source_cursor,
        inserted_rows: u64::try_from(inserted_rows)
            .map_err(|_| Error::Budget("inserted row count exceeds u64".into()))?,
        updated_rows: u64::try_from(updated_rows)
            .map_err(|_| Error::Budget("updated row count exceeds u64".into()))?,
        removed_rows: u64::try_from(removed_rows)
            .map_err(|_| Error::Budget("removed row count exceeds u64".into()))?,
    })
}
