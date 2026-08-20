#![cfg(feature = "openraft-adapter")]

use openraft::storage::{RaftLogStorage, RaftStateMachine};
use openraft::testing::{StoreBuilder, Suite};
use openraft::{
    CommittedLeaderId, Entry, EntryPayload, ErrorSubject, ErrorVerb, LogId, Membership,
    RaftSnapshotBuilder, StorageError, Vote,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use vyrm_cluster::{
    prepare_artifact_transfer, ArtifactTransferManifest, ClusterError, ClusterId, NodeId,
    PlacementPolicy, ReplicaPlacement, ReplicaRole, ReplicaTransferPlan, ShardId, ShardPlacement,
    ShardReadStamp, VyrmRaftCommand, VyrmRaftLogStore, VyrmRaftNode, VyrmRaftStateMachine,
    VyrmRaftStore, VyrmRaftTypeConfig, VyrmSnapshotData, ZoneId, CLUSTER_CONTRACT_VERSION,
};
use vyrm_core::{
    ObjectReference, RuntimeCommit, RuntimeMutation, RuntimeRecordSchema, RuntimeSchemaRegistry,
    RuntimeType, ScopeId,
};
use vyrm_kv::{recover, Database, Durability, Mutation, SnapshotBundle, WriteBatch};
use vyrm_store::{Engine, LocalObjectStore, NativeEngine};

struct VyrmStoreBuilder;

static TEST_SNAPSHOT_ORDINAL: AtomicU64 = AtomicU64::new(1);

async fn snapshot_bytes(mut snapshot: Box<VyrmSnapshotData>) -> Result<Vec<u8>, StorageError<u64>> {
    snapshot
        .seek(std::io::SeekFrom::Start(0))
        .await
        .map_err(test_storage_error)?;
    let mut bytes = Vec::new();
    snapshot
        .read_to_end(&mut bytes)
        .await
        .map_err(test_storage_error)?;
    Ok(bytes)
}

async fn snapshot_handle(
    root: &std::path::Path,
    bytes: &[u8],
) -> Result<Box<VyrmSnapshotData>, StorageError<u64>> {
    let path = root.join(format!(
        "test-snapshot-{}.spool",
        TEST_SNAPSHOT_ORDINAL.fetch_add(1, Ordering::Relaxed)
    ));
    let mut snapshot = VyrmSnapshotData::create_ephemeral(path)
        .await
        .map_err(test_storage_error)?;
    snapshot
        .write_all(bytes)
        .await
        .map_err(test_storage_error)?;
    Ok(Box::new(snapshot))
}

impl StoreBuilder<VyrmRaftTypeConfig, VyrmRaftLogStore, VyrmRaftStateMachine, TempDir>
    for VyrmStoreBuilder
{
    async fn build(
        &self,
    ) -> Result<(TempDir, VyrmRaftLogStore, VyrmRaftStateMachine), StorageError<u64>> {
        let directory = tempfile::tempdir().map_err(test_storage_error)?;
        let (log, state_machine) =
            VyrmRaftStore::open(directory.path(), ShardId(1)).map_err(test_storage_error)?;
        Ok((directory, log, state_machine))
    }
}

#[test]
fn vyrm_native_store_passes_openraft_complete_conformance_suite() {
    Suite::test_all(VyrmStoreBuilder).unwrap();
}

#[test]
fn application_state_and_idempotency_survive_reopen() {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            let directory = tempfile::tempdir().map_err(test_storage_error)?;
            let command = VyrmRaftCommand::new(
                "request-1",
                ShardId(7),
                1,
                Some(2),
                b"canonical-runtime-commit".to_vec(),
            )
            .map_err(test_storage_error)?;
            let (_, mut state_machine) =
                VyrmRaftStore::open(directory.path(), ShardId(7)).map_err(test_storage_error)?;
            state_machine
                .apply([test_membership_entry(LogId::new(
                    CommittedLeaderId::new(2, 11),
                    1,
                ))])
                .await?;
            let transition = VyrmRaftCommand::placement_transition(
                "placement-1",
                test_placement(ShardId(7), 1),
                Some(1),
            )
            .map_err(test_storage_error)?;
            state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(2, 11), 2),
                    payload: EntryPayload::Normal(transition),
                }])
                .await?;
            let command_log = LogId::new(CommittedLeaderId::new(2, 11), 3);
            let responses = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: command_log,
                    payload: EntryPayload::Normal(command.clone()),
                }])
                .await?;
            assert!(responses[0].accepted);
            assert!(!responses[0].duplicate);
            let original_digest = responses[0].state_digest.clone();
            drop(state_machine);

            let (_, mut reopened) =
                VyrmRaftStore::open(directory.path(), ShardId(7)).map_err(test_storage_error)?;
            assert_eq!(reopened.applied_state().await?.0, Some(command_log));
            let duplicate_log = LogId::new(CommittedLeaderId::new(3, 12), 4);
            let duplicate = reopened
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: duplicate_log,
                    payload: EntryPayload::Normal(command),
                }])
                .await?;
            assert!(duplicate[0].accepted);
            assert!(duplicate[0].duplicate);
            assert_eq!(duplicate[0].state_digest, original_digest);
            Ok::<(), StorageError<u64>>(())
        })
        .unwrap();
}

