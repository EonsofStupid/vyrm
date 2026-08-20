use vyrm_kv::{
    recover, Database, Durability, Error, Memtable, Mutation, RecoveredBatch, WalWriter, WriteBatch,
};

fn fixture_batch() -> WriteBatch {
    WriteBatch::new(vec![
        Mutation::Put {
            key: b"alpha".to_vec(),
            value: b"one".to_vec(),
        },
        Mutation::Put {
            key: b"beta".to_vec(),
            value: b"two".to_vec(),
        },
        Mutation::Delete {
            key: b"alpha".to_vec(),
        },
    ])
    .unwrap()
}

#[test]
fn batch_codec_is_canonical_strict_and_frozen() {
    let batch = fixture_batch();
    let encoded = batch.encode().unwrap();
    assert_eq!(WriteBatch::decode(&encoded).unwrap(), batch);

    for end in 0..encoded.len() {
        assert!(WriteBatch::decode(&encoded[..end]).is_err());
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(matches!(
        WriteBatch::decode(&trailing),
        Err(Error::InvalidBatch(_))
    ));

    let actual = encoded
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/batch-v1.hex");
    if std::env::var_os("VYRM_UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(format!("{}/fixtures", env!("CARGO_MANIFEST_DIR"))).unwrap();
        std::fs::write(fixture, format!("{actual}\n")).unwrap();
    }
    assert_eq!(
        format!("{actual}\n"),
        std::fs::read_to_string(fixture).unwrap()
    );
}

#[test]
fn wal_allocates_one_sequence_per_operation_and_memtable_preserves_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("active.wal");
    let mut writer = WalWriter::create(&path).unwrap();
    let receipt = writer
        .append_write_batch(&fixture_batch(), Durability::Authoritative)
        .unwrap();
    assert_eq!((receipt.first_sequence, receipt.last_sequence), (1, 3));

    let update = WriteBatch::new(vec![
        Mutation::Put {
            key: b"alpha".to_vec(),
            value: b"three".to_vec(),
        },
        Mutation::Delete {
            key: b"beta".to_vec(),
        },
    ])
    .unwrap();
    let receipt = writer
        .append_write_batch(&update, Durability::Authoritative)
        .unwrap();
    assert_eq!((receipt.first_sequence, receipt.last_sequence), (4, 5));
    drop(writer);

    let recovery = recover(&path).unwrap();
    let table = Memtable::recover(&recovery.batches).unwrap();
    assert_eq!(table.maximum_sequence(), 5);
    assert_eq!(table.key_count(), 2);
    assert_eq!(table.version_count(), 5);
    assert_eq!(table.get(b"alpha", 1), Some(b"one".as_slice()));
    assert_eq!(table.get(b"alpha", 3), None);
    assert_eq!(table.get(b"alpha", 4), Some(b"three".as_slice()));
    assert_eq!(table.get(b"beta", 4), Some(b"two".as_slice()));
    assert_eq!(table.get(b"beta", 5), None);
    assert_eq!(
        table.scan(b"a", Some(b"z"), 2),
        vec![
            (b"alpha".to_vec(), b"one".to_vec()),
            (b"beta".to_vec(), b"two".to_vec()),
        ]
    );
    assert_eq!(
        table.scan(b"a", Some(b"z"), 5),
        vec![(b"alpha".to_vec(), b"three".to_vec())]
    );
    assert!(table.approximate_bytes() > 0);
}

#[test]
fn a_bad_recovered_batch_cannot_partially_change_the_memtable() {
    let valid = fixture_batch().encode().unwrap();
    let first = RecoveredBatch {
        offset: 16,
        first_sequence: 1,
        last_sequence: 3,
        checksum: 0,
        payload: valid.clone(),
    };
    let mut table = Memtable::default();
    table.apply(&first).unwrap();
    let before = table.clone();

    let wrong_range = RecoveredBatch {
        offset: 64,
        first_sequence: 4,
        last_sequence: 9,
        checksum: 0,
        payload: valid,
    };
    assert!(matches!(
        table.apply(&wrong_range),
        Err(Error::InvalidBatch(_))
    ));
    assert_eq!(table, before);

    let corrupt_payload = RecoveredBatch {
        offset: 64,
        first_sequence: 4,
        last_sequence: 4,
        checksum: 0,
        payload: b"not-a-batch".to_vec(),
    };
    assert!(table.apply(&corrupt_payload).is_err());
    assert_eq!(table, before);
}

