#![cfg(feature = "object-transfer")]

use std::collections::BTreeSet;
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};
use vyrm_cluster::{
    prepare_artifact_transfer, transfer_artifacts, ArtifactTransferOperation,
    ArtifactTransferReceiver, ArtifactTransferRpc, ArtifactTransferRpcResult,
    ArtifactTransferSessionPolicy, NodeId, ReplicaTransferPlan, ShardId, ShardReadStamp,
    ARTIFACT_TRANSFER_CHUNK_MAX_BYTES, CLUSTER_CONTRACT_VERSION,
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

fn manifest_for(
    root: &std::path::Path,
    label: &str,
    bytes: &[u8],
) -> vyrm_cluster::ArtifactTransferManifest {
    let source = source_runtime(
        MemoryEngine::new(),
        LocalObjectStore::open(root.join(format!("source-{label}"))).unwrap(),
        bytes,
    );
    prepare_artifact_transfer(transfer_plan(), source.engine(), &scope()).unwrap()
}

fn policy(
    max_active_sessions: usize,
    max_reserved_bytes: u64,
    stale_incomplete_after_millis: u64,
    completed_receipt_retention_millis: u64,
    max_retained_receipts: usize,
) -> ArtifactTransferSessionPolicy {
    ArtifactTransferSessionPolicy {
        max_active_sessions,
        max_reserved_bytes,
        stale_incomplete_after_millis,
        completed_receipt_retention_millis,
        max_retained_receipts,
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
fn receiver_telemetry_rejects_prestart_observation_and_counts_invalid_requests() {
    let root = tempfile::tempdir().unwrap();
    let receiver = ArtifactTransferReceiver::open_with_policy_at(
        LocalObjectStore::open(root.path().join("target-telemetry-boundary")).unwrap(),
        ArtifactTransferSessionPolicy::default(),
        100,
    )
    .unwrap();
    assert!(receiver.telemetry_snapshot(99).is_err());
    let invalid = ArtifactTransferRpc {
        contract_version: 0,
        operation: ArtifactTransferOperation::Complete {
            manifest_digest: "00".repeat(32),
            completed_at: 100,
        },
    };
    assert!(receiver
        .handle_at(
            &NodeId::new("node:a").unwrap(),
            &NodeId::new("node:b").unwrap(),
            invalid,
            100,
        )
        .is_err());
    let telemetry = receiver.telemetry_snapshot(101).unwrap();
    assert_eq!(telemetry.complete_requests, 1);
    assert_eq!(telemetry.denied, 1);
    assert_eq!(telemetry.failed, 0);
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

#[test]
fn session_admission_enforces_count_and_reserved_byte_quotas_then_gc_reclaims_stale_work() {
    let root = tempfile::tempdir().unwrap();
    let first = manifest_for(root.path(), "quota-first", b"12345678");
    let second = manifest_for(root.path(), "quota-second", b"abcdefgh");
    let target = LocalObjectStore::open(root.path().join("target-quota")).unwrap();
    let receiver =
        ArtifactTransferReceiver::open_with_policy_at(target, policy(1, 12, 10, 100, 10), 90)
            .unwrap();

    receiver
        .handle_at(
            &first.plan.source,
            &first.plan.target,
            ArtifactTransferRpc::begin(first.clone()).unwrap(),
            100,
        )
        .unwrap();
    let inventory = receiver.session_inventory(105).unwrap();
    assert_eq!(inventory.active_sessions, 1);
    assert_eq!(inventory.reserved_bytes, 8);
    assert!(receiver
        .handle_at(
            &second.plan.source,
            &second.plan.target,
            ArtifactTransferRpc::begin(second.clone()).unwrap(),
            105,
        )
        .unwrap_err()
        .to_string()
        .contains("active-session quota"));

    let report = receiver.collect_garbage(110).unwrap();
    assert_eq!(report.removed_incomplete, 1);
    assert_eq!(report.remaining.active_sessions, 0);
    receiver
        .handle_at(
            &second.plan.source,
            &second.plan.target,
            ArtifactTransferRpc::begin(second.clone()).unwrap(),
            110,
        )
        .unwrap();

    let byte_limited = ArtifactTransferReceiver::open_with_policy_at(
        LocalObjectStore::open(root.path().join("target-byte-quota")).unwrap(),
        policy(2, 12, 1_000, 1_000, 10),
        190,
    )
    .unwrap();
    byte_limited
        .handle_at(
            &first.plan.source,
            &first.plan.target,
            ArtifactTransferRpc::begin(first.clone()).unwrap(),
            200,
        )
        .unwrap();
    assert!(byte_limited
        .handle_at(
            &second.plan.source,
            &second.plan.target,
            ArtifactTransferRpc::begin(second.clone()).unwrap(),
            201,
        )
        .unwrap_err()
        .to_string()
        .contains("reserved-byte quota"));
    let telemetry = byte_limited.telemetry_snapshot(202).unwrap();
    assert_eq!(telemetry.quota_denials, 1);
    assert_eq!(telemetry.failed, 1);
    assert_eq!(telemetry.inventory.active_sessions, 1);
}

#[test]
fn completed_receipts_are_idempotent_until_bounded_retention_gc() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"receipt retention bytes";
    let manifest = manifest_for(root.path(), "receipt", bytes);
    let target = LocalObjectStore::open(root.path().join("target-receipt")).unwrap();
    let receiver = ArtifactTransferReceiver::open_with_policy_at(
        target.clone(),
        policy(2, 1_024, 100, 10, 2),
        90,
    )
    .unwrap();
    receiver
        .handle_at(
            &manifest.plan.source,
            &manifest.plan.target,
            ArtifactTransferRpc::begin(manifest.clone()).unwrap(),
            100,
        )
        .unwrap();
    receiver
        .handle_at(
            &manifest.plan.source,
            &manifest.plan.target,
            ArtifactTransferRpc::chunk(
                manifest.manifest_digest.clone(),
                manifest.objects[0].sha256.clone(),
                0,
                bytes.to_vec(),
            )
            .unwrap(),
            101,
        )
        .unwrap();
    let completed = receiver
        .handle_at(
            &manifest.plan.source,
            &manifest.plan.target,
            ArtifactTransferRpc::complete(manifest.manifest_digest.clone(), 77).unwrap(),
            102,
        )
        .unwrap();
    assert_eq!(
        receiver.session_inventory(105).unwrap().retained_receipts,
        1
    );
    assert_eq!(
        receiver
            .handle_at(
                &manifest.plan.source,
                &manifest.plan.target,
                ArtifactTransferRpc::complete(manifest.manifest_digest.clone(), 999).unwrap(),
                105,
            )
            .unwrap(),
        completed
    );
    let before_gc = receiver.telemetry_snapshot(105).unwrap();
    assert_eq!(before_gc.begin_requests, 1);
    assert_eq!(before_gc.chunk_requests, 1);
    assert_eq!(before_gc.complete_requests, 2);
    assert_eq!(before_gc.completed_responses, 2);
    assert_eq!(before_gc.completed_receipt_replays, 1);
    let report = receiver.collect_garbage(112).unwrap();
    assert_eq!(report.removed_completed, 1);
    assert_eq!(report.remaining.retained_receipts, 0);
    assert_eq!(target.get(&manifest.objects[0]).unwrap(), bytes);
    let after_gc = receiver.telemetry_snapshot(113).unwrap();
    assert_eq!(after_gc.gc_removed_completed, 1);
    assert_eq!(after_gc.inventory.retained_receipts, 0);
}

#[test]
fn restart_recovers_inventory_and_distinct_sessions_accept_chunks_concurrently() {
    let root = tempfile::tempdir().unwrap();
    let first_bytes = vec![0x11; 64 * 1024];
    let second_bytes = vec![0x22; 96 * 1024];
    let first = manifest_for(root.path(), "concurrent-first", &first_bytes);
    let second = manifest_for(root.path(), "concurrent-second", &second_bytes);
    let target = LocalObjectStore::open(root.path().join("target-concurrent")).unwrap();
    let now: u64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap();
    let session_policy = policy(4, 1_024 * 1_024, 60_000, 60_000, 4);
    let receiver =
        ArtifactTransferReceiver::open_with_policy_at(target.clone(), session_policy.clone(), now)
            .unwrap();
    for manifest in [&first, &second] {
        receiver
            .handle_at(
                &manifest.plan.source,
                &manifest.plan.target,
                ArtifactTransferRpc::begin(manifest.clone()).unwrap(),
                now,
            )
            .unwrap();
    }
    let restarted =
        ArtifactTransferReceiver::open_with_policy_at(target.clone(), session_policy, now + 1)
            .unwrap();
    let inventory = restarted.session_inventory(now + 1).unwrap();
    assert_eq!(inventory.active_sessions, 2);
    assert_eq!(
        inventory.reserved_bytes,
        (first_bytes.len() + second_bytes.len()) as u64
    );
    let restarted_telemetry = restarted.telemetry_snapshot(now + 1).unwrap();
    assert_eq!(restarted_telemetry.begin_requests, 0);
    assert_eq!(restarted_telemetry.inventory.active_sessions, 2);

    let barrier = Arc::new(Barrier::new(3));
    let jobs = [
        (first.clone(), first_bytes.clone()),
        (second.clone(), second_bytes.clone()),
    ]
    .into_iter()
    .map(|(manifest, bytes)| {
        let receiver = restarted.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            receiver
                .handle_at(
                    &manifest.plan.source,
                    &manifest.plan.target,
                    ArtifactTransferRpc::chunk(
                        manifest.manifest_digest.clone(),
                        manifest.objects[0].sha256.clone(),
                        0,
                        bytes,
                    )
                    .unwrap(),
                    now + 2,
                )
                .unwrap()
        })
    })
    .collect::<Vec<_>>();
    barrier.wait();
    for job in jobs {
        let ArtifactTransferRpcResult::ChunkAccepted { object, .. } = job.join().unwrap() else {
            panic!("chunk did not return progress")
        };
        assert!(object.complete);
    }
    assert_eq!(target.get(&first.objects[0]).unwrap(), first_bytes);
    assert_eq!(target.get(&second.objects[0]).unwrap(), second_bytes);
    let telemetry = restarted.telemetry_snapshot(now + 3).unwrap();
    assert_eq!(telemetry.chunk_requests, 2);
    assert_eq!(telemetry.accepted_chunks, 2);
}

#[cfg(unix)]
#[test]
fn session_inventory_denies_symlinked_partial_state() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let manifest = manifest_for(root.path(), "symlink", b"symlink fixture");
    let target = LocalObjectStore::open(root.path().join("target-symlink")).unwrap();
    let receiver = ArtifactTransferReceiver::open(target.clone()).unwrap();
    receiver
        .handle_at(
            &manifest.plan.source,
            &manifest.plan.target,
            ArtifactTransferRpc::begin(manifest.clone()).unwrap(),
            100,
        )
        .unwrap();
    let external = root.path().join("outside-part");
    std::fs::write(&external, b"outside").unwrap();
    let part = target
        .root()
        .join("transfer-sessions-v1")
        .join(&manifest.manifest_digest)
        .join(format!("{}.part", manifest.objects[0].sha256));
    symlink(&external, part).unwrap();

    assert!(receiver
        .session_inventory(101)
        .unwrap_err()
        .to_string()
        .contains("outside its manifest bound"));
    assert_eq!(std::fs::read(external).unwrap(), b"outside");
}