#[test]
fn placement_epoch_and_full_request_identity_fail_closed() {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            let directory = tempfile::tempdir().map_err(test_storage_error)?;
            let (_, mut state_machine) =
                VyrmRaftStore::open(directory.path(), ShardId(9)).map_err(test_storage_error)?;
            state_machine
                .apply([test_membership_entry(LogId::new(
                    CommittedLeaderId::new(1, 1),
                    1,
                ))])
                .await?;

            let transition = VyrmRaftCommand::placement_transition(
                "epoch-1",
                test_placement(ShardId(9), 1),
                Some(1),
            )
            .map_err(test_storage_error)?;
            let transition_response = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 2),
                    payload: EntryPayload::Normal(transition),
                }])
                .await?;
            assert!(transition_response[0].accepted);

            let first =
                VyrmRaftCommand::new("epoch-anchor", ShardId(9), 1, Some(2), b"first".to_vec())
                    .map_err(test_storage_error)?;
            let first_response = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 3),
                    payload: EntryPayload::Normal(first),
                }])
                .await?;
            assert!(first_response[0].accepted);

            let wrong_epoch =
                VyrmRaftCommand::new("wrong-epoch", ShardId(9), 2, None, b"second".to_vec())
                    .map_err(test_storage_error)?;
            let denied = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 4),
                    payload: EntryPayload::Normal(wrong_epoch),
                }])
                .await?;
            assert!(!denied[0].accepted);
            assert!(denied[0].reason.contains("placement epoch"));

            let skipped = VyrmRaftCommand::placement_transition(
                "epoch-skipped",
                test_placement(ShardId(9), 3),
                Some(4),
            )
            .map_err(test_storage_error)?;
            let skipped_response = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 5),
                    payload: EntryPayload::Normal(skipped),
                }])
                .await?;
            assert!(!skipped_response[0].accepted);
            assert!(skipped_response[0].reason.contains("not the successor"));

            let mut wrong_membership = test_placement(ShardId(9), 2);
            wrong_membership.replicas[0].zone = ZoneId::new("az-foreign").unwrap();
            let mismatched = VyrmRaftCommand::placement_transition(
                "epoch-wrong-membership",
                wrong_membership,
                Some(5),
            )
            .map_err(test_storage_error)?;
            let mismatched_response = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 6),
                    payload: EntryPayload::Normal(mismatched),
                }])
                .await?;
            assert!(!mismatched_response[0].accepted);
            assert!(mismatched_response[0].reason.contains("Raft membership"));

            let advance = VyrmRaftCommand::placement_transition(
                "epoch-2",
                test_placement(ShardId(9), 2),
                Some(6),
            )
            .map_err(test_storage_error)?;
            let advanced = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 7),
                    payload: EntryPayload::Normal(advance),
                }])
                .await?;
            assert!(advanced[0].accepted);

            state_machine
                .apply([test_membership_entry_with_first_zone(
                    LogId::new(CommittedLeaderId::new(1, 1), 8),
                    "az-foreign",
                )])
                .await?;
            state_machine
                .apply([test_membership_entry(LogId::new(
                    CommittedLeaderId::new(1, 1),
                    9,
                ))])
                .await?;
            let stale_binding = VyrmRaftCommand::new(
                "stale-membership-binding",
                ShardId(9),
                2,
                Some(9),
                b"must-deny".to_vec(),
            )
            .map_err(test_storage_error)?;
            let stale_binding_response = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 10),
                    payload: EntryPayload::Normal(stale_binding),
                }])
                .await?;
            assert!(!stale_binding_response[0].accepted);
            assert!(stale_binding_response[0].reason.contains("membership"));

            let rebound = VyrmRaftCommand::placement_transition(
                "epoch-3",
                test_placement(ShardId(9), 3),
                Some(10),
            )
            .map_err(test_storage_error)?;
            let rebound_response = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 11),
                    payload: EntryPayload::Normal(rebound),
                }])
                .await?;
            assert!(rebound_response[0].accepted);

            let original = VyrmRaftCommand::new(
                "identity",
                ShardId(9),
                3,
                Some(11),
                b"same-payload".to_vec(),
            )
            .map_err(test_storage_error)?;
            let accepted = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 12),
                    payload: EntryPayload::Normal(original),
                }])
                .await?;
            assert!(accepted[0].accepted);

            let changed_cas =
                VyrmRaftCommand::new("identity", ShardId(9), 3, None, b"same-payload".to_vec())
                    .map_err(test_storage_error)?;
            let reused = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 13),
                    payload: EntryPayload::Normal(changed_cas),
                }])
                .await?;
            assert!(!reused[0].accepted);
            assert!(reused[0].reason.contains("request id"));
            Ok::<(), StorageError<u64>>(())
        })
        .unwrap();
}

