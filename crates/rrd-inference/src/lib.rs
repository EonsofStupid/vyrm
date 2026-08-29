//! Provider-neutral embedding jobs with source/model provenance.
//!
//! Inference is never authoritative by itself. A prepared vector is accepted
//! only when the source bytes match the job's expected digest before and after
//! inference, the backend exactly matches the requested model contract, and
//! the returned shape/normalization validates as a canonical `RuntimeVector`.

use rrd_core::{
    digest, DataTransaction, EmbeddingProvenance, Error, Millis, ReadStamp, Result, RuntimeCommit,
    RuntimeId, RuntimeMutation, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector,
    ScopeId, VectorNormalization, VectorValue,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(feature = "fastembed-local")]
mod fastembed_local;

#[cfg(feature = "fastembed-local")]
pub use fastembed_local::{fastembed_model_digest, FastEmbedLocalBackend, FastEmbedLocalIdentity};

pub const EMBEDDING_CONTRACT_VERSION: u16 = 1;
const MAX_EMBEDDING_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_EMBEDDING_BATCH_INPUTS: u32 = 1_024;
pub const MAX_EMBEDDING_BATCH_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_EMBEDDING_OUTPUT_VALUES: u64 = 1_073_741_824;

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

/// Declares where unredacted inference input is allowed to cross.
///
/// A local/offline backend cannot require the network or declare a remote
/// execution target. A provider backend must name the same provider at both
/// the execution and trust boundaries and must require an allow-network job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InferenceTrustBoundary {
    LocalOffline,
    RemoteProvider { provider: String },
}

/// Hard dispatch limits enforced by the coordinator before backend code runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingResourceLimits {
    pub maximum_batch_inputs: u32,
    pub maximum_input_bytes: u64,
    pub maximum_batch_bytes: u64,
    pub maximum_output_values: u64,
}

impl EmbeddingResourceLimits {
    pub fn for_model(model: &EmbeddingModelSpec, maximum_batch_inputs: u32) -> Result<Self> {
        model.validate()?;
        let maximum_batch_bytes = model
            .maximum_input_bytes
            .checked_mul(u64::from(maximum_batch_inputs))
            .ok_or_else(|| Error::InvalidRuntime {
                reason: "embedding batch byte limit overflowed u64".into(),
            })?
            .min(MAX_EMBEDDING_BATCH_BYTES);
        let maximum_output_values = u64::from(model.dimensions)
            .checked_mul(u64::from(maximum_batch_inputs))
            .ok_or_else(|| Error::InvalidRuntime {
                reason: "embedding output value limit overflowed u64".into(),
            })?
            .min(MAX_EMBEDDING_OUTPUT_VALUES);
        let limits = Self {
            maximum_batch_inputs,
            maximum_input_bytes: model.maximum_input_bytes,
            maximum_batch_bytes,
            maximum_output_values,
        };
        limits.validate(model)?;
        Ok(limits)
    }

    pub fn validate(&self, model: &EmbeddingModelSpec) -> Result<()> {
        if self.maximum_batch_inputs == 0 || self.maximum_batch_inputs > MAX_EMBEDDING_BATCH_INPUTS
        {
            return invalid("embedding maximum batch inputs must be in 1..=1024");
        }
        if self.maximum_input_bytes == 0 || self.maximum_input_bytes > model.maximum_input_bytes {
            return invalid("embedding per-input byte limit exceeds the model contract");
        }
        if self.maximum_batch_bytes < self.maximum_input_bytes
            || self.maximum_batch_bytes > MAX_EMBEDDING_BATCH_BYTES
        {
            return invalid("embedding batch byte limit is invalid");
        }
        if self.maximum_output_values < u64::from(model.dimensions)
            || self.maximum_output_values > MAX_EMBEDDING_OUTPUT_VALUES
        {
            return invalid("embedding output value limit is invalid");
        }
        Ok(())
    }
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
    pub trust: InferenceTrustBoundary,
    pub resources: EmbeddingResourceLimits,
    pub deterministic: bool,
}

