use std::collections::BTreeMap;
use vyrm_core::{
    Claim, Predicate, Producer, RuntimeCommit, RuntimeMutation, RuntimeProperties, RuntimeRecord,
    RuntimeRef, RuntimeRelation, ScopeId, Subject,
};
use vyrm_store::{Engine, Error, MemoryEngine, Store};

fn record(kind: &str, id: &str) -> RuntimeRecord {
    RuntimeRecord {
        reference: RuntimeRef::new(kind, id).unwrap(),
        valid_from: 100,
        valid_to: None,
        properties: RuntimeProperties::new(),
    }
}

fn commit(expected_cursor: u64, scope: &str) -> RuntimeCommit {
    let prompt = record("prompt", "p1");
    let outcome = record("outcome", "o1");
    RuntimeCommit {
        scope: ScopeId::new(scope).unwrap(),
        at: 100,
        actor: "agent:test".into(),
        expected_cursor,
        mutations: vec![
            RuntimeMutation::Record {
                record: prompt.clone(),
            },
            RuntimeMutation::Record {
                record: outcome.clone(),
            },
            RuntimeMutation::Relation {
                relation: RuntimeRelation {
                    reference: RuntimeRef::new("caused", "p1-o1").unwrap(),
                    from: prompt.reference,
                    to: outcome.reference,
                    valid_from: 100,
                    valid_to: None,
                    properties: BTreeMap::new(),
                },
            },
            RuntimeMutation::Claim {
                claim: Claim::new(
                    Subject::new("runtime:p1").unwrap(),
                    Predicate::new("status").unwrap(),
                    "verified",
                    100,
                    100,
                    Producer {
                        actor: "agent:test".into(),
                        on_behalf_of: None,
                        session: None,
                    },
                ),
            },
        ],
    }
}

fn assert_runtime_contract(engine: &dyn Engine) {
    let first = commit(0, "instance:a");
    let outcome = engine.commit_runtime(&first).unwrap();
    assert_eq!(outcome.first_cursor, 1);
    assert_eq!(outcome.last_cursor, 4);
    assert_eq!(outcome.first_claim_sequence, Some(1));
    assert_eq!(engine.sequence().unwrap(), 1);

    let page = engine.runtime_changes_since(0, 2, None).unwrap();
    assert_eq!(page.through_cursor, 2);
    assert_eq!(page.head_cursor, 4);
    assert!(page.has_more());
    assert_eq!(page.changes.len(), 2);
    assert!(page.changes.iter().all(|change| change.verify_digest()));

    let rest = engine
        .runtime_changes_since(page.through_cursor, 10, None)
        .unwrap();
    assert_eq!(rest.through_cursor, 4);
    assert!(!rest.has_more());
    assert_eq!(rest.changes.len(), 2);
    assert_eq!(
        rest.changes[0].previous_digest.as_deref(),
        Some(page.changes[1].digest.as_str())
    );

    let stale = commit(0, "instance:a");
    assert!(matches!(
        engine.commit_runtime(&stale),
        Err(Error::RuntimeConflict {
            expected: 0,
            actual: 4
        })
    ));
    assert_eq!(engine.runtime_cursor().unwrap(), 4);
    assert_eq!(engine.sequence().unwrap(), 1);
}

#[test]
fn fjall_and_memory_enforce_the_same_runtime_contract() {
    let dir = tempfile::tempdir().unwrap();
    let fjall = Store::open(dir.path()).unwrap();
    let memory = MemoryEngine::new();
    assert_runtime_contract(&fjall);
    assert_runtime_contract(&memory);
}

#[test]
fn dangling_relation_rejects_the_entire_commit() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let bad = RuntimeCommit {
        scope: ScopeId::new("instance:a").unwrap(),
        at: 100,
        actor: "agent:test".into(),
        expected_cursor: 0,
        mutations: vec![RuntimeMutation::Relation {
            relation: RuntimeRelation {
                reference: RuntimeRef::new("caused", "missing").unwrap(),
                from: RuntimeRef::new("prompt", "missing").unwrap(),
                to: RuntimeRef::new("outcome", "missing").unwrap(),
                valid_from: 100,
                valid_to: None,
                properties: BTreeMap::new(),
            },
        }],
    };
    assert!(matches!(
        store.commit_runtime(&bad),
        Err(Error::DanglingRuntimeReference(_))
    ));
    assert_eq!(store.runtime_cursor().unwrap(), 0);
}

#[test]
fn scope_filter_advances_across_nonmatching_changes() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.commit_runtime(&commit(0, "instance:a")).unwrap();
    let only_b = ScopeId::new("instance:b").unwrap();
    let page = store.runtime_changes_since(0, 3, Some(&only_b)).unwrap();
    assert!(page.changes.is_empty());
    assert_eq!(page.through_cursor, 3);
    assert!(page.has_more());
}

#[test]
fn runtime_log_survives_reopen_and_continues_its_hash_chain() {
    let dir = tempfile::tempdir().unwrap();
    let first_digest = {
        let store = Store::open(dir.path()).unwrap();
        store.commit_runtime(&commit(0, "instance:a")).unwrap();
        store.runtime_changes_since(3, 1, None).unwrap().changes[0]
            .digest
            .clone()
    };
    let reopened = Store::open(dir.path()).unwrap();
    let next = RuntimeCommit {
        scope: ScopeId::new("instance:b").unwrap(),
        at: 200,
        actor: "agent:test".into(),
        expected_cursor: 4,
        mutations: vec![RuntimeMutation::Record {
            record: record("prompt", "p2"),
        }],
    };
    reopened.commit_runtime(&next).unwrap();
    let change = reopened
        .runtime_changes_since(4, 1, None)
        .unwrap()
        .changes
        .remove(0);
    assert_eq!(
        change.previous_digest.as_deref(),
        Some(first_digest.as_str())
    );
    assert_eq!(reopened.runtime_cursor().unwrap(), 5);
}
