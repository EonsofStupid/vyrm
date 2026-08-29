use crate::{EmbeddingModelBinding, ScoreMetric};
use rrd_core::{digest, ProjectionId, ScopeId};
use rrd_store::{ControlTransition, Engine};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const VECTOR_COLLECTION_CATALOGUE_VERSION: u16 = 1;
const MAX_COLLECTIONS: usize = 4_096;
const MAX_NAMED_VECTORS: usize = 64;
const MAX_PAYLOAD_INDEXES: usize = 256;
const MAX_OPERATION_RECEIPTS: usize = 100_000;

pub type CollectionResult<T> = std::result::Result<T, CollectionError>;

#[derive(Debug)]
pub enum CollectionError {
    Store(rrd_store::Error),
    Invalid(String),
    Integrity(String),
    IdempotencyConflict,
    Budget(String),
}

impl fmt::Display for CollectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "vector collection storage failed: {error}"),
            Self::Invalid(message) => write!(formatter, "invalid vector collection: {message}"),
            Self::Integrity(message) => {
                write!(formatter, "vector collection integrity failed: {message}")
            }
            Self::IdempotencyConflict => formatter
                .write_str("vector collection idempotency key is bound to another operation"),
            Self::Budget(message) => {
                write!(formatter, "vector collection budget exceeded: {message}")
            }
        }
    }
}

impl std::error::Error for CollectionError {}