impl EmbeddingBackendDescriptor {
    pub fn validate(&self) -> Result<()> {
        validate_text("embedding backend id", &self.id)?;
        self.model.validate()?;
        self.resources.validate(&self.model)?;
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
        match (&self.trust, &self.execution, self.network) {
            (
                InferenceTrustBoundary::LocalOffline,
                ExecutionTarget::Cpu | ExecutionTarget::Gpu { .. },
                NetworkRequirement::None,
            ) => {}
            (
                InferenceTrustBoundary::RemoteProvider { provider: trusted },
                ExecutionTarget::Remote { provider: target },
                NetworkRequirement::Required,
            ) if trusted == target => validate_text("embedding trusted provider", trusted)?,
            (InferenceTrustBoundary::LocalOffline, _, _) => {
                return invalid("local/offline inference cannot require a remote trust boundary");
            }
            (InferenceTrustBoundary::RemoteProvider { .. }, _, _) => {
                return invalid(
                    "provider inference must match its remote execution and network boundary",
                );
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
        let mut bytes = b"rrd-inferenceding-job-v1\0".to_vec();
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

impl EmbeddingRequest {
    pub fn validate(&self) -> Result<()> {
        validate_digest("embedding job", &self.job_digest)?;
        validate_digest("embedding source", &self.source_digest)?;
        validate_text("embedding media type", &self.media_type)?;
        if self.bytes.is_empty() || self.bytes.len() > MAX_EMBEDDING_INPUT_BYTES {
            return invalid("embedding request bytes must be in 1..=67108864");
        }
        if digest::sha256_hex(&self.bytes) != self.source_digest {
            return invalid("embedding request source digest does not match its bytes");
        }
        Ok(())
    }
}

pub trait EmbeddingBackend {
    fn descriptor(&self) -> &EmbeddingBackendDescriptor;
    fn embed(&mut self, request: &EmbeddingRequest) -> Result<VectorValue>;

    /// Backends with a native batching primitive should override this method.
    /// The coordinator applies the same batch limits before either path runs.
    fn embed_batch(&mut self, requests: &[EmbeddingRequest]) -> Result<Vec<VectorValue>> {
        requests.iter().map(|request| self.embed(request)).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingBatchResult {
    pub registry_revision: u64,
    pub backend: EmbeddingBackendDescriptor,
    pub vectors: Vec<VectorValue>,
}

/// Process-owned registry of executable adapters. Durable data stores exact
/// model provenance; executable sessions, accelerator handles, and provider
/// credentials remain process-local and are never serialized into RRD.
#[derive(Default)]
pub struct EmbeddingBackendRegistry {
    revision: u64,
    backends: BTreeMap<String, Box<dyn EmbeddingBackend + Send>>,
}

impl EmbeddingBackendRegistry {
    pub fn install(&mut self, backend: Box<dyn EmbeddingBackend + Send>) -> Result<u64> {
        let descriptor = backend.descriptor();
        descriptor.validate()?;
        if self.backends.contains_key(&descriptor.id) {
            return invalid(format!(
                "embedding backend {} is already installed",
                descriptor.id
            ));
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::InvalidRuntime {
                reason: "embedding registry revision overflowed u64".into(),
            })?;
        self.backends.insert(descriptor.id.clone(), backend);
        Ok(self.revision)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn descriptors(&self) -> Vec<EmbeddingBackendDescriptor> {
        self.backends
            .values()
            .map(|backend| backend.descriptor().clone())
            .collect()
    }

    pub fn descriptor(&self, backend_id: &str) -> Option<&EmbeddingBackendDescriptor> {
        self.backends
            .get(backend_id)
            .map(|backend| backend.descriptor())
    }

    pub fn embed_batch(
        &mut self,
        backend_id: &str,
        model: &EmbeddingModelSpec,
        network_policy: NetworkPolicy,
        requests: &[EmbeddingRequest],
    ) -> Result<EmbeddingBatchResult> {
        let backend = self
            .backends
            .get_mut(backend_id)
            .ok_or_else(|| Error::InvalidRuntime {
                reason: format!("embedding backend {backend_id} is not installed"),
            })?;
        let descriptor = backend.descriptor().clone();
        if &descriptor.model != model {
            return invalid("embedding backend model differs from the requested model");
        }
        let vectors = dispatch_embedding_batch(backend.as_mut(), network_policy, requests)?;
        Ok(EmbeddingBatchResult {
            registry_revision: self.revision,
            backend: descriptor,
            vectors,
        })
    }
}

fn dispatch_embedding_batch(
    backend: &mut dyn EmbeddingBackend,
    network_policy: NetworkPolicy,
    requests: &[EmbeddingRequest],
) -> Result<Vec<VectorValue>> {
    let descriptor = backend.descriptor().clone();
    descriptor.validate()?;
    if network_policy == NetworkPolicy::Deny && descriptor.network == NetworkRequirement::Required {
        return invalid("embedding request denies the network required by its backend");
    }
    if requests.is_empty()
        || requests.len()
            > usize::try_from(descriptor.resources.maximum_batch_inputs).map_err(|_| {
                Error::InvalidRuntime {
                    reason: "embedding batch input limit exceeds usize".into(),
                }
            })?
    {
        return invalid("embedding batch input count exceeds the backend limit");
    }
    let mut total_bytes = 0_u64;
    let mut job_ids = BTreeSet::new();
    for request in requests {
        request.validate()?;
        if !job_ids.insert(request.job_id.clone()) {
            return invalid("embedding batch job ids must be unique");
        }
        let bytes = u64::try_from(request.bytes.len()).map_err(|_| Error::InvalidRuntime {
            reason: "embedding input length exceeds u64".into(),
        })?;
        if bytes > descriptor.resources.maximum_input_bytes {
            return invalid("embedding input exceeds the backend byte limit");
        }
        total_bytes = total_bytes
            .checked_add(bytes)
            .ok_or_else(|| Error::InvalidRuntime {
                reason: "embedding batch byte count overflowed u64".into(),
            })?;
    }
    if total_bytes > descriptor.resources.maximum_batch_bytes {
        return invalid("embedding batch exceeds the backend byte limit");
    }
    let output_values = u64::from(descriptor.model.dimensions)
        .checked_mul(
            u64::try_from(requests.len()).map_err(|_| Error::InvalidRuntime {
                reason: "embedding batch size exceeds u64".into(),
            })?,
        )
        .ok_or_else(|| Error::InvalidRuntime {
            reason: "embedding batch output shape overflowed u64".into(),
        })?;
    if output_values > descriptor.resources.maximum_output_values {
        return invalid("embedding batch exceeds the backend output value limit");
    }

    let vectors = backend.embed_batch(requests)?;
    if vectors.len() != requests.len() {
        return invalid("embedding backend returned an unexpected batch shape");
    }
    for vector in &vectors {
        validate_embedding_value(vector, &descriptor.model)?;
    }
    Ok(vectors)
}

fn validate_embedding_value(value: &VectorValue, model: &EmbeddingModelSpec) -> Result<()> {
    let VectorValue::Dense { values } = value else {
        return invalid("embedding backend must return one dense vector per input");
    };
    if values.len() != model.dimensions as usize || values.iter().any(|value| !value.is_finite()) {
        return invalid("embedding backend output differs from the declared dimensions");
    }
    if model.normalization == VectorNormalization::UnitL2 {
        let norm = values
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>()
            .sqrt();
        if !norm.is_finite() || (norm - 1.0).abs() > 1e-4 {
            return invalid("embedding backend output differs from the declared normalization");
        }
    }
    Ok(())
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
        request.validate()?;
        let mut values =
            dispatch_embedding_batch(backend, job.network_policy, std::slice::from_ref(&request))?;
        let value = values
            .pop()
            .expect("one validated embedding request returns one vector");

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
            collection: None,
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
        let model_identity = serde_json::to_vec(&("rrflow-feature-hash-v1", dimensions, seed))
            .map_err(|error| Error::InvalidRuntime {
                reason: format!("feature-hash model identity cannot be encoded: {error}"),
            })?;
        Ok(Self {
            descriptor: {
                let model = EmbeddingModelSpec {
                    provider: "rrflow".into(),
                    model: "feature-hash".into(),
                    revision: "v1".into(),
                    model_digest: digest::sha256_hex(&model_identity),
                    modality: EmbeddingModality::Text,
                    dimensions,
                    normalization: VectorNormalization::UnitL2,
                    maximum_input_bytes: 4 * 1024 * 1024,
                };
                EmbeddingBackendDescriptor {
                    id: "rrflow:feature-hash:cpu:v1".into(),
                    resources: EmbeddingResourceLimits::for_model(&model, 256)?,
                    model,
                    execution: ExecutionTarget::Cpu,
                    network: NetworkRequirement::None,
                    trust: InferenceTrustBoundary::LocalOffline,
                    deterministic: true,
                }
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
            let mut identity = b"rrflow-feature-hash-token-v1\0".to_vec();
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