#[test]
fn canonical_runtime_commit_is_atomic_idempotent_durable_and_transferable() {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            let directory = tempfile::tempdir().map_err(test_storage_error)?;
            let (mut log_store, mut state_machine) =
                VyrmRaftStore::open(directory.path(), ShardId(12)).map_err(test_storage_error)?;
            state_machine
                .apply([test_membership_entry(LogId::new(
                    CommittedLeaderId::new(4, 1),
                    1,
                ))])
                .await?;
            let transition = VyrmRaftCommand::placement_transition(
                "placement-1",
                test_placement(ShardId(12), 1),
                Some(1),
            )
            .map_err(test_storage_error)?;
            state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(4, 1), 2),
                    payload: EntryPayload::Normal(transition),
                }])
                .await?;
            let artifact_bytes = b"cluster-transferred-vector-artifact";
            let source_artifacts =
                LocalObjectStore::open(directory.path().join("application-objects"))
                    .map_err(test_storage_error)?;
            let staged = source_artifacts
                .put(artifact_bytes)
                .map_err(test_storage_error)?;
            let artifact = ObjectReference::for_bytes(
                "vector:hnsw:body@1:bytes",
                None,
                "application/vnd.vyrm.vector-hnsw+json",
                artifact_bytes,
                staged.receipt,
            )
            .map_err(test_storage_error)?;
            let mut commit = bootstrap_runtime_commit("cluster:atomic", 0);
            commit
                .mutations
                .push(RuntimeMutation::Object { object: artifact });
            let command = VyrmRaftCommand::runtime_commit(
                "runtime-1",
                ShardId(12),
                1,
                Some(2),
                commit.clone(),
            )
            .map_err(test_storage_error)?;
            let first_log = LogId::new(CommittedLeaderId::new(4, 1), 3);
            let first = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: first_log,
                    payload: EntryPayload::Normal(command.clone()),
                }])
                .await?;
            assert!(first[0].accepted);
            let outcome = first[0].runtime_outcome.clone().unwrap();
            assert_eq!(outcome.commit_id, commit.digest());
            assert_eq!(outcome.last_cursor, 2);

            let duplicate_log = LogId::new(CommittedLeaderId::new(4, 1), 4);
            let duplicate = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: duplicate_log,
                    payload: EntryPayload::Normal(command),
                }])
                .await?;
            assert!(duplicate[0].accepted);
            assert!(duplicate[0].duplicate);
            assert_eq!(duplicate[0].runtime_outcome.as_ref(), Some(&outcome));

            let content_retry =
                VyrmRaftCommand::runtime_commit("runtime-2", ShardId(12), 1, None, commit)
                    .map_err(test_storage_error)?;
            let retry_log = LogId::new(CommittedLeaderId::new(4, 1), 5);
            let retried = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: retry_log,
                    payload: EntryPayload::Normal(content_retry),
                }])
                .await?;
            assert!(retried[0].accepted);
            assert!(!retried[0].duplicate);
            assert!(retried[0].reason.contains("already committed"));
            assert_eq!(retried[0].runtime_outcome.as_ref(), Some(&outcome));

            let source_vote = Vote::new(7, 1);
            log_store.save_vote(&source_vote).await?;
            let mut builder = state_machine.get_snapshot_builder().await;
            let snapshot = builder.build_snapshot().await?;
            assert_eq!(snapshot.meta.last_log_id, Some(retry_log));
            assert!(snapshot.meta.snapshot_id.starts_with("v4-5-"));
            let snapshot_meta = snapshot.meta;
            let snapshot_data = snapshot_bytes(snapshot.snapshot).await?;
            let physical = SnapshotBundle::decode(&snapshot_data).map_err(test_storage_error)?;
            assert!(physical
                .get(b"vyrm/raft/v4/state/current")
                .map_err(test_storage_error)?
                .is_some());
            assert!(physical
                .get(b"vyrm/raft/v4/local/config")
                .map_err(test_storage_error)?
                .is_none());
            assert!(physical
                .get(b"vyrm/raft/v4/local/vote")
                .map_err(test_storage_error)?
                .is_none());
            let cached_transfer = state_machine
                .artifact_manifest_for_cached_snapshot(
                    &snapshot_meta,
                    &ScopeId::new("cluster:atomic").map_err(test_storage_error)?,
                    NodeId::new("node:source").map_err(test_storage_error)?,
                    NodeId::new("node:target").map_err(test_storage_error)?,
                )?
                .expect("artifact-bearing snapshot must produce a transfer manifest");
            drop(builder);
            drop(state_machine);
            drop(log_store);

            let source_native = NativeEngine::open(directory.path()).map_err(test_storage_error)?;
            let transfer = prepare_artifact_transfer(
                ReplicaTransferPlan {
                    contract_version: CLUSTER_CONTRACT_VERSION,
                    shard: ShardId(12),
                    placement_epoch: 1,
                    source: NodeId::new("node:source").map_err(test_storage_error)?,
                    target: NodeId::new("node:target").map_err(test_storage_error)?,
                    grounded_snapshot: ShardReadStamp {
                        term: retry_log.leader_id.term,
                        commit_index: retry_log.index,
                        placement_epoch: 1,
                        state_digest: retried[0].state_digest.clone(),
                    },
                    wal_from_exclusive: retry_log.index,
                    wal_through_inclusive: retry_log.index,
                    artifact_digests: BTreeSet::new(),
                },
                &source_native,
                &ScopeId::new("cluster:atomic").map_err(test_storage_error)?,
            )
            .map_err(test_storage_error)?;
            assert_eq!(transfer.objects.len(), 1);
            assert_eq!(cached_transfer, transfer);
            drop(source_native);

            let wal = recover(&directory.path().join("wal/00000000000000000001.wal"))
                .map_err(test_storage_error)?;
            let atomic_frame = wal.batches.iter().any(|batch| {
                let decoded = WriteBatch::decode(&batch.payload).unwrap();
                let has_raft_state = decoded
                    .operations
                    .iter()
                    .any(|operation| operation.key() == b"vyrm/raft/v4/state/current");
                let has_runtime_change = decoded
                    .operations
                    .iter()
                    .any(|operation| operation.key().starts_with(b"runtime_changes\0"));
                has_raft_state && has_runtime_change
            });
            assert!(
                atomic_frame,
                "runtime truth and Raft state must share one WAL frame"
            );

            let native = NativeEngine::open(directory.path()).map_err(test_storage_error)?;
            assert_eq!(native.runtime_cursor().map_err(test_storage_error)?, 2);
            assert_eq!(
                native
                    .runtime_commit_outcome(&outcome.commit_id)
                    .map_err(test_storage_error)?,
                Some(outcome.clone())
            );
            drop(native);

            let target_directory = tempfile::tempdir().map_err(test_storage_error)?;
            let (mut target_log, mut target_state) =
                VyrmRaftStore::open(target_directory.path(), ShardId(12))
                    .map_err(test_storage_error)?;
            let target_vote = Vote::new(2, 99);
            target_log.save_vote(&target_vote).await?;

            let mut corrupt_data = snapshot_data.clone();
            let middle = corrupt_data.len() / 2;
            corrupt_data[middle] ^= 0x40;
            let corrupt_snapshot = snapshot_handle(target_directory.path(), &corrupt_data).await?;
            assert!(target_state
                .install_snapshot(&snapshot_meta, corrupt_snapshot)
                .await
                .is_err());
            assert_eq!(target_state.applied_state().await?.0, None);
            assert_eq!(target_log.read_vote().await?, Some(target_vote));

            let mut forged_meta = snapshot_meta.clone();
            forged_meta.snapshot_id.push_str("-forged");
            let forged_snapshot = snapshot_handle(target_directory.path(), &snapshot_data).await?;
            assert!(target_state
                .install_snapshot(&forged_meta, forged_snapshot)
                .await
                .is_err());
            assert_eq!(target_state.applied_state().await?.0, None);
            let unhydrated_snapshot =
                snapshot_handle(target_directory.path(), &snapshot_data).await?;
            let unhydrated = target_state
                .install_snapshot(&snapshot_meta, unhydrated_snapshot)
                .await
                .unwrap_err();
            assert!(unhydrated.to_string().contains("object missing"));
            assert_eq!(target_state.applied_state().await?.0, None);
            let target_artifacts =
                LocalObjectStore::open(target_directory.path().join("application-objects"))
                    .map_err(test_storage_error)?;
            let mut incomplete_plan = transfer.plan.clone();
            incomplete_plan.artifact_digests.clear();
            let incomplete = ArtifactTransferManifest::new(
                incomplete_plan,
                transfer.scope.clone(),
                transfer.read.clone(),
                Vec::new(),
            )
            .map_err(test_storage_error)?;
            let incomplete_snapshot =
                snapshot_handle(target_directory.path(), &snapshot_data).await?;
            assert!(target_state
                .install_snapshot_with_artifacts(
                    &snapshot_meta,
                    incomplete_snapshot,
                    &source_artifacts,
                    &target_artifacts,
                    &incomplete,
                    19,
                )
                .await
                .is_err());
            assert_eq!(target_state.applied_state().await?.0, None);
            assert!(target_artifacts
                .verify(&transfer.objects[0].sha256)
                .is_err());
            let missing_source =
                LocalObjectStore::open(target_directory.path().join("intentionally-empty-source"))
                    .map_err(test_storage_error)?;
            let missing_snapshot = snapshot_handle(target_directory.path(), &snapshot_data).await?;
            assert!(target_state
                .install_snapshot_with_artifacts(
                    &snapshot_meta,
                    missing_snapshot,
                    &missing_source,
                    &target_artifacts,
                    &transfer,
                    20,
                )
                .await
                .is_err());
            assert_eq!(target_state.applied_state().await?.0, None);

            let first_install = snapshot_handle(target_directory.path(), &snapshot_data).await?;
            let artifact_receipt = target_state
                .install_snapshot_with_artifacts(
                    &snapshot_meta,
                    first_install,
                    &source_artifacts,
                    &target_artifacts,
                    &transfer,
                    21,
                )
                .await?;
            assert_eq!(artifact_receipt.transferred_objects, 1);
            assert_eq!(
                artifact_receipt.transferred_bytes,
                artifact_bytes.len() as u64
            );
            let idempotent_install =
                snapshot_handle(target_directory.path(), &snapshot_data).await?;
            target_state
                .install_snapshot(&snapshot_meta, idempotent_install)
                .await?;
            assert_eq!(target_log.read_vote().await?, Some(target_vote));
            assert_eq!(target_state.applied_state().await?.0, Some(retry_log));
            assert_eq!(
                target_state
                    .get_current_snapshot()
                    .await?
                    .expect("installed snapshot must be durable")
                    .meta,
                snapshot_meta
            );
            let after_snapshot_log = LogId::new(CommittedLeaderId::new(4, 1), 6);
            target_state
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: after_snapshot_log,
                    payload: EntryPayload::Blank,
                }])
                .await?;
            let stale_install = snapshot_handle(target_directory.path(), &snapshot_data).await?;
            assert!(target_state
                .install_snapshot(&snapshot_meta, stale_install)
                .await
                .is_err());
            assert_eq!(
                target_state.applied_state().await?.0,
                Some(after_snapshot_log)
            );
            drop(target_state);
            drop(target_log);
            let target_native =
                NativeEngine::open(target_directory.path()).map_err(test_storage_error)?;
            assert_eq!(
                target_native.runtime_cursor().map_err(test_storage_error)?,
                2
            );
            assert_eq!(
                target_native
                    .runtime_commit_outcome(&outcome.commit_id)
                    .map_err(test_storage_error)?,
                Some(outcome.clone())
            );
            let target_objects = target_native
                .runtime_changes_since(
                    0,
                    usize::MAX,
                    Some(&ScopeId::new("cluster:atomic").map_err(test_storage_error)?),
                )
                .map_err(test_storage_error)?
                .changes
                .into_iter()
                .filter_map(|change| match change.mutation {
                    RuntimeMutation::Object { object } => Some(object),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(target_objects.len(), 1);
            assert_eq!(
                target_artifacts
                    .get(&target_objects[0])
                    .map_err(test_storage_error)?,
                artifact_bytes
            );
            drop(target_native);
            let (mut target_log, mut target_state) =
                VyrmRaftStore::open(target_directory.path(), ShardId(12))
                    .map_err(test_storage_error)?;
            assert_eq!(target_log.read_vote().await?, Some(target_vote));
            assert_eq!(
                target_state.applied_state().await?.0,
                Some(after_snapshot_log)
            );
            assert_eq!(
                target_state
                    .get_current_snapshot()
                    .await?
                    .expect("snapshot cache must survive reopen")
                    .meta,
                snapshot_meta
            );

            let (_, mut reopened) =
                VyrmRaftStore::open(directory.path(), ShardId(12)).map_err(test_storage_error)?;
            assert_eq!(reopened.applied_state().await?.0, Some(retry_log));
            Ok::<(), StorageError<u64>>(())
        })
        .unwrap();
}

