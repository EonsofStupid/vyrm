use rrd_core::{
    Claim, DataTransaction, Predicate, Producer, RuntimeCommit, RuntimeMutation, RuntimeProperties,
    RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType, ScopeId,
    Subject,
};
use rrd_store::{
    create_application_backup, create_logical_backup, load_backup_catalogue,
    prune_backup_catalogue, restore_catalogued_backup, verify_backup_catalogue, BackupCoverage,
    BackupPrunePlan, ControlTransition, DataRuntime, Engine, LocalObjectStore, NativeEngine,
};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

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
fn authenticated_prune_is_partition_bound_replayable_and_preserves_shared_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let engine = NativeEngine::open(&root.path().join("source")).unwrap();
    let objects = LocalObjectStore::open(root.path().join("source-objects")).unwrap();
    let catalogue_root = root.path().join("catalogue");
    let first =
        create_application_backup(&engine, &objects, &catalogue_root, "first", 100).unwrap();
    let second =
        create_application_backup(&engine, &objects, &catalogue_root, "second", 200).unwrap();
    assert_eq!(first.archive_file, second.archive_file);
    assert_eq!(first.object_manifest_file, second.object_manifest_file);
    assert_eq!(
        first.catalogue_manifest_file,
        second.catalogue_manifest_file
    );
    let before = verify_backup_catalogue(&catalogue_root).unwrap();
    let plan = BackupPrunePlan {
        expected_catalogue_sha256: before.catalogue_sha256.clone(),
        retained_backup_ids: vec![second.backup_id.clone()],
        prune_candidate_backup_ids: vec![first.backup_id.clone()],
    };

    let pruned = prune_backup_catalogue(&catalogue_root, &plan).unwrap();
    assert!(!pruned.idempotent_replay);
    assert_eq!(pruned.pruned_backup_ids, vec![first.backup_id.clone()]);
    assert_eq!(pruned.catalogue.revision, 3);
    assert_eq!(pruned.catalogue.backups, vec![second.clone()]);
    assert!(catalogue_root.join(&second.archive_file).is_file());
    assert!(catalogue_root
        .join(second.object_manifest_file.as_ref().unwrap())
        .is_file());
    assert!(catalogue_root
        .join(second.catalogue_manifest_file.as_ref().unwrap())
        .is_file());

    let replay = prune_backup_catalogue(&catalogue_root, &plan).unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(
        replay.catalogue.catalogue_sha256,
        pruned.catalogue.catalogue_sha256
    );
    let stale = BackupPrunePlan {
        expected_catalogue_sha256: before.catalogue_sha256,
        retained_backup_ids: vec![second.backup_id],
        prune_candidate_backup_ids: vec!["f".repeat(64)],
    };
    assert!(prune_backup_catalogue(&catalogue_root, &stale)
        .unwrap_err()
        .to_string()
        .contains("stale"));
    assert!(
        create_application_backup(&engine, &objects, &catalogue_root, "first", 100)
            .unwrap_err()
            .to_string()
            .contains("cannot be resurrected")
    );
}

#[test]
fn prune_rejects_incomplete_or_corrupt_inventory_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let engine = NativeEngine::open(&root.path().join("source")).unwrap();
    let catalogue_root = root.path().join("catalogue");
    engine.append_batch(&[claim("one", 10)]).unwrap();
    let first = create_logical_backup(&engine, &catalogue_root, "first", 100).unwrap();
    engine.append_batch(&[claim("two", 20)]).unwrap();
    let second = create_logical_backup(&engine, &catalogue_root, "second", 200).unwrap();
    let before = verify_backup_catalogue(&catalogue_root).unwrap();
    let incomplete = BackupPrunePlan {
        expected_catalogue_sha256: before.catalogue_sha256.clone(),
        retained_backup_ids: Vec::new(),
        prune_candidate_backup_ids: vec![first.backup_id.clone()],
    };
    assert!(prune_backup_catalogue(&catalogue_root, &incomplete)
        .unwrap_err()
        .to_string()
        .contains("complete disjoint"));

    let catalogue_bytes = std::fs::read(catalogue_root.join("catalogue.json")).unwrap();
    let archive = catalogue_root.join(&first.archive_file);
    let mut file = OpenOptions::new().write(true).open(&archive).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[0x7f]).unwrap();
    file.sync_all().unwrap();
    let valid_partition = BackupPrunePlan {
        expected_catalogue_sha256: before.catalogue_sha256,
        retained_backup_ids: vec![second.backup_id],
        prune_candidate_backup_ids: vec![first.backup_id],
    };
    assert!(prune_backup_catalogue(&catalogue_root, &valid_partition).is_err());
    assert_eq!(
        std::fs::read(catalogue_root.join("catalogue.json")).unwrap(),
        catalogue_bytes
    );
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

