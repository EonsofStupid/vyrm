//! Optional local ONNX adapter. The `fastembed` dependency is compiled without
//! its Hugging Face or TLS features: callers provide every model/tokenizer byte.

use crate::{
    invalid, validate_digest, validate_text, EmbeddingBackend, EmbeddingBackendDescriptor,
    EmbeddingModality, EmbeddingModelSpec, EmbeddingRequest, ExecutionTarget, NetworkRequirement,
};
use fastembed::{InitOptionsUserDefined, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel};
use vyrm_core::{digest, Result, VectorNormalization, VectorValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FastEmbedLocalIdentity {
    pub provider: String,
    pub model: String,
    pub revision: String,
    pub dimensions: u32,
    pub normalization: VectorNormalization,
    pub maximum_input_bytes: u64,
    pub execution: ExecutionTarget,
    pub deterministic: bool,
    /// Digest of pooling, quantization, output-key, and preprocessing choices.
    pub runtime_config_digest: String,
}

impl FastEmbedLocalIdentity {
    fn validate(&self) -> Result<()> {
        validate_text("FastEmbed provider", &self.provider)?;
        validate_text("FastEmbed model", &self.model)?;
        validate_text("FastEmbed revision", &self.revision)?;
        validate_digest(
            "FastEmbed runtime configuration",
            &self.runtime_config_digest,
        )?;
        if self.dimensions == 0 || self.dimensions > 1_048_576 {
            return invalid("FastEmbed dimensions must be in 1..=1048576");
        }
        if self.maximum_input_bytes == 0 || self.maximum_input_bytes > 64 * 1024 * 1024 {
            return invalid("FastEmbed maximum input bytes must be in 1..=67108864");
        }
        if matches!(self.execution, ExecutionTarget::Remote { .. }) {
            return invalid("local FastEmbed execution cannot declare a remote target");
        }
        Ok(())
    }
}

pub struct FastEmbedLocalBackend {
    descriptor: EmbeddingBackendDescriptor,
    model: TextEmbedding,
}

impl FastEmbedLocalBackend {
    /// Initializes FastEmbed exclusively from caller-supplied bytes. This API
    /// cannot select or download a hub model because `hf-hub` is absent.
    pub fn try_from_user_defined(
        model: UserDefinedEmbeddingModel,
        options: InitOptionsUserDefined,
        identity: FastEmbedLocalIdentity,
    ) -> Result<Self> {
        identity.validate()?;
        let model_digest = fastembed_model_digest(&model, &identity.runtime_config_digest)?;
        let model = TextEmbedding::try_new_from_user_defined(model, options).map_err(|error| {
            vyrm_core::Error::InvalidRuntime {
                reason: format!("cannot initialize local FastEmbed model: {error}"),
            }
        })?;
        let descriptor = EmbeddingBackendDescriptor {
            id: "vyrm:fastembed:local:v1".into(),
            model: EmbeddingModelSpec {
                provider: identity.provider,
                model: identity.model,
                revision: identity.revision,
                model_digest,
                modality: EmbeddingModality::Text,
                dimensions: identity.dimensions,
                normalization: identity.normalization,
                maximum_input_bytes: identity.maximum_input_bytes,
            },
            execution: identity.execution,
            network: NetworkRequirement::None,
            deterministic: identity.deterministic,
        };
        descriptor.validate()?;
        Ok(Self { descriptor, model })
    }
}

impl EmbeddingBackend for FastEmbedLocalBackend {
    fn descriptor(&self) -> &EmbeddingBackendDescriptor {
        &self.descriptor
    }

    fn embed(&mut self, request: &EmbeddingRequest) -> Result<VectorValue> {
        if request.bytes.len() > self.descriptor.model.maximum_input_bytes as usize {
            return invalid("FastEmbed input exceeds the declared model byte limit");
        }
        if !request.media_type.starts_with("text/") && request.media_type != "application/json" {
            return invalid("local FastEmbed backend accepts text or JSON only");
        }
        let text =
            std::str::from_utf8(&request.bytes).map_err(|_| vyrm_core::Error::InvalidRuntime {
                reason: "local FastEmbed input must be UTF-8".into(),
            })?;
        let mut output = self.model.embed([text], Some(1)).map_err(|error| {
            vyrm_core::Error::InvalidRuntime {
                reason: format!("local FastEmbed inference failed: {error}"),
            }
        })?;
        if output.len() != 1 {
            return invalid("local FastEmbed returned an unexpected batch shape");
        }
        Ok(VectorValue::Dense {
            values: output.remove(0),
        })
    }
}

/// Content identity for all inference-relevant local model material. Length
/// framing prevents concatenation ambiguity; external initializers are sorted
/// by filename before hashing.
pub fn fastembed_model_digest(
    model: &UserDefinedEmbeddingModel,
    runtime_config_digest: &str,
) -> Result<String> {
    validate_digest("FastEmbed runtime configuration", runtime_config_digest)?;
    let mut bytes = b"vyrm-fastembed-local-model-v1\0".to_vec();
    append(&mut bytes, runtime_config_digest.as_bytes())?;
    append(&mut bytes, &model.onnx_file)?;
    append_tokenizer(&mut bytes, &model.tokenizer_files)?;
    let mut initializers = model.external_initializers.iter().collect::<Vec<_>>();
    initializers.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    for initializer in initializers {
        append(&mut bytes, initializer.file_name.as_bytes())?;
        append(&mut bytes, &initializer.buffer)?;
    }
    Ok(digest::sha256_hex(&bytes))
}

fn append_tokenizer(bytes: &mut Vec<u8>, tokenizer: &TokenizerFiles) -> Result<()> {
    append(bytes, &tokenizer.tokenizer_file)?;
    append(bytes, &tokenizer.config_file)?;
    append(bytes, &tokenizer.special_tokens_map_file)?;
    append(bytes, &tokenizer.tokenizer_config_file)
}

fn append(output: &mut Vec<u8>, value: &[u8]) -> Result<()> {
    let length = u64::try_from(value.len()).map_err(|_| vyrm_core::Error::InvalidRuntime {
        reason: "FastEmbed model component length exceeds u64".into(),
    })?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> UserDefinedEmbeddingModel {
        UserDefinedEmbeddingModel::new(
            b"onnx".to_vec(),
            TokenizerFiles {
                tokenizer_file: b"tokenizer".to_vec(),
                config_file: b"config".to_vec(),
                special_tokens_map_file: b"special".to_vec(),
                tokenizer_config_file: b"tokenizer-config".to_vec(),
            },
        )
        .with_external_initializer("weights.bin".into(), b"weights".to_vec())
    }

    #[test]
    fn model_digest_covers_every_local_component_and_runtime_configuration() {
        let config = "11".repeat(32);
        let baseline = fastembed_model_digest(&model(), &config).unwrap();
        assert_eq!(baseline, fastembed_model_digest(&model(), &config).unwrap());
        let mut changed = model();
        changed.onnx_file.push(1);
        assert_ne!(baseline, fastembed_model_digest(&changed, &config).unwrap());
        assert_ne!(
            baseline,
            fastembed_model_digest(&model(), &"22".repeat(32)).unwrap()
        );
    }
}
