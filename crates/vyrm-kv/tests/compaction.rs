use vyrm_kv::{CompactionPolicy, Database, DatabaseOptions, Durability, Mutation, WriteBatch};

fn batch(operations: Vec<Mutation>) -> WriteBatch {
    WriteBatch::new(operations).unwrap()
}

fn put(key: &str, value: &str) -> Mutation {
    Mutation::Put {
        key: key.as_bytes().to_vec(),
        value: value.as_bytes().to_vec(),
    }
}

fn delete(key: &str) -> Mutation {
    Mutation::Delete {
        key: key.as_bytes().to_vec(),
    }
}

#[test]
fn compaction_retains_only_versions_visible_to_protected_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("database");
    let mut database = Database::create(&root).unwrap();

    database
        .write(
            &batch(vec![put("key", "v1"), put("removed", "present")]),
            Durability::Authoritative,
        )
        .unwrap();
    let first = database.snapshot();
    database.flush_memtable(10).unwrap();
    database
        .write(
            &batch(vec![put("key", "v2"), delete("removed")]),
            Durability::Authoritative,
        )
        .unwrap();
    let second = database.snapshot();
    database.flush_memtable(20).unwrap();
    database
        .write(&batch(vec![put("key", "v3")]), Durability::Authoritative)
        .unwrap();
    let current = database.snapshot();

    let outcome = database.compact(&[first, second], 30).unwrap().unwrap();
    assert_eq!(outcome.input_segments, 3);
    assert_eq!(outcome.output_segments, 1);
    assert_eq!(outcome.input_versions, 5);
    assert_eq!(outcome.output_versions, 5);
    assert_eq!(outcome.protected_sequences, vec![2, 4, 5]);
    assert_eq!(
        database.get(b"key", first).unwrap().as_deref(),
        Some(b"v1".as_slice())
    );
    assert_eq!(
        database.get(b"key", second).unwrap().as_deref(),
        Some(b"v2".as_slice())
    );
    assert_eq!(
        database.get(b"key", current).unwrap().as_deref(),
        Some(b"v3".as_slice())
    );
    assert_eq!(
        database.get(b"removed", first).unwrap().as_deref(),
        Some(b"present".as_slice())
    );
    assert_eq!(database.get(b"removed", second).unwrap().as_deref(), None);

    drop(database);
    let reopened = Database::open(&root).unwrap();
    assert_eq!(
        reopened.get(b"key", first).unwrap().as_deref(),
        Some(b"v1".as_slice())
    );
    assert_eq!(
        reopened.get(b"key", second).unwrap().as_deref(),
        Some(b"v2".as_slice())
    );
    assert_eq!(
        reopened.get(b"key", current).unwrap().as_deref(),
        Some(b"v3".as_slice())
    );
    assert_eq!(reopened.get(b"removed", second).unwrap().as_deref(), None);
}

#[test]
fn compaction_prunes_unprotected_history_and_obsolete_tombstones() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("database");
    let mut database = Database::create(&root).unwrap();
    database
        .write(&batch(vec![put("key", "old")]), Durability::Authoritative)
        .unwrap();
    let old = database.snapshot();
    database.flush_memtable(10).unwrap();
    database
        .write(
            &batch(vec![put("key", "new"), put("gone", "value")]),
            Durability::Authoritative,
        )
        .unwrap();
    database.flush_memtable(20).unwrap();
    database
        .write(&batch(vec![delete("gone")]), Durability::Authoritative)
        .unwrap();
    let current = database.snapshot();

    let outcome = database.compact(&[], 30).unwrap().unwrap();
    assert_eq!(outcome.output_versions, 1);
    assert_eq!(database.get(b"key", old).unwrap().as_deref(), None);
    assert_eq!(
        database.get(b"key", current).unwrap().as_deref(),
        Some(b"new".as_slice())
    );
    assert_eq!(database.get(b"gone", current).unwrap().as_deref(), None);
    assert_eq!(database.manifest().segments[0].entries, 1);
}

