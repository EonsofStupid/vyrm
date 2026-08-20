use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

use fjall::{KeyspaceCreateOptions, SingleWriterTxDatabase};
use vyrm_core::{Claim, ClaimReader, Predicate, Producer, Subject};
use vyrm_store::{
    migrate_fjall_to_native, migrate_fjall_to_native_with_fault, rollback_fjall_migration, Engine,
    Error, InvocationInput, MigrationFault, MigrationPhase, Outcome, PersistentBackend,
    PersistentEngine, Store, Trigger,
};

fn claim(object: &str, sequence: u64) -> Claim {
    Claim::new(
        Subject::new("migration:subject").unwrap(),
        Predicate::new("status").unwrap(),
        object,
        sequence,
        sequence,
        Producer {
            actor: "test:migration".into(),
            on_behalf_of: None,
            session: None,
        },
    )
}

fn seed(path: &std::path::Path) {
    let store = Store::open(path).unwrap();
    store
        .append_batch(&[claim("first", 1), claim("second", 2)])
        .unwrap();
    store
        .put_projection_with(
            "migration-projection",
            b"projection bytes",
            vyrm_store::Durability::Authoritative,
        )
        .unwrap();
    store
        .record_invocation(InvocationInput {
            at: 3,
            trigger: Trigger::Manual,
            command: "migration-seed",
            arguments: &["case=full".into()],
            outcome: Outcome::Ok,
            duration_ms: 1,
            detail: Some("seed".into()),
            effectiveness: None,
        })
        .unwrap();
}

fn assert_native_state(path: &std::path::Path) {
    let engine = PersistentEngine::open(path).unwrap();
    assert_eq!(engine.backend(), PersistentBackend::Native);
    assert_eq!(engine.sequence().unwrap(), 2);
    assert_eq!(engine.invocation_count().unwrap(), 1);
    assert_eq!(
        engine
            .as_of(
                &Subject::new("migration:subject").unwrap(),
                &Predicate::new("status").unwrap(),
                99,
            )
            .unwrap()
            .unwrap()
            .object,
        "second"
    );
    assert_eq!(
        engine.get_projection("migration-projection").unwrap(),
        Some(b"projection bytes".to_vec())
    );
}

#[test]
fn byte_exact_migration_cuts_over_and_retains_fjall_for_rollback() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store");
    seed(&path);

    let report = migrate_fjall_to_native(&path, 10).unwrap();
    assert_eq!(report.phase, MigrationPhase::Complete);
    assert!(report.inventory.entries >= 8);
    assert!(report.fjall_backup.is_dir());
    assert!(report.archive.is_file());
    assert_native_state(&path);

    let rolled_back = rollback_fjall_migration(&path).unwrap();
    assert_eq!(rolled_back.phase, MigrationPhase::RolledBack);
    assert!(rolled_back.retired_native.is_dir());
    let engine = PersistentEngine::open(&path).unwrap();
    assert_eq!(engine.backend(), PersistentBackend::FjallCompatibility);
    assert_eq!(engine.sequence().unwrap(), 2);
}

#[test]
fn every_durable_and_rename_boundary_resumes_idempotently() {
    for fault in [
        MigrationFault::AfterExport,
        MigrationFault::AfterImport,
        MigrationFault::AfterVerify,
        MigrationFault::AfterSourceRename,
        MigrationFault::AfterSourceMove,
        MigrationFault::AfterCutoverRename,
        MigrationFault::AfterCutover,
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("store");
        seed(&path);
        assert!(matches!(
            migrate_fjall_to_native_with_fault(&path, 10, fault),
            Err(Error::FaultInjected(_))
        ));
        assert!(matches!(
            PersistentEngine::open(&path),
            Err(Error::Migration(_))
        ));

        let report = migrate_fjall_to_native(&path, 11).unwrap();
        assert_eq!(report.phase, MigrationPhase::Complete, "fault={fault:?}");
        assert_eq!(
            migrate_fjall_to_native(&path, 12).unwrap(),
            report,
            "completion must be idempotent for {fault:?}"
        );
        assert_native_state(&path);
    }
}

#[test]
fn rollback_refuses_to_discard_post_cutover_native_writes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store");
    seed(&path);
    migrate_fjall_to_native(&path, 10).unwrap();

    let engine = PersistentEngine::open(&path).unwrap();
    engine
        .append_batch(&[claim("native-divergence", 3)])
        .unwrap();
    drop(engine);

    assert_eq!(
        migrate_fjall_to_native(&path, 11).unwrap().phase,
        MigrationPhase::Complete,
        "a completed migration remains complete after legitimate native writes"
    );

    assert!(matches!(
        rollback_fjall_migration(&path),
        Err(Error::Migration(message)) if message.contains("diverged")
    ));
    assert_native_state_with_sequence(&path, 3);
}

