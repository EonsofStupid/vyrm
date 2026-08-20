#![cfg(feature = "object-transfer")]

use std::collections::BTreeSet;
use vyrm_cluster::{
    prepare_artifact_transfer, transfer_artifacts, NodeId, ReplicaTransferPlan, ShardId,
    ShardReadStamp, CLUSTER_CONTRACT_VERSION,
};
use vyrm_core::{
    DataTransaction, RuntimeCommit, RuntimeMutation, RuntimeProperties, RuntimeRecord,
    RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType, ScopeId,
};
use vyrm_store::{DataRuntime, Engine, LocalObjectStore, MemoryEngine};

fn scope() -> ScopeId {
    ScopeId::new("instance:artifact-transfer").unwrap()
}

fn transfer_plan() -> ReplicaTransferPlan {
    ReplicaTransferPlan {
        contract_version: CLUSTER_CONTRACT_VERSION,
        shard: ShardId(7),
        placement_epoch: 3,
        source: NodeId::new("node:a").unwrap(),
        target: NodeId::new("node:b").unwrap(),
        grounded_snapshot: ShardReadStamp {
            term: 4,
            commit_index: 12,
            placement_epoch: 3,
            state_digest: "77".repeat(32),
        },
        wal_from_exclusive: 12,
        wal_through_inclusive: 12,
        artifact_digests: BTreeSet::new(),
    }
}

fn source_runtime(
    engine: MemoryEngine,
    objects: LocalObjectStore,
    bytes: &[u8],
) -> DataRuntime<MemoryEngine, LocalObjectStore> {
    let runtime = DataRuntime::new(engine, objects);
    let scope = scope();
    let subject = RuntimeRef::new("document", "doc:one").unwrap();
    let first = runtime
        .stage_object(
            "artifact:first",
            Some(subject.clone()),
            "application/octet-stream",
            bytes,
        )
        .unwrap();
    let second = runtime
        .stage_object(
            "artifact:alias",
            Some(subject.clone()),
            "application/octet-stream",
            bytes,
        )
        .unwrap();
    let mut schema = RuntimeSchemaRegistry::empty(1, "artifact transfer fixture");
    schema.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema::default(),
    );
    let read = runtime.engine().runtime_read_stamp(&scope).unwrap();
    runtime
        .commit(
            &DataTransaction::new(
                read,
                RuntimeCommit {
                    scope,
                    at: 10,
                    actor: "cluster:test".into(),
                    expected_cursor: 0,
                    mutations: vec![
                        RuntimeMutation::Schema { registry: schema },
                        RuntimeMutation::Record {
                            record: RuntimeRecord {
                                reference: subject,
                                valid_from: 10,
                                valid_to: None,
                                properties: RuntimeProperties::new(),
                            },
                        },
                        RuntimeMutation::Object { object: first },
                        RuntimeMutation::Object { object: second },
                    ],
                },
            )
            .unwrap(),
        )
        .unwrap();
    runtime
}

#[test]
fn manifest_streams_each_digest_once_and_retry_reuses_verified_target_bytes() {
    let root = tempfile::tempdir().unwrap();
    let bytes = vec![0xa5; 512 * 1024 + 31];
    let source = source_runtime(
        MemoryEngine::new(),
        LocalObjectStore::open(root.path().join("source")).unwrap(),
        &bytes,
    );
    let target = LocalObjectStore::open(root.path().join("target")).unwrap();
    let manifest = prepare_artifact_transfer(transfer_plan(), source.engine(), &scope()).unwrap();
    assert_eq!(manifest.objects.len(), 2);
    assert_eq!(manifest.plan.artifact_digests.len(), 1);

    let first = transfer_artifacts(source.objects(), &target, &manifest, 20).unwrap();
    assert_eq!(first.transferred_objects, 1);
    assert_eq!(first.transferred_bytes, bytes.len() as u64);
    first.validate(&manifest).unwrap();
    for object in &manifest.objects {
        assert_eq!(target.get(object).unwrap(), bytes);
    }

    let retry = transfer_artifacts(source.objects(), &target, &manifest, 21).unwrap();
    assert_eq!(retry.transferred_objects, 0);
    assert_eq!(retry.transferred_bytes, 0);
    retry.validate(&manifest).unwrap();
}

#[test]
fn corruption_missing_source_and_manifest_substitution_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"grounded artifact bytes";
    let source_path = root.path().join("source");
    let source = source_runtime(
        MemoryEngine::new(),
        LocalObjectStore::open(&source_path).unwrap(),
        bytes,
    );
    let target = LocalObjectStore::open(root.path().join("target")).unwrap();
    let manifest = prepare_artifact_transfer(transfer_plan(), source.engine(), &scope()).unwrap();

    let mut substituted = manifest.clone();
    substituted.objects[0].length += 1;
    assert!(substituted.validate().is_err());
    assert!(transfer_artifacts(source.objects(), &target, &substituted, 20).is_err());

    let key = vyrm_core::ObjectReference::canonical_key(&manifest.objects[0].sha256).unwrap();
    std::fs::remove_file(source_path.join(key)).unwrap();
    let error = transfer_artifacts(source.objects(), &target, &manifest, 20)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not found"));
    assert!(target
        .inventory(&manifest.plan.artifact_digests)
        .unwrap()
        .entries
        .is_empty());
}