#[test]
fn application_backup_payload_corruption_fails_before_restore_publication() {
    let root = tempfile::tempdir().unwrap();
    let runtime = DataRuntime::new(
        NativeEngine::open(&root.path().join("source")).unwrap(),
        LocalObjectStore::open(root.path().join("source-objects")).unwrap(),
    );
    let scope = ScopeId::new("instance:complete-backup").unwrap();
    let mut schema = RuntimeSchemaRegistry::empty(1, "backup object schema");
    schema.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema::default(),
    );
    runtime
        .engine()
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 10,
            actor: "backup-test".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema { registry: schema },
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: RuntimeRef::new("document", "one").unwrap(),
                        valid_from: 10,
                        valid_to: None,
                        properties: RuntimeProperties::new(),
                    },
                },
            ],
        })
        .unwrap();
    let object = runtime
        .stage_object(
            "document-one-payload",
            Some(RuntimeRef::new("document", "one").unwrap()),
            "application/octet-stream",
            b"authenticated backup payload",
        )
        .unwrap();
    let read = runtime.engine().runtime_read_stamp(&scope).unwrap();
    runtime
        .commit(
            &DataTransaction::new(
                read.clone(),
                RuntimeCommit {
                    scope,
                    at: 20,
                    actor: "backup-test".into(),
                    expected_cursor: read.commit_cursor,
                    mutations: vec![RuntimeMutation::Object {
                        object: object.clone(),
                    }],
                },
            )
            .unwrap(),
        )
        .unwrap();
    let catalogue_root = root.path().join("catalogue");
    let entry = create_application_backup(
        runtime.engine(),
        runtime.objects(),
        &catalogue_root,
        "complete",
        100,
    )
    .unwrap();
    assert_eq!(entry.object_payloads, BackupCoverage::Included);
    assert_eq!(entry.catalogues, BackupCoverage::Included);
    assert!(entry.application_complete);
    verify_backup_catalogue(&catalogue_root).unwrap();

    let retained = catalogue_root.join("payloads").join(&object.receipt.key);
    let mut file = OpenOptions::new().write(true).open(retained).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[0x7f]).unwrap();
    file.sync_all().unwrap();
    assert!(verify_backup_catalogue(&catalogue_root).is_err());
    let target = root.path().join("must-not-publish");
    assert!(restore_catalogued_backup(&catalogue_root, &entry.backup_id, &target, 200).is_err());
    assert!(!target.exists());
}

#[test]
fn application_backup_restores_object_catalogue_and_audit_closure() {
    let root = tempfile::tempdir().unwrap();
    let runtime = DataRuntime::new(
        NativeEngine::open(&root.path().join("source")).unwrap(),
        LocalObjectStore::open(root.path().join("source-objects")).unwrap(),
    );
    let scope = ScopeId::new("instance:closure").unwrap();
    let mut schema = RuntimeSchemaRegistry::empty(1, "backup closure schema");
    schema.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema::default(),
    );
    let bootstrap = runtime
        .engine()
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 10,
            actor: "backup-closure".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema { registry: schema },
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: RuntimeRef::new("document", "one").unwrap(),
                        valid_from: 10,
                        valid_to: None,
                        properties: RuntimeProperties::new(),
                    },
                },
            ],
        })
        .unwrap();
    let object = runtime
        .stage_object(
            "document-one-payload",
            Some(RuntimeRef::new("document", "one").unwrap()),
            "text/plain",
            b"portable object closure",
        )
        .unwrap();
    let read = runtime.engine().runtime_read_stamp(&scope).unwrap();
    let object_commit = runtime
        .commit(
            &DataTransaction::new(
                read,
                RuntimeCommit {
                    scope: scope.clone(),
                    at: 20,
                    actor: "backup-closure".into(),
                    expected_cursor: bootstrap.last_cursor,
                    mutations: vec![RuntimeMutation::Object {
                        object: object.clone(),
                    }],
                },
            )
            .unwrap(),
        )
        .unwrap();
    let catalogue_key = format!(
        "server/state/vector-collection-catalogue/{}",
        scope.as_str()
    );
    let catalogue_value = br#"{"collection":"documents","vector":"body"}"#.to_vec();
    runtime
        .engine()
        .commit_catalog_transition(
            &scope,
            &ControlTransition {
                key: catalogue_key.clone(),
                expected: None,
                replacement: Some(catalogue_value.clone()),
                at: 30,
                actor: "backup-closure".into(),
                action: "vector_catalogue.created".into(),
                request_id: "backup-closure-request".into(),
                operation_id: "backup-closure-operation".into(),
            },
        )
        .unwrap();

    let catalogue_root = root.path().join("catalogue");
    let entry = create_application_backup(
        runtime.engine(),
        runtime.objects(),
        &catalogue_root,
        "closure",
        100,
    )
    .unwrap();
    assert_eq!(
        entry.archive.runtime_audit_sha256.as_deref(),
        runtime
            .engine()
            .runtime_audit(&object_commit.commit_id)
            .unwrap()
            .as_ref()
            .map(|audit| audit.digest.as_str())
    );
    assert_eq!(entry.object_manifest.as_ref().unwrap().object_count, 1);
    assert_eq!(entry.catalogue_manifest.as_ref().unwrap().record_count, 1);

    let target = root.path().join("restored");
    restore_catalogued_backup(&catalogue_root, &entry.backup_id, &target, 200).unwrap();
    let restored = NativeEngine::open(&target).unwrap();
    assert_eq!(
        restored.control_record(&catalogue_key).unwrap(),
        Some(catalogue_value)
    );
    assert_eq!(
        restored
            .runtime_read_stamp(&scope)
            .unwrap()
            .catalog_revision,
        1
    );
    assert_eq!(
        restored.runtime_audit(&object_commit.commit_id).unwrap(),
        runtime
            .engine()
            .runtime_audit(&object_commit.commit_id)
            .unwrap()
    );
    let restored_objects = LocalObjectStore::open(target.join("immutable")).unwrap();
    assert_eq!(
        restored_objects.get(&object).unwrap(),
        b"portable object closure"
    );
}
