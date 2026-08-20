use vyrm_kv::{Database, Durability, Mutation, WriteBatch};

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
