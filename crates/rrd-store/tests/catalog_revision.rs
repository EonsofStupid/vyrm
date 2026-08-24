use rrd_store::{ControlTransition, Engine, Error, MemoryEngine, NativeEngine, Store};
use vyrm_core::ScopeId;

fn scope(name: &str) -> ScopeId {
    ScopeId::new(name).unwrap()
}

fn catalogue_transition(key_suffix: &str) -> ControlTransition {
    ControlTransition {
        key: format!("server/state/catalogue/{key_suffix}"),
        expected: None,
        replacement: Some(br#"{"state":"ready"}"#.to_vec()),
        at: 1,
        actor: "test:catalogue-revision".into(),
        action: "catalogue.ensure".into(),
        request_id: format!("request-{key_suffix}"),
        operation_id: format!("operation-{key_suffix}"),
    }
}

fn assert_catalogue_revision_contract(engine: &dyn Engine, key_suffix: &str) {
    let target = scope("instance:catalogue-target");
    let unrelated = scope("instance:catalogue-unrelated");
    let stale = engine.runtime_read_stamp(&target).unwrap();
    let unrelated_before = engine.runtime_read_stamp(&unrelated).unwrap();

    assert_eq!(stale.catalog_revision, 0);
    assert_eq!(unrelated_before.catalog_revision, 0);

    let cursor_before = stale.commit_cursor;
    let manifest_before = stale.manifest_id.clone();
    let (revision, entry) = engine
        .commit_catalog_transition(&target, &catalogue_transition(key_suffix))
        .unwrap();

    assert_eq!(revision, 1);
    assert!(entry.verify());

    let fresh = engine.runtime_read_stamp(&target).unwrap();
    assert_eq!(fresh.catalog_revision, 1);
    assert_eq!(fresh.commit_cursor, cursor_before);
    assert_ne!(fresh.manifest_id, manifest_before);

    assert!(matches!(
        engine.runtime_read_changes(&stale, stale.commit_cursor, 1),
        Err(Error::ReadStampMismatch(_))
    ));
    engine
        .runtime_read_changes(&fresh, fresh.commit_cursor, 1)
        .unwrap();

    let unrelated_after = engine.runtime_read_stamp(&unrelated).unwrap();
    assert_eq!(unrelated_after.catalog_revision, 0);
    assert_eq!(unrelated_after.manifest_id, unrelated_before.manifest_id);
}

#[test]
fn every_engine_binds_catalogue_transitions_to_the_scope_read_stamp() {
    assert_catalogue_revision_contract(&MemoryEngine::new(), "memory");

    let fjall_root = tempfile::tempdir().unwrap();
    assert_catalogue_revision_contract(&Store::open(fjall_root.path()).unwrap(), "fjall");

    let native_root = tempfile::tempdir().unwrap();
    assert_catalogue_revision_contract(&NativeEngine::open(native_root.path()).unwrap(), "native");
}

#[test]
fn fjall_catalogue_revision_survives_reopen() {
    let root = tempfile::tempdir().unwrap();
    let target = scope("instance:catalogue-reopen-fjall");
    let stale = {
        let engine = Store::open(root.path()).unwrap();
        let stale = engine.runtime_read_stamp(&target).unwrap();
        let (revision, _) = engine
            .commit_catalog_transition(&target, &catalogue_transition("reopen-fjall"))
            .unwrap();
        assert_eq!(revision, 1);
        stale
    };

    let reopened = Store::open(root.path()).unwrap();
    let fresh = reopened.runtime_read_stamp(&target).unwrap();
    assert_eq!(fresh.catalog_revision, 1);
    assert_eq!(fresh.commit_cursor, stale.commit_cursor);
    assert_ne!(fresh.manifest_id, stale.manifest_id);
    assert!(matches!(
        reopened.runtime_read_changes(&stale, stale.commit_cursor, 1),
        Err(Error::ReadStampMismatch(_))
    ));
}

#[test]
fn native_catalogue_revision_survives_reopen() {
    let root = tempfile::tempdir().unwrap();
    let target = scope("instance:catalogue-reopen-native");
    let stale = {
        let engine = NativeEngine::open(root.path()).unwrap();
        let stale = engine.runtime_read_stamp(&target).unwrap();
        let (revision, _) = engine
            .commit_catalog_transition(&target, &catalogue_transition("reopen-native"))
            .unwrap();
        assert_eq!(revision, 1);
        stale
    };

    let reopened = NativeEngine::open(root.path()).unwrap();
    let fresh = reopened.runtime_read_stamp(&target).unwrap();
    assert_eq!(fresh.catalog_revision, 1);
    assert_eq!(fresh.commit_cursor, stale.commit_cursor);
    assert_ne!(fresh.manifest_id, stale.manifest_id);
    assert!(matches!(
        reopened.runtime_read_changes(&stale, stale.commit_cursor, 1),
        Err(Error::ReadStampMismatch(_))
    ));
}
