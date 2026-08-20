use vyrm_kv::{
    CompactionBoundary, Database, Durability, Error, FailureMode, FlushBoundary, Mutation,
    WriteBatch,
};

fn put(key: &str, value: &str) -> WriteBatch {
    WriteBatch::new(vec![Mutation::Put {
        key: key.as_bytes().to_vec(),
        value: value.as_bytes().to_vec(),
    }])
    .unwrap()
}

#[test]
fn every_compaction_boundary_recovers_after_crash_and_storage_full() {
    let boundaries = [
        CompactionBoundary::SegmentSynced,
        CompactionBoundary::ManifestPublished,
    ];
    let modes = [FailureMode::Crash, FailureMode::StorageFull];

    for mode in modes {
        for boundary in boundaries {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("database");
            let mut database = Database::create(&root).unwrap();
            database
                .write(&put("key", "old"), Durability::Authoritative)
                .unwrap();
            database.flush_memtable(10).unwrap();
            database
                .write(&put("key", "new"), Durability::Authoritative)
                .unwrap();
            database.flush_memtable(20).unwrap();
            let snapshot = database.snapshot();
            let before = database.manifest().generation;
            let error = database
                .compact_with_failure(&[], 30, boundary, mode)
                .unwrap_err();
            assert!(matches!(error, Error::InjectedFailure { .. }));
            drop(database);

            let mut recovered = Database::open(&root).unwrap();
            assert_eq!(
                recovered.get(b"key", snapshot).unwrap().as_deref(),
                Some(b"new".as_slice())
            );
            let published = boundary == CompactionBoundary::ManifestPublished;
            assert_eq!(
                recovered.manifest().generation,
                before + u64::from(published)
            );
            if !published {
                recovered.compact(&[], 40).unwrap();
            }
            recovered.garbage_collect().unwrap();
            drop(recovered);

            let reopened = Database::open(&root).unwrap();
            assert_eq!(
                reopened.get(b"key", snapshot).unwrap().as_deref(),
                Some(b"new".as_slice())
            );
        }
    }
}

#[test]
fn every_flush_boundary_recovers_after_crash_and_storage_full() {
    let boundaries = [
        FlushBoundary::WalSynced,
        FlushBoundary::SegmentSynced,
        FlushBoundary::SuccessorWalSynced,
        FlushBoundary::ManifestPublished,
    ];
    let modes = [FailureMode::Crash, FailureMode::StorageFull];

    for mode in modes {
        for boundary in boundaries {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("database");
            let mut database = Database::create(&root).unwrap();
            database
                .write(&put("alpha", "one"), Durability::Authoritative)
                .unwrap();
            let snapshot = database.snapshot();
            let error = database
                .flush_memtable_with_failure(10, boundary, mode)
                .unwrap_err();
            assert!(matches!(error, Error::InjectedFailure { .. }));
            drop(database);

            let mut recovered = Database::open(&root).unwrap();
            assert_eq!(
                recovered.get(b"alpha", snapshot).unwrap().as_deref(),
                Some(b"one".as_slice()),
                "lost value for {mode:?} at {boundary:?}"
            );
            let published = boundary == FlushBoundary::ManifestPublished;
            assert_eq!(
                recovered.manifest().generation,
                if published { 2 } else { 1 }
            );

            if !published {
                recovered.flush_memtable(20).unwrap();
                assert_eq!(recovered.manifest().generation, 2);
            }
            recovered
                .write(&put("beta", "two"), Durability::Authoritative)
                .unwrap();
            let current = recovered.snapshot();
            assert_eq!(
                recovered.get(b"beta", current).unwrap().as_deref(),
                Some(b"two".as_slice())
            );
            drop(recovered);

            let reopened = Database::open(&root).unwrap();
            assert_eq!(
                reopened.get(b"alpha", snapshot).unwrap().as_deref(),
                Some(b"one".as_slice())
            );
            assert_eq!(
                reopened.get(b"beta", current).unwrap().as_deref(),
                Some(b"two".as_slice())
            );
        }
    }
}
