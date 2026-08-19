use vyrm_core::{
    digest, ReadStamp, RuntimeCommit, RuntimeId, RuntimeMutation, RuntimeProperties, RuntimeRecord,
    RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType, RuntimeValue, ScopeId,
    VectorValue,
};
use vyrm_embed::{
    EmbeddingBackend, EmbeddingCoordinator, EmbeddingJob, EmbeddingRequest, EmbeddingSourceReader,
    EmbeddingSourceSnapshot, ExecutionTarget, FeatureHashBackend, NetworkPolicy,
    NetworkRequirement, EMBEDDING_CONTRACT_VERSION,
};
use vyrm_store::{Engine, MemoryEngine};

struct SequenceReader {
    snapshots: Vec<EmbeddingSourceSnapshot>,
    reads: usize,
}

impl EmbeddingSourceReader for SequenceReader {
    fn read(&mut self, _source: &RuntimeRef) -> vyrm_core::Result<EmbeddingSourceSnapshot> {
        let snapshot = self.snapshots[self.reads.min(self.snapshots.len() - 1)].clone();
        self.reads += 1;
        Ok(snapshot)
    }
}

fn job(bytes: &[u8], backend: &FeatureHashBackend) -> EmbeddingJob {
    let scope = ScopeId::new("instance:embedding").unwrap();
    EmbeddingJob {
        contract_version: EMBEDDING_CONTRACT_VERSION,
        id: RuntimeId::new("job-1").unwrap(),
        scope: scope.clone(),
        read: ReadStamp::new(scope, None, 0, 7, Some("11".repeat(32))).unwrap(),
        source: RuntimeRef::new("document", "source-1").unwrap(),
        expected_source_digest: digest::sha256_hex(bytes),
        target: RuntimeRef::new("embedding", "source-1-body").unwrap(),
        subject: RuntimeRef::new("document", "source-1").unwrap(),
        field: "body".into(),
        valid_from: 10,
        valid_to: None,
        model: backend.descriptor().model.clone(),
        network_policy: NetworkPolicy::Deny,
        requested_at: 11,
        properties: RuntimeProperties::new(),
    }
}

#[test]
fn offline_generation_is_deterministic_normalized_and_provenance_bound() {
    let bytes = b"Vyrm makes source-grounded reasoning durable";
    let mut backend = FeatureHashBackend::new(64, 7).unwrap();
    let job = job(bytes, &backend);
    let snapshot =
        EmbeddingSourceSnapshot::for_bytes(job.source.clone(), "text/plain", bytes.to_vec())
            .unwrap();
    let mut reader = SequenceReader {
        snapshots: vec![snapshot],
        reads: 0,
    };
    let prepared = EmbeddingCoordinator::prepare(&job, &mut reader, &mut backend).unwrap();
    prepared.validate().unwrap();
    assert_eq!(reader.reads, 2);
    let VectorValue::Dense { values } = &prepared.vector.value else {
        panic!("feature hash must produce dense vectors")
    };
    let norm = values
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>();
    assert!((norm - 1.0).abs() <= 1e-6);
    assert_eq!(
        prepared.vector.provenance.as_ref().unwrap().source_digest,
        digest::sha256_hex(bytes)
    );
    let transaction = prepared.transaction(&job, "agent:embedding", 12).unwrap();
    assert_eq!(transaction.read, job.read);
    assert_eq!(transaction.commit.expected_cursor, 7);
}

#[test]
fn source_change_during_inference_fails_before_a_mutation_exists() {
    let original = b"original source";
    let changed = b"changed source";
    let mut backend = FeatureHashBackend::new(32, 9).unwrap();
    let job = job(original, &backend);
    let mut reader = SequenceReader {
        snapshots: vec![
            EmbeddingSourceSnapshot::for_bytes(job.source.clone(), "text/plain", original.to_vec())
                .unwrap(),
            EmbeddingSourceSnapshot::for_bytes(job.source.clone(), "text/plain", changed.to_vec())
                .unwrap(),
        ],
        reads: 0,
    };
    let error = EmbeddingCoordinator::prepare(&job, &mut reader, &mut backend).unwrap_err();
    assert!(error.to_string().contains("changed during inference"));
}

struct MalformedBackend {
    descriptor: vyrm_embed::EmbeddingBackendDescriptor,
}

impl EmbeddingBackend for MalformedBackend {
    fn descriptor(&self) -> &vyrm_embed::EmbeddingBackendDescriptor {
        &self.descriptor
    }

    fn embed(&mut self, _request: &EmbeddingRequest) -> vyrm_core::Result<VectorValue> {
        Ok(VectorValue::Dense {
            values: vec![1.0, 0.0],
        })
    }
}

