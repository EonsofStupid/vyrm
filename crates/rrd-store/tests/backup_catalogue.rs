use rrd_store::{
    create_logical_backup, load_backup_catalogue, restore_catalogued_backup,
    verify_backup_catalogue, BackupCoverage, Engine, NativeEngine,
};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use vyrm_core::{Claim, Predicate, Producer, Subject};

fn claim(name: &str, at: u64) -> Claim {
    Claim::new(
        Subject::new(format!("backup:{name}")).unwrap(),
        Predicate::new("status").unwrap(),
        "retained",
        at,
        at,
        Producer {
            actor: "agent:backup-test".into(),
            on_behalf_of: None,
            session: None,
        },
    )
}

#[test]
fn catalogues_multiple_cuts_and_restores_the_selected_backup() {
    let root = tempfile::tempdir().unwrap();
    let engine = NativeEngine::open(&root.path().join("source")).unwrap();
    let catalogue_root = root.path().join("catalogue");
    engine.append_batch(&[claim("one", 10)]).unwrap();
    let first = create_logical_backup(&engine, &catalogue_root, "first", 100).unwrap();
    assert_eq!(first.claims, BackupCoverage::Included);
    assert_eq!(first.object_payloads, BackupCoverage::ReferencedOnly);
    assert!(!first.application_complete);

    engine.append_batch(&[claim("two", 20)]).unwrap();
    let second = create_logical_backup(&engine, &catalogue_root, "second", 200).unwrap();
    let catalogue = verify_backup_catalogue(&catalogue_root).unwrap();
    assert_eq!(catalogue.revision, 2);
    assert_eq!(catalogue.backups, vec![first.clone(), second]);

    let target = root.path().join("restored-first");
    restore_catalogued_backup(&catalogue_root, &first.backup_id, &target, 300).unwrap();
    let restored = NativeEngine::open(&target).unwrap();
    assert_eq!(restored.sequence().unwrap(), 1);
    assert_eq!(
        restored.claims_in_range(0, 1).unwrap(),
        vec![claim("one", 10)]
    );
}

#[test]
fn repeated_identical_backup_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let engine = NativeEngine::open(&root.path().join("source")).unwrap();
    let catalogue_root = root.path().join("catalogue");
    engine.append_batch(&[claim("one", 10)]).unwrap();
    let first = create_logical_backup(&engine, &catalogue_root, "daily", 100).unwrap();
    let retry = create_logical_backup(&engine, &catalogue_root, "daily", 100).unwrap();
    assert_eq!(retry, first);
    let catalogue = load_backup_catalogue(&catalogue_root).unwrap();
    assert_eq!(catalogue.revision, 1);
    assert_eq!(catalogue.backups.len(), 1);
}

#[test]
fn archive_or_catalogue_corruption_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let engine = NativeEngine::open(&root.path().join("source")).unwrap();
    let catalogue_root = root.path().join("catalogue");
    engine.append_batch(&[claim("one", 10)]).unwrap();
    let entry = create_logical_backup(&engine, &catalogue_root, "daily", 100).unwrap();
    let archive = catalogue_root.join(&entry.archive_file);
    let mut file = OpenOptions::new().write(true).open(&archive).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[0x7f]).unwrap();
    file.sync_all().unwrap();
    assert!(verify_backup_catalogue(&catalogue_root).is_err());
    let target = root.path().join("must-not-exist");
    assert!(restore_catalogued_backup(&catalogue_root, &entry.backup_id, &target, 200).is_err());
    assert!(!target.exists());

    let catalogue = catalogue_root.join("catalogue.json");
    let mut bytes = std::fs::read(&catalogue).unwrap();
    let position = bytes.iter().position(|byte| *byte == b'f').unwrap();
    bytes[position] = b'e';
    std::fs::write(&catalogue, bytes).unwrap();
    assert!(load_backup_catalogue(&catalogue_root).is_err());
}
