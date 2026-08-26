use crate::{
    bind, execute, plan, Bm25Artifact, Bm25Config, Catalog, Error, ExecutionBudget, Parameters,
    QueryRow, Result,
};
use crate::{CursorExpr, Projection, Query, Source, TemporalSelector, TimeExpr};
use rrd_core::{
    digest, ProjectionId, ProjectionStamp, ProjectionState, ScopeId, DATA_RUNTIME_CONTRACT_VERSION,
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
}

impl IndexDefinition {
    pub fn validate(&self) -> Result<()> {
        if matches!(self.source, Source::Traversal { .. }) {
            return Err(Error::Catalog(
                "recursive traversal cannot be an index source".into(),
            ));
        }
        if self.fields.is_empty() || self.fields.len() > MAX_INDEX_FIELDS {
            return Err(Error::Catalog(format!(
                "an index must contain 1..={MAX_INDEX_FIELDS} fields"
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
            if config != &Bm25Config::default() {
                return Err(Error::Catalog(
                    "BM25 v1 query semantics require the default k1/b configuration".into(),
                ));
            }
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
}

impl IndexEntry {
    pub fn is_usable_at(&self, source_cursor: u64, valid_at: u64) -> bool {
        self.stamp.state == ProjectionState::Ready
            && self.stamp.source_cursor == source_cursor
            && self.built_valid_at == Some(valid_at)
            && self.artifact_rows.is_some()
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
        match (&self.definition.kind, &self.bm25) {
            (IndexKind::Scalar, None) => {}
            (IndexKind::Bm25 { config }, Some(artifact)) => {
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
                    "query index kind and BM25 artifact disagree".into(),
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
            entry.stamp.source_cursor = 0;
            entry.stamp.artifact_digest = digest::sha256_hex(&[]);
            entry.stamp.state = ProjectionState::Building;
            entry.built_valid_at = None;
            entry.artifact_rows = None;
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
        let bm25 = match &definition.kind {
            IndexKind::Scalar => None,
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
    query.projection = Projection::Fields(definition.fields.clone());
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
