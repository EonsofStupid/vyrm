#![cfg(feature = "openraft-adapter")]

use openraft::storage::RaftStateMachine;
use openraft::testing::{StoreBuilder, Suite};
use openraft::{
    CommittedLeaderId, Entry, EntryPayload, ErrorSubject, ErrorVerb, LogId, StorageError,
};
use std::io;
use tempfile::TempDir;
use vyrm_cluster::{
    ClusterError, ShardId, VyrmRaftCommand, VyrmRaftLogStore, VyrmRaftStateMachine, VyrmRaftStore,
    VyrmRaftTypeConfig,
};

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
