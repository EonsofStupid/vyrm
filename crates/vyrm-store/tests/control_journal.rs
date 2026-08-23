use vyrm_store::{ControlTransition, Engine, Error, MemoryEngine, NativeEngine, Store};

fn transition(expected: Option<&[u8]>, replacement: Option<&[u8]>, at: u64) -> ControlTransition {
    ControlTransition {
        key: "server/state/session/session-1".into(),
        expected: expected.map(<[u8]>::to_vec),
        replacement: replacement.map(<[u8]>::to_vec),
        at,
        actor: "rrd-server".into(),
        action: "session.transition".into(),
        request_id: format!("request-{at}"),
        operation_id: format!("operation-{at}"),
    }
}

fn assert_journal(engine: &dyn Engine) {
    let created = engine
        .commit_control_transition(&transition(None, Some(b"open"), 10))
        .unwrap();
    assert_eq!(created.sequence, 1);
    assert!(created.verify());
    assert_eq!(
        engine.control_record(&created.key).unwrap(),
        Some(b"open".to_vec())
    );

    assert!(matches!(
        engine.commit_control_transition(&transition(None, Some(b"collision"), 11)),
        Err(Error::ControlConflict(_))
    ));
    let renewed = engine
        .commit_control_transition(&transition(Some(b"open"), Some(b"renewed"), 12))
        .unwrap();
    assert_eq!(renewed.sequence, 2);
    assert_eq!(
        renewed.previous_digest.as_deref(),
        Some(created.digest.as_str())
    );
    let deleted = engine
        .commit_control_transition(&transition(Some(b"renewed"), None, 13))
        .unwrap();
    assert_eq!(deleted.sequence, 3);
    assert_eq!(engine.control_record(&deleted.key).unwrap(), None);
    let journal = engine.control_journal_since(0, 10).unwrap();
    assert_eq!(journal, vec![created, renewed, deleted]);
    assert!(journal.iter().all(|entry| entry.verify()));
}

#[test]
fn every_engine_materializes_and_journals_the_same_cas_transitions() {
    assert_journal(&MemoryEngine::new());
    let native = tempfile::tempdir().unwrap();
    assert_journal(&NativeEngine::open(&native.path().join("native")).unwrap());
    let fjall = tempfile::tempdir().unwrap();
    assert_journal(&Store::open(&fjall.path().join("fjall")).unwrap());
}

#[test]
fn materialized_state_and_hash_chain_survive_restart() {
    let root = tempfile::tempdir().unwrap();
    for native in [true, false] {
        let path = root.path().join(if native { "native" } else { "fjall" });
        if native {
            let engine = NativeEngine::open(&path).unwrap();
            engine
                .commit_control_transition(&transition(None, Some(b"open"), 10))
                .unwrap();
            drop(engine);
            let reopened = NativeEngine::open(&path).unwrap();
            assert_eq!(
                reopened
                    .control_record("server/state/session/session-1")
                    .unwrap(),
                Some(b"open".to_vec())
            );
            assert_eq!(reopened.control_journal_since(0, 10).unwrap().len(), 1);
        } else {
            let engine = Store::open(&path).unwrap();
            engine
                .commit_control_transition(&transition(None, Some(b"open"), 10))
                .unwrap();
            drop(engine);
            let reopened = Store::open(&path).unwrap();
            assert_eq!(
                reopened
                    .control_record("server/state/session/session-1")
                    .unwrap(),
                Some(b"open".to_vec())
            );
            assert_eq!(reopened.control_journal_since(0, 10).unwrap().len(), 1);
        }
    }
}
