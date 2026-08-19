use std::collections::{BTreeMap, BTreeSet};
use vyrm_core::{
    Claim, Predicate, Producer, RuntimeCommit, RuntimeEvent, RuntimeEventSchema,
    RuntimeGraphSnapshot, RuntimeMutation, RuntimeProperties, RuntimePropertySchema, RuntimeRecord,
    RuntimeRecordSchema, RuntimeRef, RuntimeRelation, RuntimeRelationSchema, RuntimeSchemaRegistry,
    RuntimeType, RuntimeValue, RuntimeValueType, ScopeId, Subject,
};
use vyrm_mx::{bind, execute, plan, Catalog, Error, ExecutionBudget, Parameters};
use vyrm_ql::{parse, CursorExpr, Projection, Query, Source, TemporalSelector, TimeExpr};
use vyrm_store::{Engine, MemoryEngine, Store};

fn value(value: &str) -> RuntimeValue {
    RuntimeValue::String(value.into())
}

fn properties(values: &[(&str, &str)]) -> RuntimeProperties {
    values
        .iter()
        .map(|(key, value)| ((*key).into(), RuntimeValue::String((*value).into())))
        .collect()
}

fn schema() -> RuntimeMutation {
    let mut registry = RuntimeSchemaRegistry::empty(1, "query fixture");
    registry.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema {
            properties: BTreeMap::from([
                (
                    "status".into(),
                    RuntimePropertySchema::required(RuntimeValueType::String),
                ),
                (
                    "title".into(),
                    RuntimePropertySchema::required(RuntimeValueType::String),
                ),
            ]),
            ..RuntimeRecordSchema::default()
        },
    );
    registry.relations.insert(
        RuntimeType::new("depends_on").unwrap(),
        RuntimeRelationSchema {
            from: BTreeSet::from([RuntimeType::new("document").unwrap()]),
            to: BTreeSet::from([RuntimeType::new("document").unwrap()]),
            properties: BTreeMap::from([(
                "strength".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            )]),
            ..RuntimeRelationSchema::default()
        },
    );
    registry.events.insert(
        RuntimeType::new("tool_result").unwrap(),
        RuntimeEventSchema {
            subject_required: true,
            subject_types: BTreeSet::from([RuntimeType::new("document").unwrap()]),
            properties: BTreeMap::from([(
                "ok".into(),
                RuntimePropertySchema::required(RuntimeValueType::Bool),
            )]),
            ..RuntimeEventSchema::default()
        },
    );
    RuntimeMutation::Schema { registry }
}

fn fixture_commit() -> RuntimeCommit {
    let first = RuntimeRef::new("document", "a").unwrap();
    let second = RuntimeRef::new("document", "b").unwrap();
    RuntimeCommit {
        scope: ScopeId::new("instance:test").unwrap(),
        at: 100,
        actor: "agent:test".into(),
        expected_cursor: 0,
        mutations: vec![
            schema(),
            RuntimeMutation::Record {
                record: RuntimeRecord {
                    reference: first.clone(),
                    valid_from: 10,
                    valid_to: None,
                    properties: properties(&[("status", "open"), ("title", "Alpha")]),
                },
            },
            RuntimeMutation::Record {
                record: RuntimeRecord {
                    reference: second.clone(),
                    valid_from: 20,
                    valid_to: None,
                    properties: properties(&[("status", "closed"), ("title", "Beta")]),
                },
            },
            RuntimeMutation::Relation {
                relation: RuntimeRelation {
                    reference: RuntimeRef::new("depends_on", "a-b").unwrap(),
                    from: first.clone(),
                    to: second,
                    valid_from: 20,
                    valid_to: None,
                    properties: properties(&[("strength", "hard")]),
                },
            },
            RuntimeMutation::Event {
                event: RuntimeEvent {
                    kind: RuntimeType::new("tool_result").unwrap(),
                    subject: Some(first),
                    properties: BTreeMap::from([("ok".into(), RuntimeValue::Bool(true))]),
                },
            },
            RuntimeMutation::Claim {
                claim: Claim::new(
                    Subject::new("document:a").unwrap(),
                    Predicate::new("status").unwrap(),
                    "ready",
                    30,
                    100,
                    Producer {
                        actor: "agent:test".into(),
                        on_behalf_of: None,
                        session: Some("fixture".into()),
                    },
                ),
            },
        ],
    }
}

fn execute_fixture<E: Engine>(engine: &E, text: &str) -> vyrm_mx::QueryExecution {
    engine.commit_runtime(&fixture_commit()).unwrap();
    let catalog = Catalog::capture(engine, &ScopeId::new("instance:test").unwrap()).unwrap();
    let query = parse(text).unwrap();
    execute(
        engine,
        &plan(&bind(&query, &Parameters::new(), &catalog).unwrap()).unwrap(),
        &ExecutionBudget::default(),
    )
    .unwrap()
}