#[test]
fn stale_runtime_cursor_is_a_durable_denial_not_a_partial_apply() {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            let directory = tempfile::tempdir().map_err(test_storage_error)?;
            let (log_store, mut state_machine) =
                VyrmRaftStore::open(directory.path(), ShardId(13)).map_err(test_storage_error)?;
            state_machine
                .apply([test_membership_entry(LogId::new(
                    CommittedLeaderId::new(1, 1),
                    1,
                ))])
                .await?;
            let transition = VyrmRaftCommand::placement_transition(
                "placement-1",
                test_placement(ShardId(13), 1),
                Some(1),
            )
            .map_err(test_storage_error)?;
            state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 2),
                    payload: EntryPayload::Normal(transition),
                }])
                .await?;
            let first = VyrmRaftCommand::runtime_commit(
                "runtime-first",
                ShardId(13),
                1,
                None,
                bootstrap_runtime_commit("cluster:denial", 0),
            )
            .map_err(test_storage_error)?;
            state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 3),
                    payload: EntryPayload::Normal(first),
                }])
                .await?;

            let mut stale_commit = bootstrap_runtime_commit("cluster:denial", 0);
            stale_commit.actor = "agent:stale-writer".into();
            let stale = VyrmRaftCommand::runtime_commit(
                "runtime-stale",
                ShardId(13),
                1,
                None,
                stale_commit,
            )
            .map_err(test_storage_error)?;
            let denied_log = LogId::new(CommittedLeaderId::new(1, 1), 4);
            let denied = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: denied_log,
                    payload: EntryPayload::Normal(stale),
                }])
                .await?;
            assert!(!denied[0].accepted);
            assert!(denied[0].runtime_outcome.is_none());
            assert!(denied[0].reason.contains("runtime commit conflict"));
            drop(state_machine);
            drop(log_store);

            let native = NativeEngine::open(directory.path()).map_err(test_storage_error)?;
            assert_eq!(native.runtime_cursor().map_err(test_storage_error)?, 1);
            drop(native);
            let (_, mut reopened) =
                VyrmRaftStore::open(directory.path(), ShardId(13)).map_err(test_storage_error)?;
            assert_eq!(reopened.applied_state().await?.0, Some(denied_log));
            Ok::<(), StorageError<u64>>(())
        })
        .unwrap();
}

