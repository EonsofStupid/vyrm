#![cfg(feature = "openraft-adapter")]

use openraft::storage::RaftStateMachine;
use openraft::testing::{StoreBuilder, Suite};
use openraft::{
    CommittedLeaderId, Entry, EntryPayload, ErrorSubject, ErrorVerb, LogId, RaftSnapshotBuilder,
    StorageError,
};
use std::io;
use tempfile::TempDir;
use vyrm_cluster::{
    ClusterError, ShardId, VyrmRaftCommand, VyrmRaftLogStore, VyrmRaftStateMachine, VyrmRaftStore,
    VyrmRaftTypeConfig,
};
use vyrm_core::{
    RuntimeCommit, RuntimeMutation, RuntimeRecordSchema, RuntimeSchemaRegistry, RuntimeType,
    ScopeId,
};
use vyrm_kv::{recover, WriteBatch};
use vyrm_store::{Engine, NativeEngine};

struct VyrmStoreBuilder;

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
                3,
                Some(0),
                b"canonical-runtime-commit".to_vec(),
            )
            .map_err(test_storage_error)?;
            let log_id = LogId::new(CommittedLeaderId::new(2, 11), 1);

            let (_, mut state_machine) =
                VyrmRaftStore::open(directory.path(), ShardId(7)).map_err(test_storage_error)?;
            let responses = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id,
                    payload: EntryPayload::Normal(command.clone()),
                }])
                .await?;
            assert!(responses[0].accepted);
            assert!(!responses[0].duplicate);
            let original_digest = responses[0].state_digest.clone();
            drop(state_machine);

            let (_, mut reopened) =
                VyrmRaftStore::open(directory.path(), ShardId(7)).map_err(test_storage_error)?;
            assert_eq!(reopened.applied_state().await?.0, Some(log_id));
            let duplicate_log = LogId::new(CommittedLeaderId::new(3, 12), 2);
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

            let first =
                VyrmRaftCommand::new("epoch-anchor", ShardId(9), 4, None, b"first".to_vec())
                    .map_err(test_storage_error)?;
            let first_response = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 1),
                    payload: EntryPayload::Normal(first),
                }])
                .await?;
            assert!(first_response[0].accepted);

            let wrong_epoch =
                VyrmRaftCommand::new("wrong-epoch", ShardId(9), 5, None, b"second".to_vec())
                    .map_err(test_storage_error)?;
            let denied = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 2),
                    payload: EntryPayload::Normal(wrong_epoch),
                }])
                .await?;
            assert!(!denied[0].accepted);
            assert!(denied[0].reason.contains("placement epoch"));

            let original =
                VyrmRaftCommand::new("identity", ShardId(9), 4, Some(2), b"same-payload".to_vec())
                    .map_err(test_storage_error)?;
            let accepted = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 3),
                    payload: EntryPayload::Normal(original),
                }])
                .await?;
            assert!(accepted[0].accepted);

            let changed_cas =
                VyrmRaftCommand::new("identity", ShardId(9), 4, None, b"same-payload".to_vec())
                    .map_err(test_storage_error)?;
            let reused = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 4),
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
fn canonical_runtime_commit_is_atomic_idempotent_durable_and_snapshot_safe() {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            let directory = tempfile::tempdir().map_err(test_storage_error)?;
            let (log_store, mut state_machine) =
                VyrmRaftStore::open(directory.path(), ShardId(12)).map_err(test_storage_error)?;
            let commit = bootstrap_runtime_commit("cluster:atomic", 0);
            let command = VyrmRaftCommand::runtime_commit(
                "runtime-1",
                ShardId(12),
                6,
                Some(0),
                commit.clone(),
            )
            .map_err(test_storage_error)?;
            let first_log = LogId::new(CommittedLeaderId::new(4, 1), 1);
            let first = state_machine
                .apply([Entry::<VyrmRaftTypeConfig> {
                    log_id: first_log,
                    payload: EntryPayload::Normal(command.clone()),
                }])
                .await?;
            assert!(first[0].accepted);
            let outcome = first[0].runtime_outcome.clone().unwrap();
            assert_eq!(outcome.commit_id, commit.digest());
            assert_eq!(outcome.last_cursor, 1);

            let duplicate_log = LogId::new(CommittedLeaderId::new(4, 1), 2);
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
                VyrmRaftCommand::runtime_commit("runtime-2", ShardId(12), 6, None, commit)
                    .map_err(test_storage_error)?;
            let retry_log = LogId::new(CommittedLeaderId::new(4, 1), 3);
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

            let mut builder = state_machine.get_snapshot_builder().await;
            let snapshot_error = builder.build_snapshot().await.unwrap_err();
            assert!(snapshot_error.to_string().contains("transferable VyrmKV"));
            drop(builder);
            drop(state_machine);
            drop(log_store);

            let wal = recover(&directory.path().join("wal/00000000000000000001.wal"))
                .map_err(test_storage_error)?;
            let atomic_frame = wal.batches.iter().any(|batch| {
                let decoded = WriteBatch::decode(&batch.payload).unwrap();
                let has_raft_state = decoded
                    .operations
                    .iter()
                    .any(|operation| operation.key() == b"vyrm/raft/v2/state/current");
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
            assert_eq!(native.runtime_cursor().map_err(test_storage_error)?, 1);
            assert_eq!(
                native
                    .runtime_commit_outcome(&outcome.commit_id)
                    .map_err(test_storage_error)?,
                Some(outcome)
            );
            drop(native);

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
                    log_id: LogId::new(CommittedLeaderId::new(1, 1), 1),
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
            let denied_log = LogId::new(CommittedLeaderId::new(1, 1), 2);
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
