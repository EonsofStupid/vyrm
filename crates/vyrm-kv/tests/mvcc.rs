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
    assert_eq!(database.get(b"key", old), Some(b"old".as_slice()));
    assert_eq!(database.get(b"key", current), Some(b"new".as_slice()));
    assert_eq!(database.sync().unwrap(), current.sequence);
    drop(database);

    let reopened = Database::open(&root).unwrap();
    assert_eq!(reopened.snapshot(), current);
    assert_eq!(reopened.get(b"key", old), Some(b"old".as_slice()));
    assert_eq!(reopened.get(b"key", current), Some(b"new".as_slice()));
}
