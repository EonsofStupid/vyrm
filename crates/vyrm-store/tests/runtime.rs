use std::collections::BTreeMap;
use vyrm_core::{
    Claim, Predicate, Producer, RuntimeCommit, RuntimeMutation, RuntimeProperties,
    RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeRelation,
    RuntimeRelationSchema, RuntimeSchemaRegistry, RuntimeType, RuntimeValue, RuntimeValueType,
    ScopeId, Subject,
};
use vyrm_store::{Engine, Error, MemoryEngine, NativeEngine, Store};

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
            test_schema(),
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

fn test_schema() -> RuntimeMutation {
    let mut registry = RuntimeSchemaRegistry::empty(1, "test schema");
    registry.records.insert(
        RuntimeType::new("prompt").unwrap(),
        RuntimeRecordSchema::default(),
    );
    registry.records.insert(
        RuntimeType::new("outcome").unwrap(),
        RuntimeRecordSchema::default(),
    );
    registry.relations.insert(
        RuntimeType::new("caused").unwrap(),
        RuntimeRelationSchema {
            from: std::collections::BTreeSet::from([RuntimeType::new("prompt").unwrap()]),
            to: std::collections::BTreeSet::from([RuntimeType::new("outcome").unwrap()]),
            unique_pair: true,
            max_outgoing: Some(1),
            max_incoming: Some(1),
            ..RuntimeRelationSchema::default()
        },
    );
    RuntimeMutation::Schema { registry }
}

fn assert_runtime_contract(engine: &dyn Engine) {
    let first = commit(0, "instance:a");
    let outcome = engine.commit_runtime(&first).unwrap();
    assert_eq!(outcome.first_cursor, 1);
    assert_eq!(outcome.last_cursor, 5);
    assert_eq!(outcome.first_claim_sequence, Some(1));
    assert_eq!(engine.sequence().unwrap(), 1);

    let page = engine.runtime_changes_since(0, 2, None).unwrap();
    assert_eq!(page.through_cursor, 2);
    assert_eq!(page.head_cursor, 5);
    assert!(page.has_more());
    assert_eq!(page.changes.len(), 2);
    assert!(page.changes.iter().all(|change| change.verify_digest()));

    let rest = engine
        .runtime_changes_since(page.through_cursor, 10, None)
        .unwrap();
    assert_eq!(rest.through_cursor, 5);
    assert!(!rest.has_more());
    assert_eq!(rest.changes.len(), 3);
    assert_eq!(
        rest.changes[0].previous_digest.as_deref(),
        Some(page.changes[1].digest.as_str())
    );

    let stale = commit(0, "instance:a");
    assert!(matches!(
        engine.commit_runtime(&stale),
        Err(Error::RuntimeConflict {
            expected: 0,
            actual: 5
        })
    ));
    assert_eq!(engine.runtime_cursor().unwrap(), 5);
    assert_eq!(engine.sequence().unwrap(), 1);
}

#[test]
fn all_engines_enforce_the_same_runtime_contract() {
    let dir = tempfile::tempdir().unwrap();
    let fjall = Store::open(dir.path()).unwrap();
    let native_dir = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&native_dir.path().join("native")).unwrap();
    let memory = MemoryEngine::new();
    assert_runtime_contract(&fjall);
    assert_runtime_contract(&native);
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
        mutations: vec![
            test_schema(),
            RuntimeMutation::Relation {
                relation: RuntimeRelation {
                    reference: RuntimeRef::new("caused", "missing").unwrap(),
                    from: RuntimeRef::new("prompt", "missing").unwrap(),
                    to: RuntimeRef::new("outcome", "missing").unwrap(),
                    valid_from: 100,
                    valid_to: None,
                    properties: BTreeMap::new(),
                },
            },
        ],
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
        store.runtime_changes_since(4, 1, None).unwrap().changes[0]
            .digest
            .clone()
    };
    let reopened = Store::open(dir.path()).unwrap();
    let next = RuntimeCommit {
        scope: ScopeId::new("instance:b").unwrap(),
        at: 200,
        actor: "agent:test".into(),
        expected_cursor: 5,
        mutations: vec![
            test_schema(),
            RuntimeMutation::Record {
                record: record("prompt", "p2"),
            },
        ],
    };
    reopened.commit_runtime(&next).unwrap();
    let change = reopened
        .runtime_changes_since(5, 1, None)
        .unwrap()
        .changes
        .remove(0);
    assert_eq!(
        change.previous_digest.as_deref(),
        Some(first_digest.as_str())
    );
    assert_eq!(reopened.runtime_cursor().unwrap(), 7);
}

#[test]
fn native_runtime_log_survives_flush_reopen_and_continues_its_hash_chain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native");
    let first_digest = {
        let store = NativeEngine::open(&path).unwrap();
        store.commit_runtime(&commit(0, "instance:a")).unwrap();
        store.flush(150).unwrap();
        store.runtime_changes_since(4, 1, None).unwrap().changes[0]
            .digest
            .clone()
    };
    let reopened = NativeEngine::open(&path).unwrap();
    let next = RuntimeCommit {
        scope: ScopeId::new("instance:b").unwrap(),
        at: 200,
        actor: "agent:test".into(),
        expected_cursor: 5,
        mutations: vec![
            test_schema(),
            RuntimeMutation::Record {
                record: record("prompt", "p2"),
            },
        ],
    };
    reopened.commit_runtime(&next).unwrap();
    let change = reopened
        .runtime_changes_since(5, 1, None)
        .unwrap()
        .changes
        .remove(0);
    assert_eq!(change.previous_digest.as_deref(), Some(first_digest.as_str()));
    assert_eq!(reopened.runtime_cursor().unwrap(), 7);
}

