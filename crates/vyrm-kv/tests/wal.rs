use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use vyrm_kv::{
    recover, repair_torn_tail, Durability, Error, WalBatch, WalWriter, WAL_FORMAT_VERSION,
};

#[test]
fn atomic_frames_round_trip_and_continue_after_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("active.wal");
    let mut writer = WalWriter::create(&path).unwrap();
    let first = writer
        .append(
            &WalBatch {
                first_sequence: 1,
                last_sequence: 2,
                payload: b"alpha",
            },
            Durability::Authoritative,
        )
        .unwrap();
    assert!(first.durable);
    let second = writer
        .append(
            &WalBatch {
                first_sequence: 3,
                last_sequence: 3,
                payload: b"beta",
            },
            Durability::Buffered,
        )
        .unwrap();
    assert!(!second.durable);
    assert_eq!(writer.sync().unwrap(), 3);
    drop(writer);

    let recovery = recover(&path).unwrap();
    assert_eq!(recovery.recovered_through, 3);
    assert_eq!(recovery.valid_bytes, second.end_offset);
    assert_eq!(recovery.torn_tail, None);
    assert_eq!(recovery.batches[0].payload, b"alpha");
    assert_eq!(recovery.batches[1].payload, b"beta");
    assert_eq!(recover(&path).unwrap(), recovery, "recovery is idempotent");

    let mut reopened = WalWriter::open(&path).unwrap();
    assert_eq!(reopened.next_sequence(), 4);
    reopened
        .append(
            &WalBatch {
                first_sequence: 4,
                last_sequence: 7,
                payload: b"gamma",
            },
            Durability::Authoritative,
        )
        .unwrap();
    assert_eq!(recover(&path).unwrap().recovered_through, 7);
}

#[test]
fn sequence_gaps_and_invalid_batches_write_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("active.wal");
    let mut writer = WalWriter::create(&path).unwrap();
    let before = std::fs::metadata(&path).unwrap().len();
    for batch in [
        WalBatch {
            first_sequence: 2,
            last_sequence: 2,
            payload: b"gap",
        },
        WalBatch {
            first_sequence: 1,
            last_sequence: 1,
            payload: b"",
        },
        WalBatch {
            first_sequence: 1,
            last_sequence: u64::MAX,
            payload: b"overflow",
        },
    ] {
        assert!(matches!(
            writer.append(&batch, Durability::Authoritative),
            Err(Error::InvalidBatch(_))
        ));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), before);
    }
}

#[test]
fn torn_tail_is_reported_and_only_explicit_repair_truncates_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("active.wal");
    let mut writer = WalWriter::create(&path).unwrap();
    let receipt = writer
        .append(
            &WalBatch {
                first_sequence: 1,
                last_sequence: 1,
                payload: b"complete",
            },
            Durability::Authoritative,
        )
        .unwrap();
    drop(writer);
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"partial-record")
        .unwrap();

    let torn = recover(&path).unwrap();
    assert_eq!(torn.recovered_through, 1);
    assert_eq!(torn.valid_bytes, receipt.end_offset);
    assert_eq!(torn.torn_tail, Some(receipt.end_offset));
    assert!(matches!(
        WalWriter::open(&path),
        Err(Error::TornTail { offset }) if offset == receipt.end_offset
    ));

    let repaired = repair_torn_tail(&path).unwrap();
    assert_eq!(repaired.torn_tail, None);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), receipt.end_offset);
    assert_eq!(repair_torn_tail(&path).unwrap(), repaired);
    assert_eq!(WalWriter::open(&path).unwrap().next_sequence(), 2);
}

#[test]
fn a_torn_payload_keeps_only_the_prior_atomic_batch() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("active.wal");
    let mut writer = WalWriter::create(&path).unwrap();
    let first = writer
        .append(
            &WalBatch {
                first_sequence: 1,
                last_sequence: 1,
                payload: b"first",
            },
            Durability::Authoritative,
        )
        .unwrap();
    let second = writer
        .append(
            &WalBatch {
                first_sequence: 2,
                last_sequence: 2,
                payload: b"second-payload",
            },
            Durability::Authoritative,
        )
        .unwrap();
    drop(writer);
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(second.end_offset - 3)
        .unwrap();

    let recovery = recover(&path).unwrap();
    assert_eq!(recovery.recovered_through, 1);
    assert_eq!(recovery.batches.len(), 1);
    assert_eq!(recovery.valid_bytes, first.end_offset);
    assert_eq!(recovery.torn_tail, Some(first.end_offset));
}

#[test]
fn complete_corruption_is_fatal_and_never_repaired_as_a_torn_tail() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("active.wal");
    let mut writer = WalWriter::create(&path).unwrap();
    let receipt = writer
        .append(
            &WalBatch {
                first_sequence: 1,
                last_sequence: 1,
                payload: b"checksummed",
            },
            Durability::Authoritative,
        )
        .unwrap();
    drop(writer);

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.seek(SeekFrom::Start(receipt.end_offset - 1)).unwrap();
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).unwrap();
    byte[0] ^= 0x80;
    file.seek(SeekFrom::Start(receipt.end_offset - 1)).unwrap();
    file.write_all(&byte).unwrap();
    file.sync_all().unwrap();
    drop(file);

    assert!(matches!(recover(&path), Err(Error::Corruption { .. })));
    assert!(matches!(
        repair_torn_tail(&path),
        Err(Error::Corruption { .. })
    ));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), receipt.end_offset);
}

#[test]
fn unknown_versions_fail_before_any_replay() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("active.wal");
    drop(WalWriter::create(&path).unwrap());
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(8)).unwrap();
    file.write_all(&(WAL_FORMAT_VERSION + 1).to_be_bytes())
        .unwrap();
    file.sync_all().unwrap();
    assert!(matches!(
        recover(&path),
        Err(Error::UnsupportedVersion { object: "WAL", .. })
    ));
}

#[test]
fn wal_bytes_match_the_checked_in_format_vector() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("active.wal");
    let mut writer = WalWriter::create(&path).unwrap();
    writer
        .append(
            &WalBatch {
                first_sequence: 1,
                last_sequence: 3,
                payload: b"vyrm-golden",
            },
            Durability::Authoritative,
        )
        .unwrap();
    drop(writer);
    let bytes = std::fs::read(path).unwrap();
    let actual = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/wal-v1.hex");
    if std::env::var_os("VYRM_UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(format!("{}/fixtures", env!("CARGO_MANIFEST_DIR"))).unwrap();
        std::fs::write(fixture, format!("{actual}\n")).unwrap();
    }
    assert_eq!(
        format!("{actual}\n"),
        std::fs::read_to_string(fixture).unwrap()
    );
}
