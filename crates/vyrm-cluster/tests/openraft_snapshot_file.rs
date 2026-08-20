#![cfg(feature = "openraft-adapter")]

use openraft::storage::RaftStateMachine;
use std::io::SeekFrom;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use vyrm_cluster::{ClusterError, ShardId, VyrmRaftStore, VyrmSnapshotData};
use vyrm_kv::SNAPSHOT_BUNDLE_MAX_BYTES;

#[test]
fn ephemeral_snapshot_files_are_bounded_and_removed_on_drop() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("receive.spool");
        let mut snapshot = VyrmSnapshotData::create_ephemeral(&path).await.unwrap();
        snapshot.write_all(b"bounded snapshot").await.unwrap();
        assert!(path.is_file());

        snapshot
            .seek(SeekFrom::Start(SNAPSHOT_BUNDLE_MAX_BYTES))
            .await
            .unwrap();
        let error = snapshot.write_all(b"x").await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge);
        drop(snapshot);
        assert!(!path.exists());
    });
}

#[test]
fn reopen_reclaims_abandoned_spools_and_denies_ambiguous_entries() {
    let directory = tempfile::tempdir().unwrap();
    let (_, mut state) = VyrmRaftStore::open(directory.path(), ShardId(91)).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let abandoned_path = runtime.block_on(async {
        let snapshot = state.begin_receiving_snapshot().await.unwrap();
        let path = snapshot.path().to_path_buf();
        std::mem::forget(snapshot);
        path
    });
    assert!(abandoned_path.is_file());
    drop(state);

    let (_, state) = VyrmRaftStore::open(directory.path(), ShardId(91)).unwrap();
    assert!(!abandoned_path.exists());
    drop(state);

    let spool = directory
        .path()
        .join("raft-local-v4")
        .join("snapshot-spool");
    std::fs::create_dir(spool.join("unexpected-directory")).unwrap();
    let error = match VyrmRaftStore::open(directory.path(), ShardId(91)) {
        Ok(_) => panic!("ambiguous spool entry unexpectedly accepted"),
        Err(error) => error,
    };
    assert!(matches!(error, ClusterError::Denied(_)));
}
