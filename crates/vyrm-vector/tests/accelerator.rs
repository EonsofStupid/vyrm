#![cfg(feature = "accelerator")]

use std::collections::BTreeSet;
use vyrm_core::{ProjectionId, RuntimeProperties, RuntimeRef, RuntimeVector, ScopeId, VectorValue};
use vyrm_vector::{
    build_dense_artifact, AcceleratedBuildPolicy, AcceleratorTarget, CompactDenseSegment,
    DenseArtifactBuilder, DenseBuildBackend, ScoreMetric, VectorCandidate, VectorSegmentConfig,
    COMPACT_DENSE_FORMAT_VERSION,
};

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
    ) -> vyrm_core::Result<Vec<u8>> {
        if matches!(self.behavior, Behavior::Fail) {
            return Err(vyrm_core::Error::InvalidRuntime {
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
