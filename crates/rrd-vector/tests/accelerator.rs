#![cfg(feature = "accelerator")]

use rrd_core::{ProjectionId, RuntimeProperties, RuntimeRef, RuntimeVector, ScopeId, VectorValue};
use rrd_vector::{
    build_dense_artifact, AcceleratedBuildPolicy, AcceleratorTarget, BuildDifferentialStatus,
    CompactDenseSegment, DenseArtifactBuilder, DenseBuildBackend, HnswAcceleratorRegistry,
    HnswArtifactBuilder, HnswBuildPolicy, HnswConfig, HnswIndex, ScoreMetric, VectorCandidate,
    VectorSegmentConfig, COMPACT_DENSE_FORMAT_VERSION, HNSW_FORMAT_VERSION,
};
use std::collections::BTreeSet;

struct FakeGpu {
    descriptor: DenseBuildBackend,
    behavior: Behavior,
}

enum Behavior {
    Correct,
    Corrupt,
    WrongGeneration,
    Fail,
}

impl FakeGpu {
    fn new(behavior: Behavior) -> Self {
        Self {
            descriptor: DenseBuildBackend {
                id: "test.gpu".into(),
                target: AcceleratorTarget::Gpu {
                    platform: "test".into(),
                    device: "0".into(),
                },
                deterministic: true,
                supported_format_versions: BTreeSet::from([COMPACT_DENSE_FORMAT_VERSION]),
            },
            behavior,
        }
    }
}

impl DenseArtifactBuilder for FakeGpu {
    fn descriptor(&self) -> &DenseBuildBackend {
        &self.descriptor
    }

    fn build(
        &mut self,
        config: &VectorSegmentConfig,
        generation: u64,
        source_cursor: u64,
        candidates: &[VectorCandidate],
    ) -> rrd_core::Result<Vec<u8>> {
        if matches!(self.behavior, Behavior::Fail) {
            return Err(rrd_core::Error::InvalidRuntime {
                reason: "simulated GPU failure".into(),
            });
        }
        let generation = if matches!(self.behavior, Behavior::WrongGeneration) {
            generation + 1
        } else {
            generation
        };
        let artifact = CompactDenseSegment::build(
            config.clone(),
            generation,
            source_cursor,
            candidates.to_vec(),
        )?;
        let mut bytes = artifact.as_bytes().to_vec();
        if matches!(self.behavior, Behavior::Corrupt) {
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
        }
        Ok(bytes)
    }
}

fn fixture() -> (VectorSegmentConfig, Vec<VectorCandidate>) {
    let scope = ScopeId::new("instance:accelerator").unwrap();
    let config = VectorSegmentConfig {
        id: ProjectionId::new("vector:accelerated:body").unwrap(),
        scope: scope.clone(),
        field: "body".into(),
        dimensions: 2,
        metric: ScoreMetric::Cosine,
        embedding_model: None,
        filter_properties: BTreeSet::new(),
    };
    let candidates = [([1.0, 0.0], "a"), ([0.0, 1.0], "b")]
        .into_iter()
        .enumerate()
        .map(|(index, (values, id))| VectorCandidate {
            scope: scope.clone(),
            source_cursor: index as u64 + 1,
            vector: RuntimeVector {
                collection: None,
                reference: RuntimeRef::new("embedding", id).unwrap(),
                subject: RuntimeRef::new("document", id).unwrap(),
                field: "body".into(),
                valid_from: 1,
                valid_to: None,
                value: VectorValue::Dense {
                    values: values.to_vec(),
                },
                provenance: None,
                properties: RuntimeProperties::new(),
            },
        })
        .collect();
    (config, candidates)
}

#[test]
fn verified_gpu_bytes_are_admitted_with_a_cpu_loadable_artifact() {
    let (config, candidates) = fixture();
    let mut gpu = FakeGpu::new(Behavior::Correct);
    let outcome = build_dense_artifact(
        config,
        1,
        2,
        candidates,
        AcceleratedBuildPolicy::Require,
        Some(&mut gpu),
    )
    .unwrap();
    assert!(!outcome.used_fallback);
    assert_eq!(outcome.backend.id, "test.gpu");
    CompactDenseSegment::from_bytes(outcome.artifact.as_bytes()).unwrap();
}

#[test]
fn corrupt_wrong_generation_and_failed_gpu_outputs_never_publish() {
    for behavior in [Behavior::Corrupt, Behavior::WrongGeneration, Behavior::Fail] {
        let (config, candidates) = fixture();
        let mut gpu = FakeGpu::new(behavior);
        assert!(build_dense_artifact(
            config,
            1,
            2,
            candidates,
            AcceleratedBuildPolicy::Require,
            Some(&mut gpu),
        )
        .is_err());
    }
}