impl From<rrd_store::Error> for CollectionError {
    fn from(value: rrd_store::Error) -> Self {
        Self::Store(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorValueKind {
    Dense,
    Sparse,
    MultiDense,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorMemoryTier {
    Pinned,
    Cached,
    Cold,
}

/// Exact scalar representation admitted by one collection payload index.
///
/// This deliberately follows RRD's canonical property vocabulary instead of
/// accepting transport-specific aliases that the authoritative runtime cannot
/// validate after reopen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadIndexKind {
    Boolean,
    Integer,
    Unsigned,
    Decimal,
    Keyword,
    Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadIndexDefinition {
    pub field: ProjectionId,
    pub kind: PayloadIndexKind,
}

impl PayloadIndexDefinition {
    pub fn config_digest(&self) -> CollectionResult<String> {
        let encoded = serde_json::to_vec(self)
            .map_err(|error| CollectionError::Invalid(error.to_string()))?;
        Ok(digest::sha256_hex(&encoded))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadIndexEntry {
    pub definition: PayloadIndexDefinition,
    pub generation: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub configuration_digest: String,
}

impl PayloadIndexEntry {
    pub fn validate(&self) -> CollectionResult<()> {
        if self.generation == 0 || self.created_at == 0 || self.updated_at < self.created_at {
            return integrity("payload-index generation or timestamps are invalid");
        }
        if self.configuration_digest != self.definition.config_digest()? {
            return integrity("payload-index configuration digest differs from its definition");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedVectorConfig {
    pub name: ProjectionId,
    pub field: String,
    pub kind: VectorValueKind,
    pub dimensions: u32,
    pub metric: ScoreMetric,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<EmbeddingModelBinding>,
    pub memory_tier: VectorMemoryTier,
}

impl NamedVectorConfig {
    pub fn validate(&self) -> CollectionResult<()> {
        if self.field.trim().is_empty() || self.field.as_bytes().contains(&0) {
            return invalid("named-vector field must be non-empty and contain no NUL");
        }
        if self.dimensions == 0 || self.dimensions > 1_048_576 {
            return invalid("named-vector dimensions must be in 1..=1048576");
        }
        if let Some(model) = &self.embedding_model {
            model
                .validate()
                .map_err(|error| CollectionError::Invalid(error.to_string()))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorCollectionDefinition {
    pub id: ProjectionId,
    pub vectors: BTreeMap<ProjectionId, NamedVectorConfig>,
}

impl VectorCollectionDefinition {
    pub fn validate(&self) -> CollectionResult<()> {
        if self.vectors.is_empty() || self.vectors.len() > MAX_NAMED_VECTORS {
            return invalid(format!(
                "collection must contain 1..={MAX_NAMED_VECTORS} named vectors"
            ));
        }
        let mut fields = BTreeSet::new();
        for (name, vector) in &self.vectors {
            vector.validate()?;
            if name != &vector.name {
                return invalid("named-vector map identity differs from its definition");
            }
            if !fields.insert(vector.field.as_str()) {
                return invalid("collection vector fields must be unique");
            }
        }
        Ok(())
    }

    pub fn config_digest(&self) -> CollectionResult<String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| CollectionError::Invalid(error.to_string()))?;
        Ok(digest::sha256_hex(&encoded))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionEntry {
    pub definition: VectorCollectionDefinition,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub payload_indexes: BTreeMap<ProjectionId, PayloadIndexEntry>,
    pub generation: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub configuration_digest: String,
}

impl CollectionEntry {
    pub fn validate(&self) -> CollectionResult<()> {
        self.definition.validate()?;
        if self.generation == 0 || self.created_at == 0 || self.updated_at < self.created_at {
            return integrity("collection generation or timestamps are invalid");
        }
        if self.configuration_digest != self.definition.config_digest()? {
            return integrity("collection configuration digest differs from its definition");
        }
        if self.payload_indexes.len() > MAX_PAYLOAD_INDEXES {
            return integrity("collection payload-index bound is exceeded");
        }
        for (field, index) in &self.payload_indexes {
            index.validate()?;
            if field != &index.definition.field {
                return integrity("payload-index map identity differs from its definition");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionOperationReceipt {
    pub operation_digest: String,
    pub entry: CollectionEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionDeletionReceipt {
    pub operation_digest: String,
    pub deleted_entry: CollectionEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadIndexOperationReceipt {
    pub operation_digest: String,
    pub collection: CollectionEntry,
    pub index: PayloadIndexEntry,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionCatalogue {
    pub contract_version: u16,
    pub scope: ScopeId,
    pub revision: u64,
    pub collections: BTreeMap<ProjectionId, CollectionEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub operations: BTreeMap<String, CollectionOperationReceipt>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub deletions: BTreeMap<String, CollectionDeletionReceipt>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub payload_operations: BTreeMap<String, PayloadIndexOperationReceipt>,
}

impl CollectionCatalogue {
    fn empty(scope: ScopeId) -> Self {
        Self {
            contract_version: VECTOR_COLLECTION_CATALOGUE_VERSION,
            scope,
            revision: 0,
            collections: BTreeMap::new(),
            operations: BTreeMap::new(),
            deletions: BTreeMap::new(),
            payload_operations: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> CollectionResult<()> {
        if self.contract_version != VECTOR_COLLECTION_CATALOGUE_VERSION {
            return integrity("unsupported vector collection catalogue version");
        }
        let operation_count = self
            .operations
            .len()
            .checked_add(self.deletions.len())
            .and_then(|count| count.checked_add(self.payload_operations.len()))
            .ok_or_else(|| CollectionError::Integrity("operation receipt count overflow".into()))?;
        if self.collections.len() > MAX_COLLECTIONS || operation_count > MAX_OPERATION_RECEIPTS {
            return integrity("vector collection catalogue bounds are exceeded");
        }
        for (id, entry) in &self.collections {
            entry.validate()?;
            if id != &entry.definition.id {
                return integrity("collection map identity differs from its definition");
            }
        }
        for (key, receipt) in &self.operations {
            if key.is_empty() || !valid_digest(&receipt.operation_digest) {
                return integrity("vector collection operation receipt is invalid");
            }
            receipt.entry.validate()?;
        }
        for (key, receipt) in &self.deletions {
            if key.is_empty() || !valid_digest(&receipt.operation_digest) {
                return integrity("vector collection deletion receipt is invalid");
            }
            receipt.deleted_entry.validate()?;
        }
        for (key, receipt) in &self.payload_operations {
            if key.is_empty() || !valid_digest(&receipt.operation_digest) {
                return integrity("payload-index operation receipt is invalid");
            }
            receipt.collection.validate()?;
            receipt.index.validate()?;
            let field = &receipt.index.definition.field;
            if receipt.deleted == receipt.collection.payload_indexes.contains_key(field) {
                return integrity("payload-index receipt outcome differs from its collection");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionMutationContext {
    pub at: u64,
    pub actor: String,
    pub request_id: String,
    pub operation_id: String,
}

impl CollectionMutationContext {
    fn validate(&self) -> CollectionResult<()> {
        if self.at == 0
            || self.actor.is_empty()
            || self.request_id.is_empty()
            || self.operation_id.is_empty()
        {
            return invalid("collection mutation requires time, actor, request, and operation IDs");
        }
        Ok(())
    }
}

pub struct VectorCollectionRepository<'a, E: Engine + ?Sized> {
    engine: &'a E,
    scope: ScopeId,
    key: String,
}

impl<'a, E: Engine> VectorCollectionRepository<'a, E> {
    pub fn new(engine: &'a E, scope: ScopeId) -> Self {
        let key = format!("server/state/vector-collection-catalogue/{scope}");
        Self { engine, scope, key }
    }

    pub fn load(&self) -> CollectionResult<CollectionCatalogue> {
        let Some(bytes) = self.engine.control_record(&self.key)? else {
            return Ok(CollectionCatalogue::empty(self.scope.clone()));
        };
        let catalogue: CollectionCatalogue = serde_json::from_slice(&bytes)
            .map_err(|error| CollectionError::Integrity(error.to_string()))?;
        if catalogue.scope != self.scope {
            return integrity("collection catalogue scope differs from its control key");
        }
        catalogue.validate()?;
        Ok(catalogue)
    }

    pub fn operation_receipt(
        &self,
        idempotency_key: &str,
        operation_digest: &str,
    ) -> CollectionResult<Option<CollectionOperationReceipt>> {
        let catalogue = self.load()?;
        if catalogue.deletions.contains_key(idempotency_key)
            || catalogue.payload_operations.contains_key(idempotency_key)
        {
            return Err(CollectionError::IdempotencyConflict);
        }
        let Some(receipt) = catalogue.operations.get(idempotency_key) else {
            return Ok(None);
        };
        if receipt.operation_digest != operation_digest {
            return Err(CollectionError::IdempotencyConflict);
        }
        Ok(Some(receipt.clone()))
    }

    pub fn deletion_receipt(
        &self,
        idempotency_key: &str,
        operation_digest: &str,
    ) -> CollectionResult<Option<CollectionDeletionReceipt>> {
        let catalogue = self.load()?;
        if catalogue.operations.contains_key(idempotency_key)
            || catalogue.payload_operations.contains_key(idempotency_key)
        {
            return Err(CollectionError::IdempotencyConflict);
        }
        let Some(receipt) = catalogue.deletions.get(idempotency_key) else {
            return Ok(None);
        };
        if receipt.operation_digest != operation_digest {
            return Err(CollectionError::IdempotencyConflict);
        }
        Ok(Some(receipt.clone()))
    }

    pub fn payload_operation_receipt(
        &self,
        idempotency_key: &str,
        operation_digest: &str,
    ) -> CollectionResult<Option<PayloadIndexOperationReceipt>> {
        let catalogue = self.load()?;
        if catalogue.operations.contains_key(idempotency_key)
            || catalogue.deletions.contains_key(idempotency_key)
        {
            return Err(CollectionError::IdempotencyConflict);
        }
        let Some(receipt) = catalogue.payload_operations.get(idempotency_key) else {
            return Ok(None);
        };
        if receipt.operation_digest != operation_digest {
            return Err(CollectionError::IdempotencyConflict);
        }
        Ok(Some(receipt.clone()))
    }

    pub fn ensure(
        &self,
        context: &CollectionMutationContext,
        idempotency_key: String,
        operation_digest: String,
        definition: VectorCollectionDefinition,
    ) -> CollectionResult<(CollectionCatalogue, bool)> {
        context.validate()?;
        definition.validate()?;
        if idempotency_key.is_empty() || !valid_digest(&operation_digest) {
            return invalid("collection ensure requires an idempotency key and SHA-256 digest");
        }
        if self
            .operation_receipt(&idempotency_key, &operation_digest)?
            .is_some()
        {
            return Ok((self.load()?, true));
        }
        let expected = self.engine.control_record(&self.key)?;
        let mut catalogue = match &expected {
            Some(bytes) => serde_json::from_slice(bytes)
                .map_err(|error| CollectionError::Integrity(error.to_string()))?,
            None => CollectionCatalogue::empty(self.scope.clone()),
        };
        catalogue.validate()?;
        if catalogue.collections.len() >= MAX_COLLECTIONS
            && !catalogue.collections.contains_key(&definition.id)
        {
            return Err(CollectionError::Budget("collection limit exceeded".into()));
        }
        require_operation_budget(&catalogue)?;
        let configuration_digest = definition.config_digest()?;
        let entry = match catalogue.collections.get(&definition.id) {
            Some(existing) if existing.configuration_digest == configuration_digest => {
                existing.clone()
            }
            Some(existing) => CollectionEntry {
                definition,
                payload_indexes: existing.payload_indexes.clone(),
                generation: existing.generation.checked_add(1).ok_or_else(|| {
                    CollectionError::Integrity("collection generation overflow".into())
                })?,
                created_at: existing.created_at,
                updated_at: context.at,
                configuration_digest,
            },
            None => CollectionEntry {
                definition,
                payload_indexes: BTreeMap::new(),
                generation: 1,
                created_at: context.at,
                updated_at: context.at,
                configuration_digest,
            },
        };
        entry.validate()?;
        catalogue
            .collections
            .insert(entry.definition.id.clone(), entry.clone());
        catalogue.operations.insert(
            idempotency_key.clone(),
            CollectionOperationReceipt {
                operation_digest: operation_digest.clone(),
                entry,
            },
        );
        catalogue.revision = catalogue
            .revision
            .checked_add(1)
            .ok_or_else(|| CollectionError::Integrity("catalogue revision overflow".into()))?;
        catalogue.validate()?;
        let replacement = serde_json::to_vec(&catalogue)
            .map_err(|error| CollectionError::Integrity(error.to_string()))?;
        match self.engine.commit_catalog_transition(
            &self.scope,
            &ControlTransition {
                key: self.key.clone(),
                expected,
                replacement: Some(replacement),
                at: context.at,
                actor: context.actor.clone(),
                action: "vector_collection.ensured".into(),
                request_id: context.request_id.clone(),
                operation_id: context.operation_id.clone(),
            },
        ) {
            Ok(_) => Ok((catalogue, false)),
            Err(rrd_store::Error::ControlConflict(_)) => {
                if self
                    .operation_receipt(&idempotency_key, &operation_digest)?
                    .is_some()
                {
                    Ok((self.load()?, true))
                } else {
                    Err(CollectionError::Store(rrd_store::Error::ControlConflict(
                        self.key.clone(),
                    )))
                }
            }
            Err(error) => Err(CollectionError::Store(error)),
        }
    }

    pub fn delete(
        &self,
        context: &CollectionMutationContext,
        idempotency_key: String,
        operation_digest: String,
        collection_id: &ProjectionId,
    ) -> CollectionResult<(CollectionCatalogue, CollectionEntry, bool)> {
        context.validate()?;
        validate_operation_identity(&idempotency_key, &operation_digest, "collection delete")?;
        if let Some(receipt) = self.deletion_receipt(&idempotency_key, &operation_digest)? {
            return Ok((self.load()?, receipt.deleted_entry, true));
        }
        let expected = self.engine.control_record(&self.key)?;
        let mut catalogue = catalogue_from_expected(&self.scope, &expected)?;
        require_operation_budget(&catalogue)?;
        let deleted_entry = catalogue.collections.remove(collection_id).ok_or_else(|| {
            CollectionError::Invalid(format!("unknown vector collection {collection_id}"))
        })?;
        catalogue.deletions.insert(
            idempotency_key.clone(),
            CollectionDeletionReceipt {
                operation_digest: operation_digest.clone(),
                deleted_entry: deleted_entry.clone(),
            },
        );
        bump_catalogue_revision(&mut catalogue)?;
        let (catalogue, replayed) = self.commit_mutation(
            context,
            expected,
            catalogue,
            "vector_collection.deleted",
            || self.deletion_receipt(&idempotency_key, &operation_digest),
        )?;
        let deleted_entry = if replayed {
            self.deletion_receipt(&idempotency_key, &operation_digest)?
                .ok_or_else(|| {
                    CollectionError::Integrity("winning deletion receipt disappeared".into())
                })?
                .deleted_entry
        } else {
            deleted_entry
        };
        Ok((catalogue, deleted_entry, replayed))
    }

    pub fn ensure_payload_index(
        &self,
        context: &CollectionMutationContext,
        idempotency_key: String,
        operation_digest: String,
        collection_id: &ProjectionId,
        definition: PayloadIndexDefinition,
    ) -> CollectionResult<(CollectionCatalogue, PayloadIndexEntry, bool)> {
        context.validate()?;
        validate_operation_identity(&idempotency_key, &operation_digest, "payload-index ensure")?;
        if let Some(receipt) =
            self.payload_operation_receipt(&idempotency_key, &operation_digest)?
        {
            if receipt.deleted {
                return Err(CollectionError::IdempotencyConflict);
            }
            return Ok((self.load()?, receipt.index, true));
        }
        let expected = self.engine.control_record(&self.key)?;
        let mut catalogue = catalogue_from_expected(&self.scope, &expected)?;
        require_operation_budget(&catalogue)?;
        let collection = catalogue
            .collections
            .get_mut(collection_id)
            .ok_or_else(|| {
                CollectionError::Invalid(format!("unknown vector collection {collection_id}"))
            })?;
        if collection.payload_indexes.len() >= MAX_PAYLOAD_INDEXES
            && !collection.payload_indexes.contains_key(&definition.field)
        {
            return Err(CollectionError::Budget(
                "collection payload-index limit exceeded".into(),
            ));
        }
        let configuration_digest = definition.config_digest()?;
        let index = match collection.payload_indexes.get(&definition.field) {
            Some(existing) if existing.configuration_digest == configuration_digest => {
                existing.clone()
            }
            Some(existing) => PayloadIndexEntry {
                definition,
                generation: existing.generation.checked_add(1).ok_or_else(|| {
                    CollectionError::Integrity("payload-index generation overflow".into())
                })?,
                created_at: existing.created_at,
                updated_at: context.at,
                configuration_digest,
            },
            None => PayloadIndexEntry {
                definition,
                generation: 1,
                created_at: context.at,
                updated_at: context.at,
                configuration_digest,
            },
        };
        index.validate()?;
        let changed = collection
            .payload_indexes
            .get(&index.definition.field)
            .is_none_or(|existing| existing != &index);
        collection
            .payload_indexes
            .insert(index.definition.field.clone(), index.clone());
        if changed {
            bump_collection_generation(collection, context.at)?;
        }
        let receipt = PayloadIndexOperationReceipt {
            operation_digest: operation_digest.clone(),
            collection: collection.clone(),
            index: index.clone(),
            deleted: false,
        };
        catalogue
            .payload_operations
            .insert(idempotency_key.clone(), receipt);
        bump_catalogue_revision(&mut catalogue)?;
        let (catalogue, replayed) = self.commit_mutation(
            context,
            expected,
            catalogue,
            "vector_payload_index.ensured",
            || self.payload_operation_receipt(&idempotency_key, &operation_digest),
        )?;
        let index = if replayed {
            self.payload_operation_receipt(&idempotency_key, &operation_digest)?
                .ok_or_else(|| {
                    CollectionError::Integrity("winning payload-index receipt disappeared".into())
                })?
                .index
        } else {
            index
        };
        Ok((catalogue, index, replayed))
    }

    pub fn delete_payload_index(
        &self,
        context: &CollectionMutationContext,
        idempotency_key: String,
        operation_digest: String,
        collection_id: &ProjectionId,
        field: &ProjectionId,
    ) -> CollectionResult<(CollectionCatalogue, PayloadIndexEntry, bool)> {
        context.validate()?;
        validate_operation_identity(&idempotency_key, &operation_digest, "payload-index delete")?;
        if let Some(receipt) =
            self.payload_operation_receipt(&idempotency_key, &operation_digest)?
        {
            if !receipt.deleted {
                return Err(CollectionError::IdempotencyConflict);
            }
            return Ok((self.load()?, receipt.index, true));
        }
        let expected = self.engine.control_record(&self.key)?;
        let mut catalogue = catalogue_from_expected(&self.scope, &expected)?;
        require_operation_budget(&catalogue)?;
        let collection = catalogue
            .collections
            .get_mut(collection_id)
            .ok_or_else(|| {
                CollectionError::Invalid(format!("unknown vector collection {collection_id}"))
            })?;
        let index = collection.payload_indexes.remove(field).ok_or_else(|| {
            CollectionError::Invalid(format!(
                "unknown payload index {field} in collection {collection_id}"
            ))
        })?;
        bump_collection_generation(collection, context.at)?;
        let receipt = PayloadIndexOperationReceipt {
            operation_digest: operation_digest.clone(),
            collection: collection.clone(),
            index: index.clone(),
            deleted: true,
        };
        catalogue
            .payload_operations
            .insert(idempotency_key.clone(), receipt);
        bump_catalogue_revision(&mut catalogue)?;
        let (catalogue, replayed) = self.commit_mutation(
            context,
            expected,
            catalogue,
            "vector_payload_index.deleted",
            || self.payload_operation_receipt(&idempotency_key, &operation_digest),
        )?;
        let index = if replayed {
            self.payload_operation_receipt(&idempotency_key, &operation_digest)?
                .ok_or_else(|| {
                    CollectionError::Integrity("winning payload-index receipt disappeared".into())
                })?
                .index
        } else {
            index
        };
        Ok((catalogue, index, replayed))
    }

    fn commit_mutation<R>(
        &self,
        context: &CollectionMutationContext,
        expected: Option<Vec<u8>>,
        catalogue: CollectionCatalogue,
        action: &str,
        replay: impl FnOnce() -> CollectionResult<Option<R>>,
    ) -> CollectionResult<(CollectionCatalogue, bool)> {
        catalogue.validate()?;
        let replacement = serde_json::to_vec(&catalogue)
            .map_err(|error| CollectionError::Integrity(error.to_string()))?;
        match self.engine.commit_catalog_transition(
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
        ) {
            Ok(_) => Ok((catalogue, false)),
            Err(rrd_store::Error::ControlConflict(_)) => {
                if replay()?.is_some() {
                    Ok((self.load()?, true))
                } else {
                    Err(CollectionError::Store(rrd_store::Error::ControlConflict(
                        self.key.clone(),
                    )))
                }
            }
            Err(error) => Err(CollectionError::Store(error)),
        }
    }
}

fn catalogue_from_expected(
    scope: &ScopeId,
    expected: &Option<Vec<u8>>,
) -> CollectionResult<CollectionCatalogue> {
    let catalogue = match expected {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|error| CollectionError::Integrity(error.to_string()))?,
        None => CollectionCatalogue::empty(scope.clone()),
    };
    catalogue.validate()?;
    Ok(catalogue)
}

fn require_operation_budget(catalogue: &CollectionCatalogue) -> CollectionResult<()> {
    let count = catalogue
        .operations
        .len()
        .checked_add(catalogue.deletions.len())
        .and_then(|count| count.checked_add(catalogue.payload_operations.len()))
        .ok_or_else(|| CollectionError::Integrity("operation receipt count overflow".into()))?;
    if count >= MAX_OPERATION_RECEIPTS {
        return Err(CollectionError::Budget(
            "operation receipt limit exceeded".into(),
        ));
    }
    Ok(())
}

fn validate_operation_identity(
    idempotency_key: &str,
    operation_digest: &str,
    operation: &str,
) -> CollectionResult<()> {
    if idempotency_key.is_empty() || !valid_digest(operation_digest) {
        return invalid(format!(
            "{operation} requires an idempotency key and SHA-256 digest"
        ));
    }
    Ok(())
}

fn bump_catalogue_revision(catalogue: &mut CollectionCatalogue) -> CollectionResult<()> {
    catalogue.revision = catalogue
        .revision
        .checked_add(1)
        .ok_or_else(|| CollectionError::Integrity("catalogue revision overflow".into()))?;
    Ok(())
}

fn bump_collection_generation(collection: &mut CollectionEntry, at: u64) -> CollectionResult<()> {
    collection.generation = collection
        .generation
        .checked_add(1)
        .ok_or_else(|| CollectionError::Integrity("collection generation overflow".into()))?;
    collection.updated_at = at;
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn invalid<T>(message: impl Into<String>) -> CollectionResult<T> {
    Err(CollectionError::Invalid(message.into()))
}

fn integrity<T>(message: impl Into<String>) -> CollectionResult<T> {
    Err(CollectionError::Integrity(message.into()))
}
