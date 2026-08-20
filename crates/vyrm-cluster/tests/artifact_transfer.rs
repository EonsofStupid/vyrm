#![cfg(feature = "object-transfer")]

use std::collections::BTreeSet;
use vyrm_cluster::{
    prepare_artifact_transfer, transfer_artifacts, ArtifactTransferOperation,
    ArtifactTransferReceiver, ArtifactTransferRpc, ArtifactTransferRpcResult, NodeId,
    ReplicaTransferPlan, ShardId, ShardReadStamp, ARTIFACT_TRANSFER_CHUNK_MAX_BYTES,
    CLUSTER_CONTRACT_VERSION,
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

#[test]
fn durable_chunk_session_resumes_rejects_wrong_offsets_and_completes_once() {
    let root = tempfile::tempdir().unwrap();
    let bytes = (0..(ARTIFACT_TRANSFER_CHUNK_MAX_BYTES + 73_117))
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let source = source_runtime(
        MemoryEngine::new(),
        LocalObjectStore::open(root.path().join("source-resume")).unwrap(),
        &bytes,
    );
    let manifest = prepare_artifact_transfer(transfer_plan(), source.engine(), &scope()).unwrap();
    let target = LocalObjectStore::open(root.path().join("target-resume")).unwrap();
    let source_node = manifest.plan.source.clone();
    let target_node = manifest.plan.target.clone();
    let receiver = ArtifactTransferReceiver::open(target.clone()).unwrap();

    let ArtifactTransferRpcResult::Progress { objects, .. } = receiver
        .handle(
            &source_node,
            &target_node,
            ArtifactTransferRpc::begin(manifest.clone()).unwrap(),
        )
        .unwrap()
    else {
        panic!("begin did not return progress")
    };
    assert_eq!(objects[0].next_offset, 0);
    let first = 333_333usize;
    receiver
        .handle(
            &source_node,
            &target_node,
            ArtifactTransferRpc::chunk(
                manifest.manifest_digest.clone(),
                manifest.objects[0].sha256.clone(),
                0,
                bytes[..first].to_vec(),
            )
            .unwrap(),
        )
        .unwrap();

    let restarted = ArtifactTransferReceiver::open(target.clone()).unwrap();
    let ArtifactTransferRpcResult::Progress { objects, .. } = restarted
        .handle(
            &source_node,
            &target_node,
            ArtifactTransferRpc::begin(manifest.clone()).unwrap(),
        )
        .unwrap()
    else {
        panic!("resume did not return progress")
    };
    assert_eq!(objects[0].next_offset, first as u64);
    let ArtifactTransferRpcResult::ChunkAccepted { object, .. } = restarted
        .handle(
            &source_node,
            &target_node,
            ArtifactTransferRpc::chunk(
                manifest.manifest_digest.clone(),
                manifest.objects[0].sha256.clone(),
                0,
                b"wrong-offset".to_vec(),
            )
            .unwrap(),
        )
        .unwrap()
    else {
        panic!("wrong-offset retry did not return authoritative progress")
    };
    assert_eq!(object.next_offset, first as u64);

    let mut offset = first;
    while offset < bytes.len() {
        let end = (offset + ARTIFACT_TRANSFER_CHUNK_MAX_BYTES).min(bytes.len());
        restarted
            .handle(
                &source_node,
                &target_node,
                ArtifactTransferRpc::chunk(
                    manifest.manifest_digest.clone(),
                    manifest.objects[0].sha256.clone(),
                    offset as u64,
                    bytes[offset..end].to_vec(),
                )
                .unwrap(),
            )
            .unwrap();
        offset = end;
    }
    let completed = restarted
        .handle(
            &source_node,
            &target_node,
            ArtifactTransferRpc::complete(manifest.manifest_digest.clone(), 99).unwrap(),
        )
        .unwrap();
    let ArtifactTransferRpcResult::Completed { receipt } = completed else {
        panic!("completion did not return a receipt")
    };
    assert_eq!(receipt.transferred_objects, 1);
    assert_eq!(receipt.transferred_bytes, bytes.len() as u64);
    let replayed = restarted
        .handle(
            &source_node,
            &target_node,
            ArtifactTransferRpc::complete(manifest.manifest_digest.clone(), 99).unwrap(),
        )
        .unwrap();
    assert_eq!(
        replayed,
        ArtifactTransferRpcResult::Completed {
            receipt: receipt.clone()
        }
    );
    assert_eq!(
        restarted
            .handle(
                &source_node,
                &target_node,
                ArtifactTransferRpc::complete(manifest.manifest_digest.clone(), 100).unwrap(),
            )
            .unwrap(),
        ArtifactTransferRpcResult::Completed { receipt }
    );
    assert_eq!(target.get(&manifest.objects[0]).unwrap(), bytes);
}

#[test]
fn chunk_sessions_bind_authenticated_peers_and_discard_corrupt_completed_parts() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"expected immutable bytes";
    let source = source_runtime(
        MemoryEngine::new(),
        LocalObjectStore::open(root.path().join("source-corrupt-session")).unwrap(),
        bytes,
    );
    let manifest = prepare_artifact_transfer(transfer_plan(), source.engine(), &scope()).unwrap();
    let target = LocalObjectStore::open(root.path().join("target-corrupt-session")).unwrap();
    let receiver = ArtifactTransferReceiver::open(target.clone()).unwrap();
    let source_node = manifest.plan.source.clone();
    let target_node = manifest.plan.target.clone();
    assert!(receiver
        .handle(
            &NodeId::new("node:impostor").unwrap(),
            &target_node,
            ArtifactTransferRpc::begin(manifest.clone()).unwrap(),
        )
        .is_err());
    receiver
        .handle(
            &source_node,
            &target_node,
            ArtifactTransferRpc::begin(manifest.clone()).unwrap(),
        )
        .unwrap();

    let mut tampered = ArtifactTransferRpc::chunk(
        manifest.manifest_digest.clone(),
        manifest.objects[0].sha256.clone(),
        0,
        vec![b'x'; bytes.len()],
    )
    .unwrap();
    let ArtifactTransferOperation::Chunk { chunk_digest, .. } = &mut tampered.operation else {
        unreachable!()
    };
    *chunk_digest = "00".repeat(32);
    assert!(receiver
        .handle(&source_node, &target_node, tampered)
        .is_err());
    let corrupt = ArtifactTransferRpc::chunk(
        manifest.manifest_digest.clone(),
        manifest.objects[0].sha256.clone(),
        0,
        vec![b'x'; bytes.len()],
    )
    .unwrap();
    assert!(receiver
        .handle(&source_node, &target_node, corrupt)
        .is_err());
    assert!(target.verify(&manifest.objects[0].sha256).is_err());
    let ArtifactTransferRpcResult::Progress { objects, .. } = receiver
        .handle(
            &source_node,
            &target_node,
            ArtifactTransferRpc::begin(manifest).unwrap(),
        )
        .unwrap()
    else {
        panic!("restart did not return progress")
    };
    assert_eq!(objects[0].next_offset, 0);
}
