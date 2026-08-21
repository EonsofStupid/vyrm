use std::io::{Seek, SeekFrom, Write};
use vyrm_kv::{Durability, Error, Memtable, Mutation, Segment, WalWriter, WriteBatch};

fn table() -> Memtable {
    let directory = tempfile::tempdir().unwrap();
    let wal_path = directory.path().join("active.wal");
    let mut wal = WalWriter::create(&wal_path).unwrap();
    let first = WriteBatch::new(vec![
        Mutation::Put {
            key: b"alpha".to_vec(),
            value: b"one".to_vec(),
        },
        Mutation::Put {
            key: b"beta".to_vec(),
            value: b"two".to_vec(),
        },
    ])
    .unwrap();
    wal.append_write_batch(&first, Durability::Authoritative)
        .unwrap();
    let second = WriteBatch::new(vec![
        Mutation::Delete {
            key: b"alpha".to_vec(),
        },
        Mutation::Put {
            key: b"beta".to_vec(),
            value: b"three".to_vec(),
        },
    ])
    .unwrap();
    wal.append_write_batch(&second, Durability::Authoritative)
        .unwrap();
    drop(wal);
    let recovery = vyrm_kv::recover(&wal_path).unwrap();
    Memtable::recover(&recovery.batches).unwrap()
}

#[test]
fn immutable_segment_preserves_mvcc_reads_and_content_identity() {
    let directory = tempfile::tempdir().unwrap();
    let segments = directory.path().join("segments");
    let (segment, path) = Segment::write_from_memtable(&segments, &table()).unwrap();
    assert!(path.ends_with(format!("{}.seg", segment.descriptor.id)));
    assert_eq!(segment.descriptor.minimum_sequence, 1);
    assert_eq!(segment.descriptor.maximum_sequence, 4);
    assert_eq!(segment.descriptor.entries, 4);
    assert_eq!(segment.get(b"alpha", 1).unwrap(), Some(b"one".to_vec()));
    assert_eq!(segment.get(b"alpha", 3).unwrap(), None);
    assert_eq!(segment.get(b"beta", 2).unwrap(), Some(b"two".to_vec()));
    assert_eq!(segment.get(b"beta", 4).unwrap(), Some(b"three".to_vec()));

    let reopened = Segment::open(&path).unwrap();
    assert_eq!(reopened.descriptor, segment.descriptor);
    assert_eq!(
        reopened.scan(b"a", Some(b"z"), 2).unwrap(),
        vec![
            (b"alpha".to_vec(), b"one".to_vec()),
            (b"beta".to_vec(), b"two".to_vec()),
        ]
    );
    let (deduplicated, same_path) = Segment::write_from_memtable(&segments, &table()).unwrap();
    assert_eq!(same_path, path);
    assert_eq!(deduplicated.descriptor, segment.descriptor);
}

#[test]
fn corruption_and_truncation_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let segments = directory.path().join("segments");
    let (_, path) = Segment::write_from_memtable(&segments, &table()).unwrap();
    let original = std::fs::read(&path).unwrap();

    let corrupt = directory.path().join("corrupt.seg");
    std::fs::write(&corrupt, &original).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&corrupt)
        .unwrap();
    file.seek(SeekFrom::Start(50)).unwrap();
    file.write_all(&[original[50] ^ 0x80]).unwrap();
    file.sync_all().unwrap();
    assert!(matches!(
        Segment::open(&corrupt),
        Err(Error::InvalidSegment(_))
    ));

    let truncated = directory.path().join("truncated.seg");
    std::fs::write(&truncated, &original[..original.len() - 1]).unwrap();
    assert!(matches!(
        Segment::open(&truncated),
        Err(Error::InvalidSegment(_))
    ));
}

