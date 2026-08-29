use super::{
    invalid, validate_data_vector, validate_sha256, validate_vector_scope, CanonicalId,
    DataEmbeddingProvenance, DataVectorNormalization, DataVectorValue, Result, SearchVectors,
    VectorEmbeddingModel, VectorPayloadFilter, VectorSearchMode, VectorSearchQuery,
    VectorSearchResult, MAX_MESSAGE_BYTES,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_EMBEDDING_BATCH_INPUTS: usize = 1_024;
pub const MAX_EMBEDDING_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_EMBEDDING_BATCH_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingModality {
    Text,
    Image,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingNetworkPolicy {
    #[default]
    Deny,
    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmbeddingExecutionTarget {
    Cpu,
    Gpu { platform: String, device: String },
    Remote { provider: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmbeddingTrustBoundary {
    LocalOffline,
    RemoteProvider { provider: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingResourceLimits {
    pub maximum_batch_inputs: u32,
    pub maximum_input_bytes: u64,
    pub maximum_batch_bytes: u64,
    pub maximum_output_values: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingBackendSnapshot {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub revision: String,
    pub model_sha256: String,
    pub modality: EmbeddingModality,
    pub dimensions: u32,
    pub normalization: DataVectorNormalization,
    pub execution: EmbeddingExecutionTarget,
    pub network_required: bool,
    pub trust: EmbeddingTrustBoundary,
    pub resources: EmbeddingResourceLimits,
    pub deterministic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListEmbeddingModels {
    pub scope: String,
}

impl ListEmbeddingModels {
    pub fn validate(&self) -> Result<()> {
        validate_vector_scope(&self.scope)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingModelCatalogue {
    pub scope: String,
    pub registry_revision: u64,
    pub backends: Vec<EmbeddingBackendSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingInput {
    pub id: CanonicalId,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

impl EmbeddingInput {
    fn validate(&self) -> Result<()> {
        if self.media_type.trim().is_empty()
            || self.media_type.len() > MAX_MESSAGE_BYTES
            || self.media_type.contains('\0')
        {
            return invalid("embedding media_type is invalid");
        }
        if self.bytes.is_empty() || self.bytes.len() > MAX_EMBEDDING_INPUT_BYTES {
            return invalid("embedding input bytes must be in 1..=67108864");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateEmbeddings {
    pub scope: String,
    pub backend_id: String,
    #[serde(default)]
    pub network_policy: EmbeddingNetworkPolicy,
    pub inputs: Vec<EmbeddingInput>,
}

impl GenerateEmbeddings {
    pub fn validate(&self) -> Result<()> {
        validate_vector_scope(&self.scope)?;
        validate_backend_id(&self.backend_id)?;
        if self.inputs.is_empty() || self.inputs.len() > MAX_EMBEDDING_BATCH_INPUTS {
            return invalid("embedding batch input count must be in 1..=1024");
        }
        let mut ids = BTreeSet::new();
        let mut total_bytes = 0_usize;
        for input in &self.inputs {
            input.validate()?;
            if !ids.insert(input.id.as_str()) {
                return invalid("embedding batch input ids must be unique");
            }
            total_bytes = total_bytes.checked_add(input.bytes.len()).ok_or_else(|| {
                super::ContractError("embedding batch byte count overflowed".into())
            })?;
        }
        if total_bytes > MAX_EMBEDDING_BATCH_BYTES {
            return invalid("embedding batch bytes exceed 67108864");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedEmbedding {
    pub input_id: CanonicalId,
    pub value: DataVectorValue,
    pub provenance: DataEmbeddingProvenance,
}

impl GeneratedEmbedding {
    pub fn validate(&self) -> Result<()> {
        let dimensions = validate_data_vector(&self.value)?;
        validate_sha256(&self.provenance.source_sha256, "embedding source_sha256")?;
        validate_sha256(&self.provenance.model_sha256, "embedding model_sha256")?;
        if dimensions != self.provenance.dimensions as usize {
            return invalid("generated embedding dimensions differ from provenance");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateEmbeddingsResult {
    pub scope: String,
    pub read_manifest_sha256: String,
    pub known_at_cursor: u64,
    pub registry_revision: u64,
    pub backend: EmbeddingBackendSnapshot,
    pub embeddings: Vec<GeneratedEmbedding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmbedAndSearchVectors {
    pub scope: String,
    pub backend_id: String,
    #[serde(default)]
    pub network_policy: EmbeddingNetworkPolicy,
    pub input: EmbeddingInput,
    pub valid_at: u64,
    pub collection_id: CanonicalId,
    pub vector_name: CanonicalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<VectorPayloadFilter>,
    pub top_k: u64,
    #[serde(default)]
    pub mode: VectorSearchMode,
    pub max_scanned_changes: u64,
}

impl EmbedAndSearchVectors {
    pub fn validate(&self) -> Result<()> {
        GenerateEmbeddings {
            scope: self.scope.clone(),
            backend_id: self.backend_id.clone(),
            network_policy: self.network_policy,
            inputs: vec![self.input.clone()],
        }
        .validate()?;
        SearchVectors {
            scope: self.scope.clone(),
            valid_at: self.valid_at,
            collection_id: Some(self.collection_id.clone()),
            vector_name: Some(self.vector_name.clone()),
            field: None,
            query: VectorSearchQuery::Dense { values: vec![1.0] },
            filter: self.filter.clone(),
            metric: None,
            top_k: self.top_k,
            mode: self.mode,
            max_scanned_changes: self.max_scanned_changes,
        }
        .validate()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmbedAndSearchVectorsResult {
    pub backend: EmbeddingBackendSnapshot,
    pub embedding: GeneratedEmbedding,
    pub search: VectorSearchResult,
}

impl EmbedAndSearchVectorsResult {
    pub fn validate(&self) -> Result<()> {
        self.embedding.validate()?;
        if self.embedding.provenance.model != self.backend.model_name()
            || self.embedding.provenance.model_sha256 != self.backend.model_sha256
        {
            return invalid("embed-and-search provenance differs from its backend");
        }
        if self.search.read_manifest_sha256.is_empty() {
            return invalid("embed-and-search result has no read manifest");
        }
        Ok(())
    }
}

impl EmbeddingBackendSnapshot {
    pub fn model_binding(&self) -> VectorEmbeddingModel {
        VectorEmbeddingModel {
            name: self.model_name(),
            digest: self.model_sha256.clone(),
        }
    }

    pub fn model_name(&self) -> String {
        format!("{}/{}@{}", self.provider, self.model, self.revision)
    }
}

fn validate_backend_id(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > MAX_MESSAGE_BYTES || value.contains('\0') {
        return invalid("embedding backend_id is invalid");
    }
    Ok(())
}