#[test]
fn database_snapshots_are_repeatable_across_writes_and_reopen() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("native");
    let mut database = Database::create(&root).unwrap();
    database
        .write(
            &WriteBatch::new(vec![Mutation::Put {
                key: b"key".to_vec(),
                value: b"old".to_vec(),
            }])
            .unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    let old = database.snapshot();
    database
        .write(
            &WriteBatch::new(vec![Mutation::Put {
                key: b"key".to_vec(),
                value: b"new".to_vec(),
            }])
            .unwrap(),
            Durability::Buffered,
        )
        .unwrap();
    let current = database.snapshot();
    assert_eq!(
        database.get(b"key", old).unwrap().as_deref(),
        Some(b"old".as_slice())
    );
    assert_eq!(
        database.get(b"key", current).unwrap().as_deref(),
        Some(b"new".as_slice())
    );
    assert_eq!(database.sync().unwrap(), current.sequence);
    drop(database);

    let reopened = Database::open(&root).unwrap();
    assert_eq!(reopened.snapshot(), current);
    assert_eq!(
        reopened.get(b"key", old).unwrap().as_deref(),
        Some(b"old".as_slice())
    );
    assert_eq!(
        reopened.get(b"key", current).unwrap().as_deref(),
        Some(b"new".as_slice())
    );
}

#[test]
fn batch_point_reads_match_individual_reads_across_segments_memtable_and_snapshots() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("native");
    let mut database = Database::create(&root).unwrap();
    database
        .write(
            &WriteBatch::new(vec![
                Mutation::Put {
                    key: b"alpha".to_vec(),
                    value: b"alpha-old".to_vec(),
                },
                Mutation::Put {
                    key: b"beta".to_vec(),
                    value: b"beta-old".to_vec(),
                },
            ])
            .unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    let old = database.snapshot();
    database.flush_memtable(old.sequence).unwrap();
    database
        .write(
            &WriteBatch::new(vec![
                Mutation::Delete {
                    key: b"alpha".to_vec(),
                },
                Mutation::Put {
                    key: b"beta".to_vec(),
                    value: b"beta-new".to_vec(),
                },
                Mutation::Put {
                    key: b"gamma".to_vec(),
                    value: b"gamma-new".to_vec(),
                },
            ])
            .unwrap(),
            Durability::Buffered,
        )
        .unwrap();
    let current = database.snapshot();
    let keys = vec![
        b"gamma".to_vec(),
        b"alpha".to_vec(),
        b"missing".to_vec(),
        b"beta".to_vec(),
        b"beta".to_vec(),
    ];

    for snapshot in [old, current] {
        let expected = keys
            .iter()
            .map(|key| database.get(key, snapshot).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(database.get_many(&keys, snapshot).unwrap(), expected);
    }

    database.flush_memtable(current.sequence).unwrap();
    database.compact(&[], current.sequence).unwrap();
    for snapshot in [old, current] {
        let expected = keys
            .iter()
            .map(|key| database.get(key, snapshot).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(database.get_many(&keys, snapshot).unwrap(), expected);
    }
}

#[test]
fn flush_rotates_wal_publishes_manifest_and_preserves_old_snapshots() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("native");
    let mut database = Database::create(&root).unwrap();
    database
        .write(
            &WriteBatch::new(vec![Mutation::Put {
                key: b"key".to_vec(),
                value: b"v1".to_vec(),
            }])
            .unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    let first_snapshot = database.snapshot();
    let first_manifest = database.flush_memtable(10).unwrap().unwrap();
    assert_eq!(first_manifest.generation, 2);
    assert_eq!(first_manifest.durable_sequence, 1);
    assert_eq!(first_manifest.wal_start_sequence, 2);
    assert_eq!(first_manifest.segments.len(), 1);
    assert_eq!(database.memtable().version_count(), 0);
    database.checkpoint("after-first-flush", 10).unwrap();

    database
        .write(
            &WriteBatch::new(vec![Mutation::Put {
                key: b"key".to_vec(),
                value: b"v2".to_vec(),
            }])
            .unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    let second_snapshot = database.snapshot();
    assert_eq!(
        database.get(b"key", first_snapshot).unwrap().as_deref(),
        Some(b"v1".as_slice())
    );
    assert_eq!(
        database.get(b"key", second_snapshot).unwrap().as_deref(),
        Some(b"v2".as_slice())
    );
    let second_manifest = database.flush_memtable(11).unwrap().unwrap();
    assert_eq!(second_manifest.generation, 3);
    assert_eq!(second_manifest.durable_sequence, 2);
    assert_eq!(second_manifest.wal_start_sequence, 3);
    assert_eq!(second_manifest.segments.len(), 2);
    assert!(database.flush_memtable(12).unwrap().is_none());
    drop(database);

    let reopened = Database::open(&root).unwrap();
    assert_eq!(reopened.manifest(), &second_manifest);
    assert_eq!(reopened.snapshot(), second_snapshot);
    assert_eq!(
        reopened.get(b"key", first_snapshot).unwrap().as_deref(),
        Some(b"v1".as_slice())
    );
    assert_eq!(
        reopened.get(b"key", second_snapshot).unwrap().as_deref(),
        Some(b"v2".as_slice())
    );
    assert_eq!(reopened.checkpoints().unwrap().len(), 1);
    assert!(root.join("wal/00000000000000000001.wal").exists());
    assert!(root.join("wal/00000000000000000002.wal").exists());
    assert!(root.join("wal/00000000000000000003.wal").exists());
}