#[test]
fn sparse_segment_matches_the_memtable_for_point_range_and_mvcc_reads() {
    let directory = tempfile::tempdir().unwrap();
    let wal_path = directory.path().join("many.wal");
    let mut wal = WalWriter::create(&wal_path).unwrap();
    for phase in 0..3 {
        let operations = (0..200)
            .map(|index| {
                let key = format!("key:{index:04}").into_bytes();
                if phase == 2 && index % 5 == 0 {
                    Mutation::Delete { key }
                } else {
                    Mutation::Put {
                        key,
                        value: format!("value:{phase}:{index:04}|").repeat(8).into_bytes(),
                    }
                }
            })
            .collect();
        wal.append_write_batch(
            &WriteBatch::new(operations).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    }
    drop(wal);
    let recovery = vyrm_kv::recover(&wal_path).unwrap();
    let table = Memtable::recover(&recovery.batches).unwrap();
    let (segment, path) =
        Segment::write_from_memtable(&directory.path().join("segments"), &table).unwrap();
    assert!(segment.block_count() >= 2);
    assert_eq!(&std::fs::read(&path).unwrap()[..8], b"VYRSEG03");
    assert!(std::fs::metadata(path).unwrap().len() < table.approximate_bytes() as u64);

    for snapshot in [1, 199, 200, 201, 399, 400, 401, 599, 600] {
        for index in 0..200 {
            let key = format!("key:{index:04}");
            assert_eq!(
                segment.get(key.as_bytes(), snapshot).unwrap().as_deref(),
                table.get(key.as_bytes(), snapshot),
                "point mismatch for {key} at sequence {snapshot}"
            );
        }
        for (start, end) in [
            (b"key:0000".as_slice(), Some(b"key:0200".as_slice())),
            (b"key:0063".as_slice(), Some(b"key:0131".as_slice())),
            (b"key:0190".as_slice(), None),
        ] {
            assert_eq!(
                segment.scan(start, end, snapshot).unwrap(),
                table.scan(start, end, snapshot),
                "range mismatch at sequence {snapshot}"
            );
        }
    }
}

fn rewrite_checksum(bytes: &mut Vec<u8>) {
    bytes.truncate(bytes.len() - 64);
    bytes.extend_from_slice(vyrm_core::digest::sha256_hex(bytes).as_bytes());
}

#[test]
fn v3_rejects_authenticated_length_flags_and_block_corruption() {
    let directory = tempfile::tempdir().unwrap();
    let segments = directory.path().join("segments");
    let (_, path) = Segment::write_from_memtable(&segments, &table()).unwrap();
    let original = std::fs::read(path).unwrap();

    let mut wrong_length = original.clone();
    wrong_length[40..48].copy_from_slice(&999u64.to_be_bytes());
    rewrite_checksum(&mut wrong_length);
    let wrong_length_path = directory.path().join("wrong-length.seg");
    std::fs::write(&wrong_length_path, wrong_length).unwrap();
    assert!(matches!(
        Segment::open(&wrong_length_path),
        Err(Error::InvalidSegment(_))
    ));

    let mut unknown_flags = original.clone();
    unknown_flags[12..16].copy_from_slice(&2u32.to_be_bytes());
    rewrite_checksum(&mut unknown_flags);
    let unknown_flags_path = directory.path().join("unknown-flags.seg");
    std::fs::write(&unknown_flags_path, unknown_flags).unwrap();
    assert!(matches!(
        Segment::open(&unknown_flags_path),
        Err(Error::InvalidSegment(_))
    ));

    let mut corrupt_body = original;
    corrupt_body[52] ^= 0xff;
    rewrite_checksum(&mut corrupt_body);
    let corrupt_body_path = directory.path().join("corrupt-body.seg");
    std::fs::write(&corrupt_body_path, corrupt_body).unwrap();
    assert!(matches!(
        Segment::open(&corrupt_body_path),
        Err(Error::InvalidSegment(_))
    ));

    let mut impossible_entries =
        std::fs::read(segments.read_dir().unwrap().next().unwrap().unwrap().path()).unwrap();
    let index_offset = u64::from_be_bytes(impossible_entries[48..56].try_into().unwrap()) as usize;
    impossible_entries[index_offset + 40..index_offset + 48]
        .copy_from_slice(&u64::MAX.to_be_bytes());
    rewrite_checksum(&mut impossible_entries);
    let impossible_entries_path = directory.path().join("impossible-entries.seg");
    std::fs::write(&impossible_entries_path, impossible_entries).unwrap();
    assert!(matches!(
        Segment::open(&impossible_entries_path),
        Err(Error::InvalidSegment(_))
    ));
}

#[test]
fn legacy_v1_segments_remain_readable_after_v2_compression() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.seg");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"VYRSEG01");
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&40u16.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&1u64.to_be_bytes());
    bytes.extend_from_slice(&1u64.to_be_bytes());
    bytes.extend_from_slice(&1u64.to_be_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0, 0, 0]);
    bytes.extend_from_slice(&5u32.to_be_bytes());
    bytes.extend_from_slice(&3u32.to_be_bytes());
    bytes.extend_from_slice(&1u64.to_be_bytes());
    bytes.extend_from_slice(b"alpha");
    bytes.extend_from_slice(b"one");
    bytes.extend_from_slice(vyrm_core::digest::sha256_hex(&bytes).as_bytes());
    std::fs::write(&path, bytes).unwrap();

    let segment = Segment::open(&path).unwrap();
    assert_eq!(segment.get(b"alpha", 1).unwrap(), Some(b"one".to_vec()));
    assert_eq!(segment.descriptor.entries, 1);
}

