use rrd_store::{
    export_logical_archive, inspect_logical_archive, restore_logical_archive_to_new_root, Engine,
    NativeEngine,
};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use vyrm_core::{Claim, Predicate, Producer, RuntimeCommit, RuntimeMutation, ScopeId, Subject};
use vyrm_core::{RuntimeEventSchema, RuntimeSchemaRegistry, RuntimeType};

fn claim(name: &str, at: u64) -> Claim {
    Claim::new(
        Subject::new(format!("subject:{name}")).unwrap(),
        Predicate::new("status").unwrap(),
        "verified",
        at,
        at,
        Producer {
            actor: "agent:archive-test".into(),
            on_behalf_of: None,
            session: Some("archive-test".into()),
        },
    )
}

fn runtime_claim(expected_cursor: u64, value: Claim) -> RuntimeCommit {
    RuntimeCommit {
        scope: ScopeId::new("instance:archive-test").unwrap(),
        at: value.tx_time,
        actor: "agent:archive-test".into(),
        expected_cursor,
        mutations: vec![RuntimeMutation::Claim { claim: value }],
    }
}

fn source(root: &std::path::Path) -> NativeEngine {
    let engine = NativeEngine::open(root).unwrap();
    engine
        .append_batch(&[claim("standalone-before", 10)])
        .unwrap();
    let mut registry = RuntimeSchemaRegistry::empty(1, "archive test bootstrap");
    registry.events.insert(
        RuntimeType::new("archive_event").unwrap(),
        RuntimeEventSchema::default(),
    );
    engine
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new("instance:archive-test").unwrap(),
            at: 15,
            actor: "agent:archive-test".into(),
            expected_cursor: 0,
            mutations: vec![RuntimeMutation::Schema { registry }],
        })
        .unwrap();
    engine
        .commit_runtime(&runtime_claim(1, claim("runtime-one", 20)))
        .unwrap();
    engine
        .append_batch(&[claim("standalone-middle", 30)])
        .unwrap();
    engine
        .commit_runtime(&runtime_claim(2, claim("runtime-two", 40)))
        .unwrap();
    engine
        .append_batch(&[claim("standalone-after", 50)])
        .unwrap();
    engine
}

#[test]
fn exports_inspects_and_restores_exact_log_coordinates() {
    let root = tempfile::tempdir().unwrap();
    let source_root = root.path().join("source");
    let archive = root.path().join("backup.rrd-archive");
    let target = root.path().join("restored");
    let source = source(&source_root);

    let exported = export_logical_archive(&source, &archive).unwrap();
    assert_eq!(exported.claim_sequence, 5);
    assert_eq!(exported.runtime_cursor, 3);
    assert_eq!(exported.standalone_claims, 3);
    assert_eq!(exported.runtime_commits, 3);
    assert_eq!(inspect_logical_archive(&archive).unwrap(), exported);

    let report = restore_logical_archive_to_new_root(&archive, &target, 100).unwrap();
    assert!(report.reopened);
    assert_eq!(report.inventory, exported);

    let restored = NativeEngine::open(&target).unwrap();
    assert_eq!(restored.sequence().unwrap(), source.sequence().unwrap());
    assert_eq!(
        restored.runtime_cursor().unwrap(),
        source.runtime_cursor().unwrap()
    );
    assert_eq!(
        restored.claims_in_range(0, 5).unwrap(),
        source.claims_in_range(0, 5).unwrap()
    );
    assert_eq!(
        restored.runtime_changes_since(0, 10, None).unwrap().changes,
        source.runtime_changes_since(0, 10, None).unwrap().changes
    );
}

#[test]
fn corruption_is_denied_before_target_publication_and_retry_succeeds() {
    let root = tempfile::tempdir().unwrap();
    let source = source(&root.path().join("source"));
    let archive = root.path().join("backup.rrd-archive");
    let target = root.path().join("restored");
    export_logical_archive(&source, &archive).unwrap();
    let original = std::fs::read(&archive).unwrap();

    let mut file = OpenOptions::new().write(true).open(&archive).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[0x7f]).unwrap();
    file.sync_all().unwrap();
    assert!(inspect_logical_archive(&archive).is_err());
    assert!(restore_logical_archive_to_new_root(&archive, &target, 100).is_err());
    assert!(!target.exists());
    assert!(!std::fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("rrd-restore")
    }));

    std::fs::write(&archive, original).unwrap();
    restore_logical_archive_to_new_root(&archive, &target, 101).unwrap();
    assert_eq!(NativeEngine::open(&target).unwrap().sequence().unwrap(), 5);
}

#[test]
fn truncated_archive_and_existing_restore_root_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let source = source(&root.path().join("source"));
    let archive = root.path().join("backup.rrd-archive");
    let target = root.path().join("existing");
    export_logical_archive(&source, &archive).unwrap();
    std::fs::create_dir(&target).unwrap();
    assert!(restore_logical_archive_to_new_root(&archive, &target, 100).is_err());

    let truncated = root.path().join("truncated.rrd-archive");
    let bytes = std::fs::read(&archive).unwrap();
    std::fs::write(&truncated, &bytes[..bytes.len() - 17]).unwrap();
    assert!(inspect_logical_archive(&truncated).is_err());
}
