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
    assert_eq!(segment.get(b"alpha", 1), Some(b"one".as_slice()));
    assert_eq!(segment.get(b"alpha", 3), None);
    assert_eq!(segment.get(b"beta", 2), Some(b"two".as_slice()));
    assert_eq!(segment.get(b"beta", 4), Some(b"three".as_slice()));

    let reopened = Segment::open(&path).unwrap();
    assert_eq!(reopened, segment);
    assert_eq!(
        reopened.scan(b"a", Some(b"z"), 2),
        vec![
            (b"alpha".to_vec(), b"one".to_vec()),
            (b"beta".to_vec(), b"two".to_vec()),
        ]
    );
    let (deduplicated, same_path) = Segment::write_from_memtable(&segments, &table()).unwrap();
    assert_eq!(same_path, path);
    assert_eq!(deduplicated, segment);
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