#[test]
fn versions_crossing_block_boundaries_keep_exact_mvcc_semantics() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = vyrm_kv::Database::create(directory.path()).unwrap();
    let operations = (0..160)
        .map(|version| Mutation::Put {
            key: b"one-key".to_vec(),
            value: vec![version as u8; 1024],
        })
        .collect();
    database
        .write_owned(
            WriteBatch::new(operations).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    database.flush_memtable(1).unwrap();

    for sequence in [1, 63, 64, 65, 127, 128, 160] {
        assert_eq!(
            database
                .get(b"one-key", vyrm_kv::Snapshot { sequence })
                .unwrap(),
            Some(vec![(sequence - 1) as u8; 1024])
        );
    }
}

#[test]
fn authenticated_block_filters_skip_negative_point_read_io() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = vyrm_kv::Database::create(directory.path()).unwrap();
    let operations = (0..200)
        .map(|index| Mutation::Put {
            key: format!("key:{:04}", index * 2).into_bytes(),
            value: vec![index as u8; 128],
        })
        .collect();
    database
        .write_owned(
            WriteBatch::new(operations).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    database.flush_memtable(1).unwrap();
    let snapshot = database.snapshot();
    let before = database.block_cache_stats();

    let mut rejected = false;
    for index in 0..200 {
        let missing = format!("key:{:04}", index * 2 + 1);
        assert_eq!(database.get(missing.as_bytes(), snapshot).unwrap(), None);
        let after = database.block_cache_stats();
        if after.filter_negatives > before.filter_negatives {
            assert_eq!(after.loads, before.loads);
            rejected = true;
            break;
        }
    }
    assert!(
        rejected,
        "expected at least one deterministic negative filter hit"
    );

    assert_eq!(
        database.get(b"key:0000", snapshot).unwrap(),
        Some(vec![0; 128])
    );
    let after_present = database.block_cache_stats();
    assert!(after_present.filter_checks > before.filter_checks);
    assert!(after_present.loads > before.loads);
}

#[test]
fn an_open_v3_segment_detects_later_block_tampering_on_read() {
    let directory = tempfile::tempdir().unwrap();
    let segments = directory.path().join("segments");
    let (segment, path) = Segment::write_from_memtable(&segments, &table()).unwrap();
    let original = std::fs::read(&path).unwrap();
    let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(70)).unwrap();
    file.write_all(&[original[70] ^ 0x40]).unwrap();
    file.sync_all().unwrap();

    assert!(matches!(
        segment.get(b"alpha", 1),
        Err(Error::InvalidSegment(_))
    ));
}

#[test]
fn database_block_cache_is_shared_bounded_and_observable() {
    let directory = tempfile::tempdir().unwrap();
    let mut database =
        vyrm_kv::Database::create_with_block_cache(directory.path(), 12 * 1024).unwrap();
    let operations = (0..300)
        .map(|index| Mutation::Put {
            key: format!("cache:{index:04}").into_bytes(),
            value: vec![(index % 251) as u8; 1024],
        })
        .collect();
    database
        .write_owned(
            WriteBatch::new(operations).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    database.flush_memtable(1).unwrap();
    let snapshot = database.snapshot();

    assert_eq!(database.block_cache_stats().entries, 0);
    database.get(b"cache:0001", snapshot).unwrap();
    let after_miss = database.block_cache_stats();
    assert_eq!(after_miss.misses, 1);
    assert_eq!(after_miss.loads, 1);
    assert!(after_miss.bytes_loaded > 0);
    assert!(after_miss.bytes_decoded >= after_miss.bytes_loaded);
    database.get(b"cache:0001", snapshot).unwrap();
    let after_hit = database.block_cache_stats();
    assert_eq!(after_hit.hits, 1);
    assert_eq!(after_hit.loads, after_miss.loads);
    assert_eq!(after_hit.bytes_loaded, after_miss.bytes_loaded);
    for index in [70, 140, 210, 299] {
        database
            .get(format!("cache:{index:04}").as_bytes(), snapshot)
            .unwrap();
    }
    let final_stats = database.block_cache_stats();
    assert!(final_stats.resident_bytes <= final_stats.capacity_bytes);
    assert!(final_stats.evictions > 0);
}
