use rrd_store::{
    export_logical_archive, migrate_native_format, migrate_native_format_with_fault,
    native_format_migration_status, restore_logical_archive_to_new_root, Engine,
    FormatMigrationFault, FormatMigrationPhase, NativeEngine,
};
use vyrm_core::{Claim, Predicate, Producer, Subject};
use vyrm_kv::Database;

fn claim(name: &str, at: u64) -> Claim {
    Claim::new(
        Subject::new(format!("format:{name}")).unwrap(),
        Predicate::new("status").unwrap(),
        "preserved",
        at,
        at,
        Producer {
            actor: "agent:format-test".into(),
            on_behalf_of: None,
            session: None,
        },
    )
}

fn legacy_source(path: &std::path::Path) {
    let database = Database::create(path).unwrap();
    assert_eq!(database.manifest().application_format, None);
    drop(database);
    let engine = NativeEngine::open(path).unwrap();
    engine
        .append_batch(&[claim("one", 10), claim("two", 20)])
        .unwrap();
    engine
        .put_projection("format-test", b"projection-bytes")
        .unwrap();
    engine.flush(30).unwrap();
}

#[test]
fn exact_successor_upgrade_preserves_all_keyspaces_and_resumes_idempotently() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("legacy-native");
    legacy_source(&source);
    let report = migrate_native_format(&source, 100).unwrap();
    assert_eq!(report.phase, FormatMigrationPhase::Complete);
    assert_eq!(report.source_application_format, None);
    assert_eq!(
        native_format_migration_status(&source).unwrap().unwrap(),
        report
    );
    assert_eq!(migrate_native_format(&source, 101).unwrap(), report);

    let engine = NativeEngine::open(&source).unwrap();
    assert_eq!(
        engine.manifest().unwrap().application_format,
        Some(report.target_application_format)
    );
    assert_eq!(
        engine.claims_in_range(0, 2).unwrap(),
        vec![claim("one", 10), claim("two", 20)]
    );
    assert_eq!(
        engine.get_projection("format-test").unwrap(),
        Some(b"projection-bytes".to_vec())
    );

    let retained = root.path().join(".legacy-native.native-format-v1-backup");
    let database = Database::open(&retained).unwrap();
    assert_eq!(database.manifest().application_format, None);
    assert_eq!(database.manifest().digest, report.source_manifest);
}

#[test]
fn every_durable_and_rename_boundary_resumes_to_complete() {
    let faults = [
        FormatMigrationFault::AfterExport,
        FormatMigrationFault::AfterImport,
        FormatMigrationFault::AfterVerify,
        FormatMigrationFault::AfterSourceRename,
        FormatMigrationFault::AfterSourceMove,
        FormatMigrationFault::AfterCutoverRename,
        FormatMigrationFault::AfterCutover,
    ];
    for (index, fault) in faults.into_iter().enumerate() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join(format!("legacy-{index}"));
        legacy_source(&source);
        assert!(migrate_native_format_with_fault(&source, 100, fault).is_err());
        let report = migrate_native_format(&source, 101).unwrap();
        assert_eq!(
            report.phase,
            FormatMigrationPhase::Complete,
            "fault {fault:?}"
        );
        assert_eq!(NativeEngine::open(&source).unwrap().sequence().unwrap(), 2);
    }
}

#[test]
fn logical_recovery_crosses_native_application_formats() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("legacy-native");
    let archive = root.path().join("legacy.rrd-archive");
    let target = root.path().join("current-native");
    legacy_source(&source);
    let legacy = NativeEngine::open(&source).unwrap();
    export_logical_archive(&legacy, &archive).unwrap();
    restore_logical_archive_to_new_root(&archive, &target, 100).unwrap();
    let restored = NativeEngine::open(&target).unwrap();
    assert!(restored.manifest().unwrap().application_format.is_some());
    assert_eq!(
        restored.claims_in_range(0, 2).unwrap(),
        legacy.claims_in_range(0, 2).unwrap()
    );
}

#[test]
fn upgrade_rejects_a_store_that_is_not_the_exact_predecessor() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current");
    NativeEngine::open(&current).unwrap();
    assert!(migrate_native_format(&current, 100).is_err());
    assert!(native_format_migration_status(&current).unwrap().is_none());
}

#[test]
fn source_mutation_after_export_is_denied_before_cutover() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("legacy-native");
    legacy_source(&source);
    assert!(
        migrate_native_format_with_fault(&source, 100, FormatMigrationFault::AfterExport).is_err()
    );
    NativeEngine::open(&source)
        .unwrap()
        .append_batch(&[claim("late", 40)])
        .unwrap();
    assert!(migrate_native_format(&source, 101).is_err());
    assert!(source.is_dir());
    assert!(!root
        .path()
        .join(".legacy-native.native-format-v1-backup")
        .exists());
}
