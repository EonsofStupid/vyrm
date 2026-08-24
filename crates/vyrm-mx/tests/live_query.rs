use std::collections::BTreeMap;
use vyrm_core::{
    RuntimeCommit, RuntimeMutation, RuntimeProperties, RuntimePropertySchema, RuntimeRecord,
    RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType, RuntimeValue,
    RuntimeValueType, ScopeId,
};
use vyrm_mx::{poll_live_query, Error, LiveQueryBudget, LiveQueryDelta, Parameters};
use vyrm_ql::parse;
use vyrm_store::{Engine, MemoryEngine, NativeEngine, Store};

fn scope() -> ScopeId {
    ScopeId::new("instance:live-test").unwrap()
}

fn record(id: &str, status: &str) -> RuntimeRecord {
    RuntimeRecord {
        reference: RuntimeRef::new("document", id).unwrap(),
        valid_from: 10,
        valid_to: None,
        properties: RuntimeProperties::from([(
            "status".into(),
            RuntimeValue::String(status.into()),
        )]),
    }
}

fn seed<E: Engine>(engine: &E) {
    let mut registry = RuntimeSchemaRegistry::empty(1, "live query fixture");
    registry.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema {
            properties: BTreeMap::from([(
                "status".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            )]),
            ..RuntimeRecordSchema::default()
        },
    );
    engine
        .commit_runtime(&RuntimeCommit {
            scope: scope(),
            at: 10,
            actor: "test".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema { registry },
                RuntimeMutation::Record {
                    record: record("a", "open"),
                },
            ],
        })
        .unwrap();
}

fn exercise<E: Engine>(engine: &E) -> LiveQueryDelta {
    seed(engine);
    engine
        .commit_runtime(&RuntimeCommit {
            scope: scope(),
            at: 20,
            actor: "test".into(),
            expected_cursor: 2,
            mutations: vec![
                RuntimeMutation::Record {
                    record: record("a", "closed"),
                },
                RuntimeMutation::Record {
                    record: record("b", "open"),
                },
            ],
        })
        .unwrap();
    let query =
        parse("FROM record:document AT VALID 100 KNOWN HEAD PROJECT id, status EXPLAIN CONTRACT")
            .unwrap();
    let delta = poll_live_query(
        engine,
        &scope(),
        &query,
        &Parameters::new(),
        2,
        &LiveQueryBudget::default(),
    )
    .unwrap();
    assert_eq!(delta.from_cursor, 2);
    assert_eq!(delta.through_cursor, 4);
    assert_eq!(delta.head_cursor, 4);
    assert_eq!(delta.change_count(), 2);
    assert_eq!(delta.added[0].identity, "record:document:b");
    assert_eq!(delta.updated[0].before.identity, "record:document:a");
    assert_eq!(
        delta.updated[0].before.values["status"],
        RuntimeValue::String("open".into())
    );
    assert_eq!(
        delta.updated[0].after.values["status"],
        RuntimeValue::String("closed".into())
    );
    assert!(delta.removed.is_empty());
    let open_query = parse(
        "FROM record:document AT VALID 100 KNOWN HEAD WHERE status = \"open\" PROJECT id, status",
    )
    .unwrap();
    let membership = poll_live_query(
        engine,
        &scope(),
        &open_query,
        &Parameters::new(),
        2,
        &LiveQueryBudget::default(),
    )
    .unwrap();
    assert_eq!(membership.added[0].identity, "record:document:b");
    assert_eq!(membership.removed[0].identity, "record:document:a");
    assert!(membership.updated.is_empty());
    assert!(poll_live_query(
        engine,
        &scope(),
        &query,
        &Parameters::new(),
        4,
        &LiveQueryBudget::default(),
    )
    .unwrap()
    .is_empty());
    assert!(matches!(
        poll_live_query(
            engine,
            &scope(),
            &query,
            &Parameters::new(),
            2,
            &LiveQueryBudget {
                max_delta_rows: 1,
                ..LiveQueryBudget::default()
            },
        ),
        Err(Error::Budget(_))
    ));
    delta
}

#[test]
fn semantic_deltas_are_identical_across_every_engine() {
    let expected = exercise(&MemoryEngine::new());
    let fjall_root = tempfile::tempdir().unwrap();
    assert_eq!(expected, exercise(&Store::open(fjall_root.path()).unwrap()));
    let native_root = tempfile::tempdir().unwrap();
    assert_eq!(
        expected,
        exercise(&NativeEngine::open(&native_root.path().join("native")).unwrap())
    );
}

#[test]
fn resume_and_snapshot_contracts_fail_closed() {
    let engine = MemoryEngine::new();
    seed(&engine);
    let fixed = parse("FROM record:document AT VALID 100 KNOWN 2 PROJECT id").unwrap();
    assert!(matches!(
        poll_live_query(
            &engine,
            &scope(),
            &fixed,
            &Parameters::new(),
            0,
            &LiveQueryBudget::default(),
        ),
        Err(Error::Binding(_))
    ));
    let live = parse("FROM record:document AT VALID 100 KNOWN HEAD PROJECT id").unwrap();
    assert!(matches!(
        poll_live_query(
            &engine,
            &scope(),
            &live,
            &Parameters::new(),
            3,
            &LiveQueryBudget::default(),
        ),
        Err(Error::Binding(_))
    ));
    assert!(poll_live_query(
        &engine,
        &scope(),
        &live,
        &Parameters::new(),
        0,
        &LiveQueryBudget {
            execution: vyrm_mx::ExecutionBudget {
                max_rows: 1,
                ..vyrm_mx::ExecutionBudget::default()
            },
            max_delta_rows: 10,
        },
    )
    .is_ok());
}
