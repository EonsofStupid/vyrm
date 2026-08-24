use crate::{EmbeddingModelBinding, ScoreMetric};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use vyrm_core::{digest, ProjectionId, ScopeId};
use vyrm_store::{ControlTransition, Engine};

pub const VECTOR_COLLECTION_CATALOGUE_VERSION: u16 = 1;
const MAX_COLLECTIONS: usize = 4_096;
const MAX_NAMED_VECTORS: usize = 64;
const MAX_OPERATION_RECEIPTS: usize = 100_000;

pub type CollectionResult<T> = std::result::Result<T, CollectionError>;

#[derive(Debug)]
pub enum CollectionError {
    Store(vyrm_store::Error),
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

impl From<vyrm_store::Error> for CollectionError {
    fn from(value: vyrm_store::Error) -> Self {
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
pub struct CollectionCatalogue {
    pub contract_version: u16,
    pub scope: ScopeId,
    pub revision: u64,
    pub collections: BTreeMap<ProjectionId, CollectionEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub operations: BTreeMap<String, CollectionOperationReceipt>,
}

impl CollectionCatalogue {
    fn empty(scope: ScopeId) -> Self {
        Self {
            contract_version: VECTOR_COLLECTION_CATALOGUE_VERSION,
            scope,
            revision: 0,
            collections: BTreeMap::new(),
            operations: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> CollectionResult<()> {
        if self.contract_version != VECTOR_COLLECTION_CATALOGUE_VERSION {
            return integrity("unsupported vector collection catalogue version");
        }
        if self.collections.len() > MAX_COLLECTIONS
            || self.operations.len() > MAX_OPERATION_RECEIPTS
        {
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
        let Some(receipt) = catalogue.operations.get(idempotency_key) else {
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
        if catalogue.operations.len() >= MAX_OPERATION_RECEIPTS {
            return Err(CollectionError::Budget(
                "operation receipt limit exceeded".into(),
            ));
        }
        let configuration_digest = definition.config_digest()?;
        let entry = match catalogue.collections.get(&definition.id) {
            Some(existing) if existing.configuration_digest == configuration_digest => {
                existing.clone()
            }
            Some(existing) => CollectionEntry {
                definition,
                generation: existing.generation.checked_add(1).ok_or_else(|| {
                    CollectionError::Integrity("collection generation overflow".into())
                })?,
                created_at: existing.created_at,
                updated_at: context.at,
                configuration_digest,
            },
            None => CollectionEntry {
                definition,
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
        match self.engine.commit_control_transition(&ControlTransition {
            key: self.key.clone(),
            expected,
            replacement: Some(replacement),
            at: context.at,
            actor: context.actor.clone(),
            action: "vector_collection.ensured".into(),
            request_id: context.request_id.clone(),
            operation_id: context.operation_id.clone(),
        }) {
            Ok(_) => Ok((catalogue, false)),
            Err(vyrm_store::Error::ControlConflict(_)) => {
                if self
                    .operation_receipt(&idempotency_key, &operation_digest)?
                    .is_some()
                {
                    Ok((self.load()?, true))
                } else {
                    Err(CollectionError::Store(vyrm_store::Error::ControlConflict(
                        self.key.clone(),
                    )))
                }
            }
            Err(error) => Err(CollectionError::Store(error)),
        }
    }
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
