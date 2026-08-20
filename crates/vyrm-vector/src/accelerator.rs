//! Capability-explicit accelerator boundary for deterministic artifact builds.
//!
//! Accelerators produce bytes, never truth. The coordinator opens and verifies
//! those bytes, compares them with the deterministic CPU artifact, and only
//! then returns an object that can be published. GPU libraries therefore stay
//! optional build-time adapters; query nodes consume the same portable format.

use crate::contract::invalid;
use crate::{
    CompactDenseSegment, VectorCandidate, VectorSegmentConfig, COMPACT_DENSE_FORMAT_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use vyrm_core::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AcceleratorTarget {
    Cpu,
    Gpu { platform: String, device: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DenseBuildBackend {
    pub id: String,
    pub target: AcceleratorTarget,
    pub deterministic: bool,
    pub supported_format_versions: BTreeSet<u16>,
}

impl DenseBuildBackend {
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() || self.id.as_bytes().contains(&0) {
            return invalid("dense build backend id must be non-empty and contain no NUL bytes");
        }
        if let AcceleratorTarget::Gpu { platform, device } = &self.target {
            if platform.trim().is_empty()
                || device.trim().is_empty()
                || platform.as_bytes().contains(&0)
                || device.as_bytes().contains(&0)
            {
                return invalid("GPU build target must name a platform and device");
            }
        }
        if !self
            .supported_format_versions
            .contains(&COMPACT_DENSE_FORMAT_VERSION)
        {
            return invalid("dense build backend does not support the required artifact format");
        }
        if !self.deterministic {
            return invalid("exact dense build backend must declare deterministic output");
        }
        Ok(())
    }
}

/// Adapter implemented by optional CUDA, ROCm, Metal, Vulkan, or other build
/// providers. Returned bytes are untrusted until the coordinator verifies them.
pub trait DenseArtifactBuilder {
    fn descriptor(&self) -> &DenseBuildBackend;

    fn build(
        &mut self,
        config: &VectorSegmentConfig,
        generation: u64,
        source_cursor: u64,
        candidates: &[VectorCandidate],
    ) -> Result<Vec<u8>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceleratedBuildPolicy {
    CpuOnly,
    Prefer { allow_cpu_fallback: bool },
    Require,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DenseBuildOutcome {
    pub artifact: CompactDenseSegment,
    pub backend: DenseBuildBackend,
    pub used_fallback: bool,
    pub fallback_reason: Option<String>,
}

/// Builds the CPU oracle first, then admits accelerator output only when its
/// verified portable bytes are identical. This is deliberately strict for the
/// exact artifact format: non-deterministic ANN builders require their own
/// recall/equivalence policy rather than weakening this gate.
pub fn build_dense_artifact(
    config: VectorSegmentConfig,
    generation: u64,
    source_cursor: u64,
    candidates: impl IntoIterator<Item = VectorCandidate>,
    policy: AcceleratedBuildPolicy,
    accelerator: Option<&mut dyn DenseArtifactBuilder>,
) -> Result<DenseBuildOutcome> {
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    let cpu = CompactDenseSegment::build(
        config.clone(),
        generation,
        source_cursor,
        candidates.clone(),
    )?;
    let cpu_backend = DenseBuildBackend {
        id: "vyrm.cpu.compact-dense.v1".into(),
        target: AcceleratorTarget::Cpu,
        deterministic: true,
        supported_format_versions: BTreeSet::from([COMPACT_DENSE_FORMAT_VERSION]),
    };
    if policy == AcceleratedBuildPolicy::CpuOnly {
        return Ok(DenseBuildOutcome {
            artifact: cpu,
            backend: cpu_backend,
            used_fallback: false,
            fallback_reason: None,
        });
    }

    let Some(accelerator) = accelerator else {
        return match policy {
            AcceleratedBuildPolicy::Prefer {
                allow_cpu_fallback: true,
            } => Ok(DenseBuildOutcome {
                artifact: cpu,
                backend: cpu_backend,
                used_fallback: true,
                fallback_reason: Some("accelerator unavailable".into()),
            }),
            _ => invalid("accelerator is required by dense artifact build policy"),
        };
    };
    if let Err(error) = accelerator.descriptor().validate() {
        return fallback_or_error(policy, cpu, cpu_backend, error.to_string());
    }
    if !matches!(
        accelerator.descriptor().target,
        AcceleratorTarget::Gpu { .. }
    ) {
        return fallback_or_error(
            policy,
            cpu,
            cpu_backend,
            "accelerator adapter did not declare a GPU target".into(),
        );
    }
    let bytes = match accelerator.build(&config, generation, source_cursor, &candidates) {
        Ok(bytes) => bytes,
        Err(error) => return fallback_or_error(policy, cpu, cpu_backend, error.to_string()),
    };
    let accelerated = match CompactDenseSegment::from_bytes(&bytes) {
        Ok(artifact) => artifact,
        Err(error) => return fallback_or_error(policy, cpu, cpu_backend, error.to_string()),
    };
    if accelerated.as_bytes() != cpu.as_bytes() || accelerated.descriptor() != cpu.descriptor() {
        return fallback_or_error(
            policy,
            cpu,
            cpu_backend,
            "accelerator artifact failed deterministic CPU byte-parity gate".into(),
        );
    }
    Ok(DenseBuildOutcome {
        artifact: accelerated,
        backend: accelerator.descriptor().clone(),
        used_fallback: false,
        fallback_reason: None,
    })
}

fn fallback_or_error(
    policy: AcceleratedBuildPolicy,
    cpu: CompactDenseSegment,
    cpu_backend: DenseBuildBackend,
    reason: String,
) -> Result<DenseBuildOutcome> {
    match policy {
        AcceleratedBuildPolicy::Prefer {
            allow_cpu_fallback: true,
        } => Ok(DenseBuildOutcome {
            artifact: cpu,
            backend: cpu_backend,
            used_fallback: true,
            fallback_reason: Some(reason),
        }),
        AcceleratedBuildPolicy::CpuOnly => unreachable!("CPU-only returns before acceleration"),
        AcceleratedBuildPolicy::Prefer {
            allow_cpu_fallback: false,
        }
        | AcceleratedBuildPolicy::Require => invalid(reason),
    }
}