#[test]
fn malformed_output_shape_fails_closed() {
    let bytes = b"shape mismatch";
    let baseline = FeatureHashBackend::new(32, 5).unwrap();
    let job = job(bytes, &baseline);
    let snapshot =
        EmbeddingSourceSnapshot::for_bytes(job.source.clone(), "text/plain", bytes.to_vec())
            .unwrap();
    let mut reader = SequenceReader {
        snapshots: vec![snapshot],
        reads: 0,
    };
    let mut backend = MalformedBackend {
        descriptor: baseline.descriptor().clone(),
    };
    assert!(EmbeddingCoordinator::prepare(&job, &mut reader, &mut backend).is_err());
}

#[test]
fn offline_policy_and_exact_model_identity_are_enforced_before_inference() {
    let bytes = b"policy enforcement";
    let baseline = FeatureHashBackend::new(32, 5).unwrap();
    let job = job(bytes, &baseline);
    let snapshot =
        EmbeddingSourceSnapshot::for_bytes(job.source.clone(), "text/plain", bytes.to_vec())
            .unwrap();
    let mut reader = SequenceReader {
        snapshots: vec![snapshot.clone()],
        reads: 0,
    };
    let mut remote_descriptor = baseline.descriptor().clone();
    remote_descriptor.execution = ExecutionTarget::Remote {
        provider: "example".into(),
    };
    remote_descriptor.network = NetworkRequirement::Required;
    let mut remote = MalformedBackend {
        descriptor: remote_descriptor,
    };
    let error = EmbeddingCoordinator::prepare(&job, &mut reader, &mut remote).unwrap_err();
    assert!(error.to_string().contains("denies the network"));
    assert_eq!(reader.reads, 0);

    let mut reader = SequenceReader {
        snapshots: vec![snapshot],
        reads: 0,
    };
    let mut wrong_descriptor = baseline.descriptor().clone();
    wrong_descriptor.model.revision = "other".into();
    let mut wrong_model = MalformedBackend {
        descriptor: wrong_descriptor,
    };
    let error = EmbeddingCoordinator::prepare(&job, &mut reader, &mut wrong_model).unwrap_err();
    assert!(error.to_string().contains("requested model"));
    assert_eq!(reader.reads, 0);
}

#[test]
fn transaction_cas_rejects_a_runtime_source_change_after_inference() {
    let bytes = b"source at captured cursor";
    let mut backend = FeatureHashBackend::new(32, 17).unwrap();
    let mut job = job(bytes, &backend);
    let engine = MemoryEngine::default();
    let mut properties = RuntimeProperties::new();
    properties.insert(
        "content_digest".into(),
        RuntimeValue::Digest(digest::sha256_hex(bytes)),
    );
    engine
        .commit_runtime(&RuntimeCommit {
            scope: job.scope.clone(),
            at: 10,
            actor: "test:source".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema {
                    registry: {
                        let mut registry =
                            RuntimeSchemaRegistry::empty(1, "embedding source test schema");
                        registry.records.insert(
                            RuntimeType::new("document").unwrap(),
                            RuntimeRecordSchema {
                                allow_additional_properties: true,
                                ..RuntimeRecordSchema::default()
                            },
                        );
                        registry
                    },
                },
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: job.source.clone(),
                        valid_from: 10,
                        valid_to: None,
                        properties,
                    },
                },
            ],
        })
        .unwrap();
    job.read = engine.runtime_read_stamp(&job.scope).unwrap();
    let snapshot =
        EmbeddingSourceSnapshot::for_bytes(job.source.clone(), "text/plain", bytes.to_vec())
            .unwrap();
    let mut reader = SequenceReader {
        snapshots: vec![snapshot],
        reads: 0,
    };
    let prepared = EmbeddingCoordinator::prepare(&job, &mut reader, &mut backend).unwrap();

    let mut changed = RuntimeProperties::new();
    changed.insert(
        "content_digest".into(),
        RuntimeValue::Digest(digest::sha256_hex(b"changed after inference")),
    );
    engine
        .commit_runtime(&RuntimeCommit {
            scope: job.scope.clone(),
            at: 12,
            actor: "test:source".into(),
            expected_cursor: job.read.commit_cursor,
            mutations: vec![RuntimeMutation::Record {
                record: RuntimeRecord {
                    reference: job.source.clone(),
                    valid_from: 10,
                    valid_to: None,
                    properties: changed,
                },
            }],
        })
        .unwrap();

    let transaction = prepared.transaction(&job, "agent:embedding", 13).unwrap();
    assert!(engine.commit_data_transaction(&transaction).is_err());
}