#[test]
fn fallback_is_explicit_and_policy_controlled() {
    let (config, candidates) = fixture();
    let outcome = build_dense_artifact(
        config.clone(),
        1,
        2,
        candidates.clone(),
        AcceleratedBuildPolicy::Prefer {
            allow_cpu_fallback: true,
        },
        None,
    )
    .unwrap();
    assert!(outcome.used_fallback);
    assert_eq!(outcome.backend.target, AcceleratorTarget::Cpu);
    assert_eq!(
        outcome.fallback_reason.as_deref(),
        Some("accelerator unavailable")
    );

    assert!(build_dense_artifact(
        config,
        1,
        2,
        candidates,
        AcceleratedBuildPolicy::Prefer {
            allow_cpu_fallback: false,
        },
        None,
    )
    .is_err());
}

struct FakeHnswGpu {
    descriptor: DenseBuildBackend,
    behavior: Behavior,
}

impl FakeHnswGpu {
    fn new(id: &str, behavior: Behavior) -> Self {
        Self {
            descriptor: DenseBuildBackend {
                id: id.into(),
                target: AcceleratorTarget::Gpu {
                    platform: "test-vulkan".into(),
                    device: "device-0".into(),
                },
                deterministic: true,
                supported_format_versions: BTreeSet::from([HNSW_FORMAT_VERSION]),
            },
            behavior,
        }
    }
}

impl HnswArtifactBuilder for FakeHnswGpu {
    fn descriptor(&self) -> &DenseBuildBackend {
        &self.descriptor
    }

    fn build(
        &mut self,
        config: &HnswConfig,
        generation: u64,
        source_cursor: u64,
        candidates: &[VectorCandidate],
    ) -> rrd_core::Result<Vec<u8>> {
        if matches!(self.behavior, Behavior::Fail) {
            return Err(rrd_core::Error::InvalidRuntime {
                reason: "simulated HNSW device loss".into(),
            });
        }
        let generation = if matches!(self.behavior, Behavior::WrongGeneration) {
            generation + 1
        } else {
            generation
        };
        let artifact = HnswIndex::build(
            config.clone(),
            generation,
            source_cursor,
            candidates.to_vec(),
        )?;
        let mut bytes = artifact.as_bytes().to_vec();
        if matches!(self.behavior, Behavior::Corrupt) {
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
        }
        Ok(bytes)
    }
}

fn hnsw_fixture() -> (HnswConfig, Vec<VectorCandidate>) {
    let (segment, candidates) = fixture();
    (
        HnswConfig {
            id: segment.id,
            scope: segment.scope,
            field: segment.field,
            dimensions: segment.dimensions,
            metric: segment.metric,
            embedding_model: None,
            m: 2,
            ef_construction: 8,
            max_level: 4,
            seed: 7,
            filter_properties: BTreeSet::new(),
        },
        candidates,
    )
}

#[test]
fn hnsw_registry_admits_only_cpu_identical_and_semantically_probed_gpu_bytes() {
    let (config, candidates) = hnsw_fixture();
    let mut registry = HnswAcceleratorRegistry::default();
    assert_eq!(
        registry
            .install(Box::new(FakeHnswGpu::new(
                "test.hnsw.gpu",
                Behavior::Correct
            )))
            .unwrap(),
        1
    );
    let outcome = registry
        .build(
            config,
            1,
            2,
            candidates,
            HnswBuildPolicy::Require {
                backend_id: "test.hnsw.gpu".into(),
            },
        )
        .unwrap();
    assert!(!outcome.evidence.used_fallback);
    assert_eq!(
        outcome.evidence.byte_differential,
        BuildDifferentialStatus::Passed
    );
    assert_eq!(
        outcome.evidence.semantic_differential,
        BuildDifferentialStatus::Passed
    );
    assert_eq!(outcome.evidence.resources.semantic_probe_queries, 2);
    assert_eq!(
        outcome.evidence.resources.accelerator_artifact_bytes,
        Some(outcome.evidence.resources.cpu_artifact_bytes)
    );
}

#[test]
fn hnsw_device_failure_and_bad_bytes_fall_back_only_when_policy_allows() {
    for (id, behavior) in [
        ("test.hnsw.fail", Behavior::Fail),
        ("test.hnsw.corrupt", Behavior::Corrupt),
        ("test.hnsw.wrong-generation", Behavior::WrongGeneration),
    ] {
        let (config, candidates) = hnsw_fixture();
        let mut registry = HnswAcceleratorRegistry::default();
        registry
            .install(Box::new(FakeHnswGpu::new(id, behavior)))
            .unwrap();
        let fallback = registry
            .build(
                config.clone(),
                1,
                2,
                candidates.clone(),
                HnswBuildPolicy::Prefer {
                    backend_id: id.into(),
                    allow_cpu_fallback: true,
                },
            )
            .unwrap();
        assert!(fallback.evidence.used_fallback);
        assert_eq!(
            fallback.evidence.selected_backend.target,
            AcceleratorTarget::Cpu
        );
        assert!(fallback.evidence.fallback_reason.is_some());

        assert!(registry
            .build(
                config,
                1,
                2,
                candidates,
                HnswBuildPolicy::Require {
                    backend_id: id.into(),
                },
            )
            .is_err());
    }
}