fn assert_schema_contract(engine: &dyn Engine) {
    let scope = ScopeId::new("instance:schema").unwrap();
    let error = engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 99,
            actor: "agent:schema-test".into(),
            expected_cursor: 0,
            mutations: vec![RuntimeMutation::Record {
                record: record("prompt", "ungoverned"),
            }],
        })
        .unwrap_err();
    assert!(matches!(error, Error::RuntimeSchemaMissing(_)));
    assert_eq!(engine.runtime_cursor().unwrap(), 0);

    let mut registry = RuntimeSchemaRegistry::empty(1, "bootstrap strict schema");
    registry.records.insert(
        RuntimeType::new("prompt").unwrap(),
        RuntimeRecordSchema {
            properties: BTreeMap::from([(
                "text".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            )]),
            unique_properties: std::collections::BTreeSet::from(["text".into()]),
            ..RuntimeRecordSchema::default()
        },
    );
    registry.records.insert(
        RuntimeType::new("outcome").unwrap(),
        RuntimeRecordSchema::default(),
    );
    registry.relations.insert(
        RuntimeType::new("caused").unwrap(),
        RuntimeRelationSchema {
            from: std::collections::BTreeSet::from([RuntimeType::new("prompt").unwrap()]),
            to: std::collections::BTreeSet::from([RuntimeType::new("outcome").unwrap()]),
            unique_pair: true,
            max_outgoing: Some(1),
            ..RuntimeRelationSchema::default()
        },
    );
    let mut prompt = record("prompt", "p1");
    prompt
        .properties
        .insert("text".into(), RuntimeValue::String("inspect".into()));
    let outcome_one = record("outcome", "o1");
    let outcome_two = record("outcome", "o2");
    engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 100,
            actor: "agent:schema-test".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema {
                    registry: registry.clone(),
                },
                RuntimeMutation::Record {
                    record: prompt.clone(),
                },
                RuntimeMutation::Record {
                    record: outcome_one.clone(),
                },
                RuntimeMutation::Record {
                    record: outcome_two.clone(),
                },
                RuntimeMutation::Relation {
                    relation: RuntimeRelation {
                        reference: RuntimeRef::new("caused", "c1").unwrap(),
                        from: prompt.reference.clone(),
                        to: outcome_one.reference,
                        valid_from: 100,
                        valid_to: None,
                        properties: BTreeMap::new(),
                    },
                },
            ],
        })
        .unwrap();
    assert_eq!(engine.runtime_schema(&scope).unwrap().unwrap().revision, 1);

    let mut wrong_type = record("prompt", "p2");
    wrong_type
        .properties
        .insert("text".into(), RuntimeValue::Unsigned(7));
    let error = engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 101,
            actor: "agent:schema-test".into(),
            expected_cursor: 5,
            mutations: vec![RuntimeMutation::Record { record: wrong_type }],
        })
        .unwrap_err();
    assert!(error.to_string().contains("wrong value type"));
    assert_eq!(engine.runtime_cursor().unwrap(), 5);

    let error = engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 102,
            actor: "agent:schema-test".into(),
            expected_cursor: 5,
            mutations: vec![RuntimeMutation::Relation {
                relation: RuntimeRelation {
                    reference: RuntimeRef::new("caused", "c2").unwrap(),
                    from: prompt.reference,
                    to: outcome_two.reference,
                    valid_from: 102,
                    valid_to: None,
                    properties: BTreeMap::new(),
                },
            }],
        })
        .unwrap_err();
    assert!(error.to_string().contains("max_outgoing=1"));
    assert_eq!(engine.runtime_cursor().unwrap(), 5);

    let mut migrated = registry;
    migrated.revision = 2;
    migrated.migration = "document compatible schema evolution".into();
    engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 103,
            actor: "agent:schema-test".into(),
            expected_cursor: 5,
            mutations: vec![RuntimeMutation::Schema { registry: migrated }],
        })
        .unwrap();
    assert_eq!(engine.runtime_schema(&scope).unwrap().unwrap().revision, 2);
}

#[test]
fn all_engines_enforce_schema_types_cardinality_and_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let fjall = Store::open(dir.path()).unwrap();
    let native_dir = tempfile::tempdir().unwrap();
    let native_path = native_dir.path().join("native");
    let native = NativeEngine::open(&native_path).unwrap();
    let memory = MemoryEngine::new();
    assert_schema_contract(&fjall);
    assert_schema_contract(&native);
    assert_schema_contract(&memory);

    drop(fjall);
    let reopened = Store::open(dir.path()).unwrap();
    assert_eq!(
        reopened
            .runtime_schema(&ScopeId::new("instance:schema").unwrap())
            .unwrap()
            .unwrap()
            .revision,
        2
    );
    drop(native);
    let reopened = NativeEngine::open(&native_path).unwrap();
    assert_eq!(
        reopened
            .runtime_schema(&ScopeId::new("instance:schema").unwrap())
            .unwrap()
            .unwrap()
            .revision,
        2
    );
}