#[test]
fn memory_and_fjall_return_identical_exact_rows() {
    let memory = MemoryEngine::new();
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let text = "FROM record:document AT VALID 100 KNOWN HEAD WHERE status = \"open\" PROJECT id, title EXPLAIN CONTRACT";
    let left = execute_fixture(&memory, text);
    let right = execute_fixture(&store, text);
    assert_eq!(left, right);
    assert_eq!(left.returned_rows, 1);
    assert_eq!(left.batches[0].rows[0].values["id"], value("a"));
    assert_eq!(left.batches[0].rows[0].values["title"], value("Alpha"));

    let read = memory
        .runtime_read_stamp(&ScopeId::new("instance:test").unwrap())
        .unwrap();
    let page = memory
        .runtime_read_changes(&read, 0, read.commit_cursor as usize)
        .unwrap();
    let direct =
        RuntimeGraphSnapshot::from_changes(&page.changes, read.scope, 100, read.commit_cursor);
    let direct_ids = direct
        .records
        .iter()
        .filter(|record| {
            record.reference.kind.as_str() == "document"
                && record.properties.get("status") == Some(&value("open"))
        })
        .map(|record| record.reference.id.to_string())
        .collect::<Vec<_>>();
    let query_ids = left.batches[0]
        .rows
        .iter()
        .map(|row| match &row.values["id"] {
            RuntimeValue::String(id) => id.clone(),
            value => panic!("unexpected query id {value:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        query_ids, direct_ids,
        "query must match the direct graph API"
    );
}

#[test]
fn all_source_families_execute_at_explicit_time() {
    let engine = MemoryEngine::new();
    engine.commit_runtime(&fixture_commit()).unwrap();
    let catalog = Catalog::capture(&engine, &ScopeId::new("instance:test").unwrap()).unwrap();
    for (text, identity) in [
        (
            "FROM relation:depends_on AT VALID 100 KNOWN HEAD PROJECT id",
            "relation:depends_on:a-b",
        ),
        (
            "FROM event:tool_result AT VALID 100 KNOWN HEAD WHERE ok = true PROJECT cursor",
            "event:tool_result:5",
        ),
        (
            "FROM claim:status AT VALID 100 KNOWN HEAD PROJECT subject, object",
            "claim:document:a:status",
        ),
    ] {
        let query = parse(text).unwrap();
        let result = execute(
            &engine,
            &plan(&bind(&query, &Parameters::new(), &catalog).unwrap()).unwrap(),
            &ExecutionBudget::default(),
        )
        .unwrap();
        assert_eq!(result.returned_rows, 1, "{text}");
        assert_eq!(result.batches[0].rows[0].identity, identity);
    }
}

#[test]
fn text_and_typed_sdk_produce_the_same_plan() {
    let engine = MemoryEngine::new();
    engine.commit_runtime(&fixture_commit()).unwrap();
    let catalog = Catalog::capture(&engine, &ScopeId::new("instance:test").unwrap()).unwrap();
    let parsed = parse("FROM record:document AT VALID 100 KNOWN HEAD PROJECT id").unwrap();
    let mut typed = Query::new(
        Source::Record {
            kind: RuntimeType::new("document").unwrap(),
        },
        TemporalSelector {
            valid_at: TimeExpr::Literal(100),
            known_at: CursorExpr::Head,
        },
    );
    typed.projection = Projection::Fields(vec!["id".into()]);
    let parsed_plan = plan(&bind(&parsed, &Parameters::new(), &catalog).unwrap()).unwrap();
    let typed_plan = plan(&bind(&typed, &Parameters::new(), &catalog).unwrap()).unwrap();
    assert_eq!(parsed_plan, typed_plan);
}

#[test]
fn binding_and_budget_fail_closed() {
    let engine = MemoryEngine::new();
    engine.commit_runtime(&fixture_commit()).unwrap();
    let catalog = Catalog::capture(&engine, &ScopeId::new("instance:test").unwrap()).unwrap();
    let unknown = parse("FROM record:document AT VALID 100 KNOWN HEAD PROJECT missing").unwrap();
    assert!(matches!(
        bind(&unknown, &Parameters::new(), &catalog),
        Err(Error::Binding(_))
    ));
    let wrong_type =
        parse("FROM record:document AT VALID 100 KNOWN HEAD WHERE status = true PROJECT id")
            .unwrap();
    assert!(matches!(
        bind(&wrong_type, &Parameters::new(), &catalog),
        Err(Error::Binding(_))
    ));

    let query = parse("FROM record:document AT VALID 100 KNOWN HEAD PROJECT *").unwrap();
    let physical = plan(&bind(&query, &Parameters::new(), &catalog).unwrap()).unwrap();
    assert!(matches!(
        execute(
            &engine,
            &physical,
            &ExecutionBudget {
                max_scanned_changes: 5,
                ..ExecutionBudget::default()
            }
        ),
        Err(Error::Budget(_))
    ));
    let mut forged = physical;
    forged.explanation.contract.exact = false;
    assert!(matches!(
        execute(&engine, &forged, &ExecutionBudget::default()),
        Err(Error::Integrity(_))
    ));

    let limited = parse("FROM record:document AT VALID 100 KNOWN HEAD PROJECT id LIMIT 1").unwrap();
    let result = execute(
        &engine,
        &plan(&bind(&limited, &Parameters::new(), &catalog).unwrap()).unwrap(),
        &ExecutionBudget::default(),
    )
    .unwrap();
    assert_eq!(result.returned_rows, 1);
    assert!(
        !result.truncated,
        "a semantic LIMIT is not budget truncation"
    );
}