#[test]
fn garbage_collection_preserves_current_and_checkpoint_reachability() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("database");
    let mut database = Database::create(&root).unwrap();
    database
        .write(&batch(vec![put("key", "v1")]), Durability::Authoritative)
        .unwrap();
    database.flush_memtable(10).unwrap();
    let checkpoint = database.checkpoint("keep-first", 11).unwrap();
    let first_segment = database.manifest().segments[0].id.clone();

    database
        .write(&batch(vec![put("key", "v2")]), Durability::Authoritative)
        .unwrap();
    database.flush_memtable(20).unwrap();
    database.compact(&[], 30).unwrap();
    let current_segment = database.manifest().segments[0].id.clone();

    let first = database.garbage_collect().unwrap();
    assert!(first.retained_manifests.contains(&checkpoint.manifest));
    assert!(first
        .retained_manifests
        .contains(&database.manifest().digest));
    assert!(first.retained_segments.contains(&first_segment));
    assert!(first.retained_segments.contains(&current_segment));
    assert!(root
        .join("segments")
        .join(format!("{first_segment}.seg"))
        .exists());
    assert!(root
        .join("segments")
        .join(format!("{current_segment}.seg"))
        .exists());
    assert!(!first.removed_wals.is_empty());

    assert!(database.release_checkpoint("keep-first").unwrap());
    let second = database.garbage_collect().unwrap();
    assert!(second
        .removed_segments
        .contains(&format!("{first_segment}.seg")));
    assert!(second
        .removed_manifests
        .contains(&format!("{}.json", checkpoint.manifest)));
    assert!(!root
        .join("segments")
        .join(format!("{first_segment}.seg"))
        .exists());
    assert!(root
        .join("segments")
        .join(format!("{current_segment}.seg"))
        .exists());
}

#[test]
fn explicit_compaction_promotes_a_single_segment() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("database");
    let mut database = Database::create(&root).unwrap();
    database
        .write(&batch(vec![put("key", "value")]), Durability::Authoritative)
        .unwrap();
    let snapshot = database.snapshot();
    database.flush_memtable(10).unwrap();

    let outcome = database.compact(&[snapshot], 20).unwrap().unwrap();

    assert_eq!(outcome.input_segments, 1);
    assert_eq!(outcome.output_segments, 1);
    assert_eq!(outcome.source_level, 0);
    assert_eq!(outcome.target_level, 1);
    assert_eq!(database.manifest().segments[0].level, 1);
    assert_eq!(
        database.get(b"key", snapshot).unwrap().as_deref(),
        Some(b"value".as_slice())
    );
}

#[test]
fn leveled_compaction_streams_into_key_partitioned_outputs() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("database");
    let mut database = Database::create_with_options(
        &root,
        DatabaseOptions {
            compaction: CompactionPolicy {
                target_segment_bytes: 512,
                ..CompactionPolicy::default()
            },
            ..DatabaseOptions::default()
        },
    )
    .unwrap();
    for phase in 0..3 {
        let operations = (0..32)
            .map(|index| {
                put(
                    &format!("key:{index:04}"),
                    &format!("phase:{phase}:{}", "x".repeat(48)),
                )
            })
            .collect();
        database
            .write(&batch(operations), Durability::Authoritative)
            .unwrap();
        database.flush_memtable(phase + 1).unwrap();
    }
    let snapshot = database.snapshot();
    let outcome = database.compact(&[], 10).unwrap().unwrap();
    assert_eq!(outcome.source_level, 0);
    assert_eq!(outcome.target_level, 1);
    assert!(outcome.output_segments > 1);
    assert!(outcome.peak_buffer_bytes <= 512);
    assert_eq!(outcome.input_versions, 96);
    assert_eq!(outcome.output_versions, 32);
    assert!(database
        .manifest()
        .segments
        .iter()
        .all(|segment| segment.level == 1));
    for pair in database.manifest().segments.windows(2) {
        assert!(pair[0].last_key < pair[1].first_key);
    }
    for index in 0..32 {
        let key = format!("key:{index:04}");
        assert_eq!(
            database.get(key.as_bytes(), snapshot).unwrap(),
            Some(format!("phase:2:{}", "x".repeat(48)).into_bytes())
        );
    }
}

#[test]
fn automatic_leveled_maintenance_preserves_untracked_snapshot_history() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("database");
    let mut database = Database::create_with_options(
        &root,
        DatabaseOptions {
            maintenance: vyrm_kv::MaintenancePolicy {
                wal_payload_max_bytes: usize::MAX,
                memtable_max_versions: 1,
            },
            compaction: CompactionPolicy {
                l0_compaction_trigger: 2,
                ..CompactionPolicy::default()
            },
            ..DatabaseOptions::default()
        },
    )
    .unwrap();
    database
        .write(&batch(vec![put("key", "v1")]), Durability::Authoritative)
        .unwrap();
    let old = database.snapshot();
    database
        .write(
            &batch(vec![put("other", "value")]),
            Durability::Authoritative,
        )
        .unwrap();
    database
        .write(&batch(vec![put("key", "v2")]), Durability::Authoritative)
        .unwrap();

    assert_eq!(database.maintenance_stats().automatic_compactions, 1);
    assert_eq!(
        database.get(b"key", old).unwrap().as_deref(),
        Some(b"v1".as_slice())
    );
    assert_eq!(
        database
            .get(b"key", database.snapshot())
            .unwrap()
            .as_deref(),
        Some(b"v2".as_slice())
    );
}