#[test]
fn store_is_permanently_bound_to_one_shard() {
    let directory = tempfile::tempdir().unwrap();
    VyrmRaftStore::open(directory.path(), ShardId(2)).unwrap();
    let error = match VyrmRaftStore::open(directory.path(), ShardId(3)) {
        Ok(_) => panic!("foreign shard binding unexpectedly opened"),
        Err(error) => error,
    };
    assert!(matches!(error, ClusterError::Denied(_)));
}

#[test]
fn legacy_or_ambiguous_adapter_domains_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = Database::create(directory.path()).unwrap();
    database
        .write_owned(
            WriteBatch::new(vec![Mutation::Put {
                key: b"vyrm/raft/v2/meta/config".to_vec(),
                value: br#"{"format_version":2,"shard":2}"#.to_vec(),
            }])
            .unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    drop(database);
    let error = match VyrmRaftStore::open(directory.path(), ShardId(2)) {
        Ok(_) => panic!("legacy single-domain adapter unexpectedly opened as v3"),
        Err(error) => error,
    };
    assert!(matches!(error, ClusterError::Denied(_)));
}

fn test_storage_error(error: impl std::fmt::Display) -> StorageError<u64> {
    StorageError::from_io_error(
        ErrorSubject::Store,
        ErrorVerb::Write,
        io::Error::other(error.to_string()),
    )
}

