use tempfile::tempdir;
use vyrm_kv::{
    Database, DatabaseOptions, Durability, Error, MaintenancePolicy, Mutation, WriteBatch,
};

fn put(key: &str, value: &str) -> Mutation {
    Mutation::Put {
        key: key.as_bytes().to_vec(),
        value: value.as_bytes().to_vec(),
    }
}

fn options(max_versions: usize) -> DatabaseOptions {
    DatabaseOptions {
        maintenance: MaintenancePolicy {
            wal_payload_max_bytes: usize::MAX,
            memtable_max_versions: max_versions,
        },
        ..DatabaseOptions::default()
    }
}

#[test]
fn the_next_writer_synchronously_flushes_before_crossing_the_mutable_bound() {
    let root = tempdir().unwrap().keep().join("native");
    let mut database = Database::create_with_options(&root, options(2)).unwrap();
    database
        .write_owned(
            WriteBatch::new(vec![put("a", "one"), put("b", "two")]).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    assert_eq!(database.manifest().durable_sequence, 0);
    assert_eq!(database.memtable().version_count(), 2);

    database
        .write_owned(
            WriteBatch::new(vec![put("c", "three")]).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();

    assert_eq!(database.manifest().durable_sequence, 2);
    assert_eq!(database.memtable().version_count(), 1);
    assert_eq!(database.maintenance_stats().automatic_flushes, 1);
    assert_eq!(database.maintenance_stats().write_stalls, 1);
    assert_eq!(database.maintenance_stats().failed_flushes, 0);
    let snapshot = database.snapshot();
    assert_eq!(
        database.get(b"a", snapshot).unwrap().as_deref(),
        Some(b"one".as_slice())
    );
    assert_eq!(
        database.get(b"c", snapshot).unwrap().as_deref(),
        Some(b"three".as_slice())
    );

    drop(database);
    let reopened = Database::open_with_options(&root, options(2)).unwrap();
    let snapshot = reopened.snapshot();
    assert_eq!(snapshot.sequence, 3);
    assert_eq!(
        reopened.get(b"b", snapshot).unwrap().as_deref(),
        Some(b"two".as_slice())
    );
    assert_eq!(
        reopened.get(b"c", snapshot).unwrap().as_deref(),
        Some(b"three".as_slice())
    );
}

#[test]
fn one_oversized_atomic_batch_is_retained_and_reported_without_splitting() {
    let root = tempdir().unwrap().keep().join("native");
    let mut database = Database::create_with_options(&root, options(1)).unwrap();
    database
        .write_owned(
            WriteBatch::new(vec![put("a", "one"), put("b", "two")]).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();

    assert_eq!(database.snapshot().sequence, 2);
    assert_eq!(database.memtable().version_count(), 2);
    assert_eq!(database.maintenance_stats().oversized_batches, 1);
    assert_eq!(database.maintenance_stats().automatic_flushes, 0);
}

#[test]
fn zero_maintenance_limits_fail_before_creating_storage() {
    let root = tempdir().unwrap().keep().join("native");
    let error = Database::create_with_options(
        &root,
        DatabaseOptions {
            maintenance: MaintenancePolicy {
                wal_payload_max_bytes: 0,
                memtable_max_versions: 1,
            },
            ..DatabaseOptions::default()
        },
    )
    .err()
    .expect("invalid policy must fail");
    assert!(matches!(error, Error::InvalidConfiguration(_)));
    assert!(!root.exists());
}