fn assert_native_state_with_sequence(path: &std::path::Path, sequence: u64) {
    let engine = PersistentEngine::open(path).unwrap();
    assert_eq!(engine.backend(), PersistentBackend::Native);
    assert_eq!(engine.sequence().unwrap(), sequence);
}

#[test]
fn corrupt_archive_is_denied_before_cutover() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store");
    seed(&path);
    migrate_fjall_to_native_with_fault(&path, 10, MigrationFault::AfterExport).unwrap_err();
    let report = vyrm_store::migration_status(&path).unwrap().unwrap();
    let mut archive = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&report.archive)
        .unwrap();
    archive.seek(SeekFrom::End(-1)).unwrap();
    archive.write_all(&[0x7f]).unwrap();
    archive.sync_all().unwrap();

    assert!(matches!(
        migrate_fjall_to_native(&path, 11),
        Err(Error::Migration(message)) if message.contains("SHA-256")
    ));
    let fjall = Store::open(&path).unwrap();
    assert_eq!(fjall.sequence().unwrap(), 2);
}

#[test]
fn truncated_archive_is_denied_before_cutover() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store");
    seed(&path);
    migrate_fjall_to_native_with_fault(&path, 10, MigrationFault::AfterExport).unwrap_err();
    let report = vyrm_store::migration_status(&path).unwrap().unwrap();
    let length = std::fs::metadata(&report.archive).unwrap().len();
    OpenOptions::new()
        .write(true)
        .open(&report.archive)
        .unwrap()
        .set_len(length - 17)
        .unwrap();

    assert!(matches!(
        migrate_fjall_to_native(&path, 11),
        Err(Error::Migration(message)) if message.contains("truncated")
    ));
    assert_eq!(Store::open(&path).unwrap().sequence().unwrap(), 2);
}

#[test]
fn rollback_resumes_both_unmarked_rename_windows() {
    for backup_already_restored in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("store");
        seed(&path);
        let report = migrate_fjall_to_native(&path, 10).unwrap();

        std::fs::rename(&path, &report.retired_native).unwrap();
        if backup_already_restored {
            std::fs::rename(&report.fjall_backup, &path).unwrap();
        }
        let recovered = rollback_fjall_migration(&path).unwrap();
        assert_eq!(recovered.phase, MigrationPhase::RolledBack);
        let engine = PersistentEngine::open(&path).unwrap();
        assert_eq!(engine.backend(), PersistentBackend::FjallCompatibility);
        assert_eq!(engine.sequence().unwrap(), 2);
    }
}

#[test]
fn completed_migration_artifacts_rebase_when_the_instance_directory_moves() {
    let root = tempfile::tempdir().unwrap();
    let before = root.path().join("before");
    std::fs::create_dir(&before).unwrap();
    let path = before.join("store");
    seed(&path);
    migrate_fjall_to_native(&path, 10).unwrap();

    let after = root.path().join("after");
    std::fs::rename(&before, &after).unwrap();
    let moved = after.join("store");
    assert_native_state(&moved);
    let report = vyrm_store::migration_status(&moved).unwrap().unwrap();
    assert_eq!(report.source, std::fs::canonicalize(&moved).unwrap());
    assert_eq!(report.phase, MigrationPhase::Complete);
}

#[test]
fn empty_archive_v1_matches_the_portable_golden_vector() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("empty");
    drop(Store::open(&path).unwrap());
    migrate_fjall_to_native_with_fault(&path, 10, MigrationFault::AfterExport).unwrap_err();
    let report = vyrm_store::migration_status(&path).unwrap().unwrap();
    let bytes = std::fs::read(&report.archive).unwrap();
    let encoded: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        encoded,
        include_str!("fixtures/migration-v1-empty.hex").trim(),
        "archive format changed without a version/golden update"
    );
}

#[test]
fn unknown_fjall_keyspaces_are_denied_instead_of_silently_omitted() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store");
    seed(&path);
    let db = SingleWriterTxDatabase::builder(&path).open().unwrap();
    db.keyspace("future_unregistered_state", KeyspaceCreateOptions::default)
        .unwrap();
    drop(db);

    assert!(matches!(
        migrate_fjall_to_native(&path, 10),
        Err(Error::Migration(message)) if message.contains("unknown keyspace")
    ));
    assert!(!path.join("CURRENT").exists());
}