fn bootstrap_runtime_commit(scope: &str, expected_cursor: u64) -> RuntimeCommit {
    let mut registry = RuntimeSchemaRegistry::empty(1, "cluster bootstrap");
    registry.records.insert(
        RuntimeType::new("reasoning_run").unwrap(),
        RuntimeRecordSchema::default(),
    );
    RuntimeCommit {
        scope: ScopeId::new(scope).unwrap(),
        at: 1,
        actor: "agent:cluster-test".into(),
        expected_cursor,
        mutations: vec![RuntimeMutation::Schema { registry }],
    }
}

fn test_membership_entry(log_id: LogId<u64>) -> Entry<VyrmRaftTypeConfig> {
    test_membership_entry_with_first_zone(log_id, "az-1")
}

fn test_membership_entry_with_first_zone(
    log_id: LogId<u64>,
    first_zone: &str,
) -> Entry<VyrmRaftTypeConfig> {
    let nodes = (1..=3)
        .map(|id| {
            (
                id,
                VyrmRaftNode {
                    canonical_id: format!("node-{id}"),
                    zone: if id == 1 {
                        first_zone.to_owned()
                    } else {
                        format!("az-{id}")
                    },
                    endpoint: format!("in-process://node-{id}"),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    Entry {
        log_id,
        payload: EntryPayload::Membership(Membership::new(vec![BTreeSet::from([1, 2, 3])], nodes)),
    }
}

fn test_placement(shard: ShardId, epoch: u64) -> ShardPlacement {
    ShardPlacement {
        contract_version: CLUSTER_CONTRACT_VERSION,
        cluster: ClusterId::new("cluster:storage-test").unwrap(),
        shard,
        epoch,
        policy: PlacementPolicy {
            voter_count: 3,
            minimum_voter_zones: 3,
            maximum_voters_per_zone: 1,
        },
        replicas: (1..=3)
            .map(|id| ReplicaPlacement {
                node: NodeId::new(format!("node-{id}")).unwrap(),
                zone: ZoneId::new(format!("az-{id}")).unwrap(),
                role: ReplicaRole::Voter,
            })
            .collect(),
    }
}
