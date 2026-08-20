//! Provider-neutral embedding jobs with source/model provenance.
//!
//! Inference is never authoritative by itself. A prepared vector is accepted
//! only when the source bytes match the job's expected digest before and after
//! inference, the backend exactly matches the requested model contract, and
//! the returned shape/normalization validates as a canonical `RuntimeVector`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use vyrm_core::{
    digest, DataTransaction, EmbeddingProvenance, Error, Millis, ReadStamp, Result, RuntimeCommit,
    RuntimeId, RuntimeMutation, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector,
    ScopeId, VectorNormalization, VectorValue,
};

#[cfg(feature = "fastembed-local")]
mod fastembed_local;

#[cfg(feature = "fastembed-local")]
pub use fastembed_local::{fastembed_model_digest, FastEmbedLocalBackend, FastEmbedLocalIdentity};

pub const EMBEDDING_CONTRACT_VERSION: u16 = 1;
const MAX_EMBEDDING_INPUT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingModality {
    Text,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicy {
    Deny,
    Allow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkRequirement {
    None,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionTarget {
    Cpu,
    Gpu { platform: String, device: String },
    Remote { provider: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingModelSpec {
    pub provider: String,
    pub model: String,
    pub revision: String,
    pub model_digest: String,
    pub modality: EmbeddingModality,
    pub dimensions: u32,
    pub normalization: VectorNormalization,
    pub maximum_input_bytes: u64,
}

impl EmbeddingModelSpec {
    pub fn validate(&self) -> Result<()> {
        validate_text("embedding provider", &self.provider)?;
        validate_text("embedding model", &self.model)?;
        validate_text("embedding revision", &self.revision)?;
        validate_digest("embedding model", &self.model_digest)?;
        if self.dimensions == 0 || self.dimensions > 1_048_576 {
            return invalid("embedding model dimensions must be in 1..=1048576");
        }
        if self.maximum_input_bytes == 0
            || self.maximum_input_bytes > MAX_EMBEDDING_INPUT_BYTES as u64
        {
            return invalid("embedding maximum input bytes must be in 1..=67108864");
        }
        Ok(())
    }

    pub fn canonical_name(&self) -> String {
        format!("{}/{}@{}", self.provider, self.model, self.revision)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingBackendDescriptor {
    pub id: String,
    pub model: EmbeddingModelSpec,
    pub execution: ExecutionTarget,
    pub network: NetworkRequirement,
    pub deterministic: bool,
}

impl EmbeddingBackendDescriptor {
    pub fn validate(&self) -> Result<()> {
        validate_text("embedding backend id", &self.id)?;
        self.model.validate()?;
        match &self.execution {
            ExecutionTarget::Cpu => {}
            ExecutionTarget::Gpu { platform, device } => {
                validate_text("embedding GPU platform", platform)?;
                validate_text("embedding GPU device", device)?;
            }
            ExecutionTarget::Remote { provider } => {
                validate_text("embedding remote provider", provider)?;
                if self.network != NetworkRequirement::Required {
                    return invalid("remote embedding execution must require network access");
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingJob {
    pub contract_version: u16,
    pub id: RuntimeId,
    pub scope: ScopeId,
    pub read: ReadStamp,
    pub source: RuntimeRef,
    pub expected_source_digest: String,
    pub target: RuntimeRef,
    pub subject: RuntimeRef,
    pub field: String,
    pub valid_from: Millis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Millis>,
    pub model: EmbeddingModelSpec,
    pub network_policy: NetworkPolicy,
    pub requested_at: Millis,
    #[serde(default)]
    pub properties: RuntimeProperties,
}

impl EmbeddingJob {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != EMBEDDING_CONTRACT_VERSION {
            return invalid("unsupported embedding job contract version");
        }
        self.read.validate()?;
        if self.read.scope != self.scope {
            return invalid("embedding job scope differs from its read stamp");
        }
        validate_digest("embedding source", &self.expected_source_digest)?;
        validate_text("embedding field", &self.field)?;
        if self
            .valid_to
            .is_some_and(|valid_to| valid_to <= self.valid_from)
        {
            return invalid("embedding valid-time window must be half-open and non-empty");
        }
        self.model.validate()
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        let mut bytes = b"vyrm-embedding-job-v1\0".to_vec();
        bytes.extend_from_slice(&serde_json::to_vec(self).map_err(|error| {
            Error::InvalidRuntime {
                reason: format!("embedding job cannot be encoded: {error}"),
            }
        })?);
        Ok(digest::sha256_hex(&bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingSourceSnapshot {
    pub source: RuntimeRef,
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub digest: String,
}

impl EmbeddingSourceSnapshot {
    pub fn for_bytes(
        source: RuntimeRef,
        media_type: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Self> {
        let bytes = bytes.into();
        let snapshot = Self {
            source,
            media_type: media_type.into(),
            digest: digest::sha256_hex(&bytes),
            bytes,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("embedding media type", &self.media_type)?;
        if self.bytes.is_empty() || self.bytes.len() > MAX_EMBEDDING_INPUT_BYTES {
            return invalid("embedding source bytes must be in 1..=67108864");
        }
        validate_digest("embedding source", &self.digest)?;
        if digest::sha256_hex(&self.bytes) != self.digest {
            return invalid("embedding source digest does not match its bytes");
        }
        Ok(())
    }
}

pub trait EmbeddingSourceReader {
    fn read(&mut self, source: &RuntimeRef) -> Result<EmbeddingSourceSnapshot>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingRequest {
    pub job_id: RuntimeId,
    pub job_digest: String,
    pub source_digest: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

pub trait EmbeddingBackend {
    fn descriptor(&self) -> &EmbeddingBackendDescriptor;
    fn embed(&mut self, request: &EmbeddingRequest) -> Result<VectorValue>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedEmbedding {
    pub contract_version: u16,
    pub job_id: RuntimeId,
    pub job_digest: String,
    pub scope: ScopeId,
    pub backend: EmbeddingBackendDescriptor,
    pub vector: RuntimeVector,
}

impl PreparedEmbedding {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != EMBEDDING_CONTRACT_VERSION {
            return invalid("unsupported prepared embedding contract version");
        }
        validate_digest("embedding job", &self.job_digest)?;
        self.backend.validate()?;
        self.vector.validate()?;
        let provenance = self
            .vector
            .provenance
            .as_ref()
            .ok_or_else(|| Error::InvalidRuntime {
                reason: "prepared embedding has no provenance".into(),
            })?;
        if provenance.model != self.backend.model.canonical_name()
            || provenance.model_digest != self.backend.model.model_digest
            || provenance.dimensions != self.backend.model.dimensions
            || provenance.normalization != self.backend.model.normalization
        {
            return invalid("prepared vector provenance differs from its backend model");
        }
        Ok(())
    }

    pub fn into_mutation(self) -> RuntimeMutation {
        RuntimeMutation::Vector {
            vector: self.vector,
        }
    }

    pub fn transaction(
        &self,
        job: &EmbeddingJob,
        actor: impl Into<String>,
        at: Millis,
    ) -> Result<DataTransaction> {
        self.validate()?;
        job.validate()?;
        if job.digest()? != self.job_digest
            || self.job_id != job.id
            || self.scope != job.scope
            || self.vector.reference != job.target
            || self.vector.subject != job.subject
            || self.vector.field != job.field
        {
            return invalid("prepared embedding differs from its source job");
        }
        if at < job.requested_at {
            return invalid("embedding commit time precedes its request time");
        }
        DataTransaction::new(
            job.read.clone(),
            RuntimeCommit {
                scope: job.scope.clone(),
                at,
                actor: actor.into(),
                expected_cursor: job.read.commit_cursor,
                mutations: vec![RuntimeMutation::Vector {
                    vector: self.vector.clone(),
                }],
            },
        )
    }
}

pub struct EmbeddingCoordinator;

impl EmbeddingCoordinator {
    pub fn prepare<S: EmbeddingSourceReader, B: EmbeddingBackend>(
        job: &EmbeddingJob,
        source_reader: &mut S,
        backend: &mut B,
    ) -> Result<PreparedEmbedding> {
        job.validate()?;
        let job_digest = job.digest()?;
        let descriptor = backend.descriptor().clone();
        descriptor.validate()?;
        if descriptor.model != job.model {
            return invalid("embedding backend model differs from the requested model");
        }
        if job.network_policy == NetworkPolicy::Deny
            && descriptor.network == NetworkRequirement::Required
        {
            return invalid("embedding job denies the network required by its backend");
        }

        let before = source_reader.read(&job.source)?;
        validate_source(job, &before)?;
        let request = EmbeddingRequest {
            job_id: job.id.clone(),
            job_digest: job_digest.clone(),
            source_digest: before.digest.clone(),
            media_type: before.media_type.clone(),
            bytes: before.bytes,
        };
        let value = backend.embed(&request)?;

        // The source is sampled again after inference. A transaction-level CAS
        // remains the final commit authority, but this closes the expensive
        // inference race before a vector can even enter a commit.
        let after = source_reader.read(&job.source)?;
        after.validate()?;
        if after.source != job.source {
            return invalid("embedding source reader returned the wrong identity");
        }
        if after.digest != request.source_digest {
            return invalid("embedding source changed during inference");
        }
        validate_source(job, &after)?;

        let mut generation_parameters = RuntimeProperties::new();
        generation_parameters.insert(
            "backend".into(),
            RuntimeValue::String(descriptor.id.clone()),
        );
        generation_parameters.insert(
            "execution".into(),
            RuntimeValue::String(execution_name(&descriptor.execution)),
        );
        generation_parameters.insert(
            "job_digest".into(),
            RuntimeValue::Digest(job_digest.clone()),
        );
        generation_parameters.insert(
            "deterministic".into(),
            RuntimeValue::Bool(descriptor.deterministic),
        );
        let vector = RuntimeVector {
            reference: job.target.clone(),
            subject: job.subject.clone(),
            field: job.field.clone(),
            valid_from: job.valid_from,
            valid_to: job.valid_to,
            value,
            provenance: Some(EmbeddingProvenance {
                source_digest: request.source_digest,
                model: descriptor.model.canonical_name(),
                model_digest: descriptor.model.model_digest.clone(),
                dimensions: descriptor.model.dimensions,
                normalization: descriptor.model.normalization,
                generation_parameters,
            }),
            properties: job.properties.clone(),
        };
        let prepared = PreparedEmbedding {
            contract_version: EMBEDDING_CONTRACT_VERSION,
            job_id: job.id.clone(),
            job_digest,
            scope: job.scope.clone(),
            backend: descriptor,
            vector,
        };
        prepared.validate()?;
        Ok(prepared)
    }
}

/// Deterministic, dependency-free local baseline for offline operation.
///
/// It is a feature-hashing model, not a semantic-model quality claim. Its role
/// is to keep the full source/provenance/commit pipeline executable when no
/// ONNX or accelerator adapter is installed.
pub struct FeatureHashBackend {
    descriptor: EmbeddingBackendDescriptor,
    seed: u64,
}

impl FeatureHashBackend {
    pub fn new(dimensions: u32, seed: u64) -> Result<Self> {
        if !(8..=65_536).contains(&dimensions) {
            return invalid("feature-hash dimensions must be in 8..=65536");
        }
        let model_identity = serde_json::to_vec(&("vyrm-feature-hash-v1", dimensions, seed))
            .map_err(|error| Error::InvalidRuntime {
                reason: format!("feature-hash model identity cannot be encoded: {error}"),
            })?;
        Ok(Self {
            descriptor: EmbeddingBackendDescriptor {
                id: "vyrm:feature-hash:cpu:v1".into(),
                model: EmbeddingModelSpec {
                    provider: "vyrm".into(),
                    model: "feature-hash".into(),
                    revision: "v1".into(),
                    model_digest: digest::sha256_hex(&model_identity),
                    modality: EmbeddingModality::Text,
                    dimensions,
                    normalization: VectorNormalization::UnitL2,
                    maximum_input_bytes: 4 * 1024 * 1024,
                },
                execution: ExecutionTarget::Cpu,
                network: NetworkRequirement::None,
                deterministic: true,
            },
            seed,
        })
    }
}

impl EmbeddingBackend for FeatureHashBackend {
    fn descriptor(&self) -> &EmbeddingBackendDescriptor {
        &self.descriptor
    }

    fn embed(&mut self, request: &EmbeddingRequest) -> Result<VectorValue> {
        if request.bytes.len() > self.descriptor.model.maximum_input_bytes as usize {
            return invalid("embedding input exceeds model byte limit");
        }
        if !request.media_type.starts_with("text/") && request.media_type != "application/json" {
            return invalid("feature-hash backend accepts text or JSON only");
        }
        let text = std::str::from_utf8(&request.bytes)
            .map_err(|_| Error::InvalidRuntime {
                reason: "feature-hash input must be UTF-8".into(),
            })?
            .to_lowercase();
        let mut values = vec![0.0_f32; self.descriptor.model.dimensions as usize];
        let mut tokens = BTreeSet::new();
        for token in text.split(|character: char| !character.is_alphanumeric()) {
            if !token.is_empty() {
                tokens.insert(token);
            }
        }
        if tokens.is_empty() {
            return invalid("feature-hash input contains no tokens");
        }
        for token in tokens {
            let mut identity = b"vyrm-feature-hash-token-v1\0".to_vec();
            identity.extend_from_slice(&self.seed.to_be_bytes());
            identity.extend_from_slice(token.as_bytes());
            let hash = digest::sha256(&identity);
            let index = u64::from_be_bytes(hash[..8].try_into().expect("eight-byte hash prefix"))
                as usize
                % values.len();
            let sign = if hash[8] & 1 == 0 { 1.0 } else { -1.0 };
            values[index] += sign;
        }
        let norm = values
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>()
            .sqrt() as f32;
        for value in &mut values {
            *value /= norm;
        }
        Ok(VectorValue::Dense { values })
    }
}

fn validate_source(job: &EmbeddingJob, snapshot: &EmbeddingSourceSnapshot) -> Result<()> {
    snapshot.validate()?;
    if snapshot.source != job.source {
        return invalid("embedding source reader returned the wrong identity");
    }
    if snapshot.digest != job.expected_source_digest {
        return invalid("embedding source differs from the job's expected digest");
    }
    if snapshot.bytes.len() > job.model.maximum_input_bytes as usize {
        return invalid("embedding source exceeds the requested model byte limit");
    }
    match job.model.modality {
        EmbeddingModality::Text
            if !snapshot.media_type.starts_with("text/")
                && snapshot.media_type != "application/json" =>
        {
            invalid("text embedding job received a non-text source")
        }
        EmbeddingModality::Image if !snapshot.media_type.starts_with("image/") => {
            invalid("image embedding job received a non-image source")
        }
        _ => Ok(()),
    }
}

fn execution_name(execution: &ExecutionTarget) -> String {
    match execution {
        ExecutionTarget::Cpu => "cpu".into(),
        ExecutionTarget::Gpu { platform, device } => format!("gpu:{platform}:{device}"),
        ExecutionTarget::Remote { provider } => format!("remote:{provider}"),
    }
}

fn validate_text(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.as_bytes().contains(&0) {
        return invalid(format!(
            "{label} must be non-empty and contain no NUL bytes"
        ));
    }
    Ok(())
}

fn validate_digest(label: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return invalid(format!("{label} digest must be lowercase SHA-256 hex"));
    }
    Ok(())
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidRuntime {
        reason: reason.into(),
    })
}
