//! Capability-explicit accelerator boundary for deterministic artifact builds.
//!
//! Accelerators produce bytes, never truth. The coordinator opens and verifies
//! those bytes, compares them with the deterministic CPU artifact, and only
//! then returns an object that can be published. GPU libraries therefore stay
//! optional build-time adapters; query nodes consume the same portable format.

use crate::contract::invalid;
use crate::{
    CompactDenseSegment, HnswConfig, HnswIndex, SearchMode, SearchRequest, VectorCandidate,
    VectorQuery, VectorSegmentConfig, COMPACT_DENSE_FORMAT_VERSION, HNSW_FORMAT_VERSION,
};
use rrd_core::{ReadStamp, Result, VectorValue};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MAX_SEMANTIC_PROBES: usize = 16;

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
        self.validate_for(COMPACT_DENSE_FORMAT_VERSION)
    }

    pub fn validate_for(&self, required_format_version: u16) -> Result<()> {
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
            .contains(&required_format_version)
        {
            return invalid("dense build backend does not support the required artifact format");
        }
        if !self.deterministic {
            return invalid("exact dense build backend must declare deterministic output");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildDifferentialStatus {
    NotRun,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorBuildResourceEvidence {
    pub input_vectors: u64,
    pub dimensions: u64,
    pub input_values: u64,
    pub cpu_artifact_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accelerator_artifact_bytes: Option<u64>,
    pub semantic_probe_queries: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HnswBuildEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_backend_id: Option<String>,
    pub selected_backend: DenseBuildBackend,
    pub used_fallback: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub byte_differential: BuildDifferentialStatus,
    pub semantic_differential: BuildDifferentialStatus,
    pub resources: VectorBuildResourceEvidence,
}

impl HnswBuildEvidence {
    pub fn validate(&self) -> Result<()> {
        self.selected_backend.validate_for(HNSW_FORMAT_VERSION)?;
        if self.resources.dimensions == 0
            || self.resources.input_values
                != self
                    .resources
                    .input_vectors
                    .checked_mul(self.resources.dimensions)
                    .ok_or_else(|| rrd_core::Error::InvalidRuntime {
                        reason: "HNSW build resource values overflowed".into(),
                    })?
            || self.resources.cpu_artifact_bytes == 0
        {
            return invalid("HNSW build resource evidence is incoherent");
        }
        match (&self.selected_backend.target, self.used_fallback) {
            (AcceleratorTarget::Cpu, true) if self.fallback_reason.is_some() => {}
            (AcceleratorTarget::Cpu, false)
                if self.requested_backend_id.is_none() && self.fallback_reason.is_none() => {}
            (AcceleratorTarget::Gpu { .. }, false)
                if self.requested_backend_id.as_deref()
                    == Some(self.selected_backend.id.as_str())
                    && self.fallback_reason.is_none()
                    && self.byte_differential == BuildDifferentialStatus::Passed
                    && self.semantic_differential == BuildDifferentialStatus::Passed
                    && self.resources.accelerator_artifact_bytes.is_some() => {}
            _ => return invalid("HNSW build backend and fallback evidence are incoherent"),
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

/// Optional physical HNSW construction adapter. Returned bytes remain
/// untrusted until the coordinator decodes them and proves CPU parity.
pub trait HnswArtifactBuilder {
    fn descriptor(&self) -> &DenseBuildBackend;

    fn build(
        &mut self,
        config: &HnswConfig,
        generation: u64,
        source_cursor: u64,
        candidates: &[VectorCandidate],
    ) -> Result<Vec<u8>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HnswBuildPolicy {
    CpuOnly,
    Prefer {
        backend_id: String,
        allow_cpu_fallback: bool,
    },
    Require {
        backend_id: String,
    },
}

impl HnswBuildPolicy {
    fn backend_id(&self) -> Option<&str> {
        match self {
            Self::CpuOnly => None,
            Self::Prefer { backend_id, .. } | Self::Require { backend_id } => Some(backend_id),
        }
    }

    fn allows_fallback(&self) -> bool {
        matches!(
            self,
            Self::Prefer {
                allow_cpu_fallback: true,
                ..
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HnswBuildOutcome {
    pub artifact: HnswIndex,
    pub evidence: HnswBuildEvidence,
}

#[derive(Default)]
pub struct HnswAcceleratorRegistry {
    revision: u64,
    builders: BTreeMap<String, Box<dyn HnswArtifactBuilder + Send>>,
}

impl HnswAcceleratorRegistry {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn descriptors(&self) -> Vec<DenseBuildBackend> {
        self.builders
            .values()
            .map(|builder| builder.descriptor().clone())
            .collect()
    }

    pub fn install(&mut self, builder: Box<dyn HnswArtifactBuilder + Send>) -> Result<u64> {
        builder.descriptor().validate_for(HNSW_FORMAT_VERSION)?;
        if !matches!(builder.descriptor().target, AcceleratorTarget::Gpu { .. }) {
            return invalid("HNSW accelerator registry requires a GPU target");
        }
        let id = builder.descriptor().id.clone();
        if self.builders.contains_key(&id) {
            return invalid("HNSW accelerator backend id is already installed");
        }
        self.revision =
            self.revision
                .checked_add(1)
                .ok_or_else(|| rrd_core::Error::InvalidRuntime {
                    reason: "HNSW accelerator registry revision overflowed".into(),
                })?;
        self.builders.insert(id, builder);
        Ok(self.revision)
    }

    pub fn build(
        &mut self,
        config: HnswConfig,
        generation: u64,
        source_cursor: u64,
        candidates: Vec<VectorCandidate>,
        policy: HnswBuildPolicy,
    ) -> Result<HnswBuildOutcome> {
        let Some(backend_id) = policy.backend_id().map(str::to_owned) else {
            return build_hnsw_artifact(
                config,
                generation,
                source_cursor,
                candidates,
                policy,
                None,
            );
        };
        let accelerator = self.builders.get_mut(&backend_id);
        build_hnsw_artifact(
            config,
            generation,
            source_cursor,
            candidates,
            policy,
            accelerator.map(|builder| builder.as_mut() as &mut dyn HnswArtifactBuilder),
        )
    }
}

pub fn build_hnsw_artifact(
    config: HnswConfig,
    generation: u64,
    source_cursor: u64,
    candidates: Vec<VectorCandidate>,
    policy: HnswBuildPolicy,
    accelerator: Option<&mut dyn HnswArtifactBuilder>,
) -> Result<HnswBuildOutcome> {
    let cpu = HnswIndex::build(
        config.clone(),
        generation,
        source_cursor,
        candidates.clone(),
    )?;
    let cpu_backend = hnsw_cpu_backend();
    let input_vectors =
        u64::try_from(candidates.len()).map_err(|_| rrd_core::Error::InvalidRuntime {
            reason: "HNSW build vector count exceeds u64".into(),
        })?;
    let dimensions =
        u64::try_from(config.dimensions).map_err(|_| rrd_core::Error::InvalidRuntime {
            reason: "HNSW build dimensions exceed u64".into(),
        })?;
    let cpu_artifact_bytes =
        u64::try_from(cpu.as_bytes().len()).map_err(|_| rrd_core::Error::InvalidRuntime {
            reason: "HNSW CPU artifact bytes exceed u64".into(),
        })?;
    let base_resources = VectorBuildResourceEvidence {
        input_vectors,
        dimensions,
        input_values: input_vectors.checked_mul(dimensions).ok_or_else(|| {
            rrd_core::Error::InvalidRuntime {
                reason: "HNSW input value count overflowed".into(),
            }
        })?,
        cpu_artifact_bytes,
        accelerator_artifact_bytes: None,
        semantic_probe_queries: 0,
    };
    if policy == HnswBuildPolicy::CpuOnly {
        let evidence = HnswBuildEvidence {
            requested_backend_id: None,
            selected_backend: cpu_backend,
            used_fallback: false,
            fallback_reason: None,
            byte_differential: BuildDifferentialStatus::NotRun,
            semantic_differential: BuildDifferentialStatus::NotRun,
            resources: base_resources,
        };
        evidence.validate()?;
        return Ok(HnswBuildOutcome {
            artifact: cpu,
            evidence,
        });
    }

    let requested_backend_id = policy
        .backend_id()
        .expect("non-CPU policy has backend")
        .to_owned();
    let Some(accelerator) = accelerator else {
        return hnsw_fallback_or_error(
            policy,
            cpu,
            cpu_backend,
            requested_backend_id,
            "accelerator unavailable".into(),
            BuildDifferentialStatus::NotRun,
            BuildDifferentialStatus::NotRun,
            base_resources,
        );
    };
    if accelerator.descriptor().id != requested_backend_id {
        return hnsw_fallback_or_error(
            policy,
            cpu,
            cpu_backend,
            requested_backend_id,
            "installed accelerator identity differs from the requested backend".into(),
            BuildDifferentialStatus::NotRun,
            BuildDifferentialStatus::NotRun,
            base_resources,
        );
    }
    if let Err(error) = accelerator.descriptor().validate_for(HNSW_FORMAT_VERSION) {
        return hnsw_fallback_or_error(
            policy,
            cpu,
            cpu_backend,
            requested_backend_id,
            error.to_string(),
            BuildDifferentialStatus::NotRun,
            BuildDifferentialStatus::NotRun,
            base_resources,
        );
    }
    if !matches!(
        accelerator.descriptor().target,
        AcceleratorTarget::Gpu { .. }
    ) {
        return hnsw_fallback_or_error(
            policy,
            cpu,
            cpu_backend,
            requested_backend_id,
            "accelerator adapter did not declare a GPU target".into(),
            BuildDifferentialStatus::NotRun,
            BuildDifferentialStatus::NotRun,
            base_resources,
        );
    }
    let bytes = match accelerator.build(&config, generation, source_cursor, &candidates) {
        Ok(bytes) => bytes,
        Err(error) => {
            return hnsw_fallback_or_error(
                policy,
                cpu,
                cpu_backend,
                requested_backend_id,
                error.to_string(),
                BuildDifferentialStatus::NotRun,
                BuildDifferentialStatus::NotRun,
                base_resources,
            )
        }
    };
    let mut resources = base_resources;
    resources.accelerator_artifact_bytes =
        Some(
            u64::try_from(bytes.len()).map_err(|_| rrd_core::Error::InvalidRuntime {
                reason: "HNSW accelerator artifact bytes exceed u64".into(),
            })?,
        );
    let accelerated = match HnswIndex::from_bytes(&bytes) {
        Ok(artifact) => artifact,
        Err(error) => {
            return hnsw_fallback_or_error(
                policy,
                cpu,
                cpu_backend,
                requested_backend_id,
                error.to_string(),
                BuildDifferentialStatus::Failed,
                BuildDifferentialStatus::NotRun,
                resources,
            )
        }
    };
    if accelerated.as_bytes() != cpu.as_bytes() || accelerated.descriptor() != cpu.descriptor() {
        return hnsw_fallback_or_error(
            policy,
            cpu,
            cpu_backend,
            requested_backend_id,
            "accelerator HNSW artifact failed deterministic CPU byte-parity gate".into(),
            BuildDifferentialStatus::Failed,
            BuildDifferentialStatus::NotRun,
            resources,
        );
    }
    let semantic_probe_queries = match semantic_probe_differential(
        &cpu,
        &accelerated,
        &config,
        source_cursor,
        &candidates,
    ) {
        Ok(probes) => probes,
        Err(error) => {
            return hnsw_fallback_or_error(
                policy,
                cpu,
                cpu_backend,
                requested_backend_id,
                error.to_string(),
                BuildDifferentialStatus::Passed,
                BuildDifferentialStatus::Failed,
                resources,
            )
        }
    };
    resources.semantic_probe_queries = semantic_probe_queries;
    let evidence = HnswBuildEvidence {
        requested_backend_id: Some(requested_backend_id),
        selected_backend: accelerator.descriptor().clone(),
        used_fallback: false,
        fallback_reason: None,
        byte_differential: BuildDifferentialStatus::Passed,
        semantic_differential: BuildDifferentialStatus::Passed,
        resources,
    };
    evidence.validate()?;
    Ok(HnswBuildOutcome {
        artifact: accelerated,
        evidence,
    })
}

/// Describes an already-built CPU HNSW artifact, including incremental
/// generations that intentionally do not pass through the full-build
/// accelerator coordinator.
pub fn cpu_hnsw_build_evidence(artifact: &HnswIndex) -> Result<HnswBuildEvidence> {
    let descriptor = artifact.descriptor();
    let input_vectors =
        u64::try_from(descriptor.nodes).map_err(|_| rrd_core::Error::InvalidRuntime {
            reason: "HNSW build vector count exceeds u64".into(),
        })?;
    let dimensions =
        u64::try_from(descriptor.dimensions).map_err(|_| rrd_core::Error::InvalidRuntime {
            reason: "HNSW build dimensions exceed u64".into(),
        })?;
    let evidence = HnswBuildEvidence {
        requested_backend_id: None,
        selected_backend: hnsw_cpu_backend(),
        used_fallback: false,
        fallback_reason: None,
        byte_differential: BuildDifferentialStatus::NotRun,
        semantic_differential: BuildDifferentialStatus::NotRun,
        resources: VectorBuildResourceEvidence {
            input_vectors,
            dimensions,
            input_values: input_vectors.checked_mul(dimensions).ok_or_else(|| {
                rrd_core::Error::InvalidRuntime {
                    reason: "HNSW input value count overflowed".into(),
                }
            })?,
            cpu_artifact_bytes: u64::try_from(artifact.as_bytes().len()).map_err(|_| {
                rrd_core::Error::InvalidRuntime {
                    reason: "HNSW CPU artifact bytes exceed u64".into(),
                }
            })?,
            accelerator_artifact_bytes: None,
            semantic_probe_queries: 0,
        },
    };
    evidence.validate()?;
    Ok(evidence)
}

fn hnsw_cpu_backend() -> DenseBuildBackend {
    DenseBuildBackend {
        id: "rrflow.cpu.hnsw.v2".into(),
        target: AcceleratorTarget::Cpu,
        deterministic: true,
        supported_format_versions: BTreeSet::from([HNSW_FORMAT_VERSION]),
    }
}

#[allow(clippy::too_many_arguments)]
fn hnsw_fallback_or_error(
    policy: HnswBuildPolicy,
    cpu: HnswIndex,
    cpu_backend: DenseBuildBackend,
    requested_backend_id: String,
    reason: String,
    byte_differential: BuildDifferentialStatus,
    semantic_differential: BuildDifferentialStatus,
    resources: VectorBuildResourceEvidence,
) -> Result<HnswBuildOutcome> {
    if !policy.allows_fallback() {
        return invalid(reason);
    }
    let evidence = HnswBuildEvidence {
        requested_backend_id: Some(requested_backend_id),
        selected_backend: cpu_backend,
        used_fallback: true,
        fallback_reason: Some(reason),
        byte_differential,
        semantic_differential,
        resources,
    };
    evidence.validate()?;
    Ok(HnswBuildOutcome {
        artifact: cpu,
        evidence,
    })
}

fn semantic_probe_differential(
    cpu: &HnswIndex,
    accelerated: &HnswIndex,
    config: &HnswConfig,
    source_cursor: u64,
    candidates: &[VectorCandidate],
) -> Result<u64> {
    let read = ReadStamp::new(
        config.scope.clone(),
        None,
        0,
        source_cursor,
        Some("ab".repeat(32)),
    )?;
    let candidate_count = candidates.len().max(1);
    let top_k = candidate_count.min(10);
    let ef_search = candidate_count.max(top_k);
    let mut probes = 0_u64;
    for candidate in candidates.iter().take(MAX_SEMANTIC_PROBES) {
        let VectorValue::Dense { values } = &candidate.vector.value else {
            continue;
        };
        let request = SearchRequest {
            scope: config.scope.clone(),
            read: read.clone(),
            valid_at: candidate.vector.valid_from,
            field: config.field.clone(),
            query: VectorQuery::Dense {
                values: values.clone(),
            },
            metric: config.metric,
            embedding_model: config.embedding_model.clone(),
            top_k,
            mode: SearchMode::RequireApproximate {
                exact_rerank: top_k,
            },
            filter: None,
        };
        if cpu.search(&request, ef_search)? != accelerated.search(&request, ef_search)? {
            return invalid("accelerator HNSW artifact failed CPU semantic differential");
        }
        probes = probes.saturating_add(1);
    }
    Ok(probes)
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
        id: "rrflow.cpu.compact-dense.v1".into(),
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
