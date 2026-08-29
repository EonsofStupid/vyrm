use rrd_core::{
    Claim, GeoPoint, GeoValue, Predicate, Producer, RuntimeCommit, RuntimeEvent,
    RuntimeEventSchema, RuntimeGeo, RuntimeGraphSnapshot, RuntimeMutation, RuntimeProperties,
    RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeRelation,
    RuntimeRelationSchema, RuntimeSchemaRegistry, RuntimeSeriesSample, RuntimeType, RuntimeValue,
    RuntimeValueType, ScopeId, SeriesValue, Subject,
};
use rrd_query::{
    bind, execute, plan, Catalog, Error, ExecutionBudget, Parameters, PhysicalOperator,
};
use rrd_query::{
    parse, ComparisonOperator, CursorExpr, Projection, Query, Source, TemporalSelector, TimeExpr,
};
use rrd_store::{Engine, MemoryEngine, NativeEngine, Store};
use std::collections::{BTreeMap, BTreeSet};

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
    registry.records.insert(
        RuntimeType::new("metric").unwrap(),
        RuntimeRecordSchema::default(),
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
            RuntimeMutation::Record {
                record: RuntimeRecord {
                    reference: RuntimeRef::new("metric", "latency").unwrap(),
                    valid_from: 10,
                    valid_to: None,
                    properties: RuntimeProperties::new(),
                },
            },
            RuntimeMutation::SeriesSample {
                sample: RuntimeSeriesSample {
                    reference: RuntimeRef::new("sample", "latency-100").unwrap(),
                    series: RuntimeRef::new("metric", "latency").unwrap(),
                    observed_at: 100,
                    value: SeriesValue::Decimal("12.5".into()),
                    properties: RuntimeProperties::new(),
                },
            },
            RuntimeMutation::Geo {
                geo: RuntimeGeo {
                    reference: RuntimeRef::new("location", "alpha").unwrap(),
                    subject: RuntimeRef::new("document", "a").unwrap(),
                    field: "position".into(),
                    valid_from: 10,
                    valid_to: None,
                    value: GeoValue::Point {
                        point: GeoPoint {
                            longitude: -122.4,
                            latitude: 37.8,
                        },
                    },
                    properties: RuntimeProperties::new(),
                },
            },
            RuntimeMutation::Relation {
                relation: RuntimeRelation {
                    reference: RuntimeRef::new("depends_on", "b-a-cycle").unwrap(),
                    from: RuntimeRef::new("document", "b").unwrap(),
                    to: RuntimeRef::new("document", "a").unwrap(),
                    valid_from: 20,
                    valid_to: None,
                    properties: properties(&[("strength", "cycle")]),
                },
            },
        ],
    }
}

fn execute_fixture<E: Engine>(engine: &E, text: &str) -> rrd_query::QueryExecution {
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

fn execute_text<E: Engine>(
    engine: &E,
    text: &str,
) -> (rrd_query::PhysicalPlan, rrd_query::QueryExecution) {
    let catalog = Catalog::capture(engine, &ScopeId::new("instance:test").unwrap()).unwrap();
    let query = parse(text).unwrap();
    let physical = plan(&bind(&query, &Parameters::new(), &catalog).unwrap()).unwrap();
    let execution = execute(engine, &physical, &ExecutionBudget::default()).unwrap();
    (physical, execution)
}

fn seed_historical_corrections<E: Engine>(engine: &E) {
    let first = fixture_commit();
    let head = engine.commit_runtime(&first).unwrap().last_cursor;
    engine
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new("instance:test").unwrap(),
            at: 101,
            actor: "agent:correction".into(),
            expected_cursor: head,
            mutations: vec![
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: RuntimeRef::new("document", "a").unwrap(),
                        valid_from: 10,
                        valid_to: None,
                        properties: properties(&[("status", "open"), ("title", "Alpha corrected")]),
                    },
                },
                RuntimeMutation::Claim {
                    claim: Claim::new(
                        Subject::new("document:a").unwrap(),
                        Predicate::new("status").unwrap(),
                        "corrected",
                        30,
                        101,
                        Producer {
                            actor: "agent:correction".into(),
                            on_behalf_of: None,
                            session: Some("correction".into()),
                        },
                    ),
                },
            ],
        })
        .unwrap();
}

fn historical_outcome<E: Engine>(engine: &E) -> (String, String, String, String, u64, u64) {
    let (historical_plan, historical_record) = execute_text(
        engine,
        "FROM record:document AT VALID 100 KNOWN 10 WHERE id = \"a\" PROJECT title EXPLAIN CONTRACT",
    );
    let (_, current_record) = execute_text(
        engine,
        "FROM record:document AT VALID 100 KNOWN HEAD WHERE id = \"a\" PROJECT title EXPLAIN CONTRACT",
    );
    let (_, historical_claim) = execute_text(
        engine,
        "FROM claim:status AT VALID 100 KNOWN 10 PROJECT object EXPLAIN CONTRACT",
    );
    let (_, current_claim) = execute_text(
        engine,
        "FROM claim:status AT VALID 100 KNOWN HEAD PROJECT object EXPLAIN CONTRACT",
    );
    let string_field = |execution: &rrd_query::QueryExecution, field: &str| {
        let RuntimeValue::String(value) = &execution.batches[0].rows[0].values[field] else {
            panic!("{field} was not a string")
        };
        value.clone()
    };
    (
        string_field(&historical_record, "title"),
        string_field(&current_record, "title"),
        string_field(&historical_claim, "object"),
        string_field(&current_claim, "object"),
        historical_record.known_at_cursor,
        historical_plan.explanation.contract.source_cursor,
    )
}

#[test]
fn memory_fjall_and_native_return_identical_exact_rows() {
    let memory = MemoryEngine::new();
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let native_dir = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&native_dir.path().join("native")).unwrap();
    let text = "FROM record:document AT VALID 100 KNOWN HEAD WHERE status = \"open\" PROJECT id, title EXPLAIN CONTRACT";
    let left = execute_fixture(&memory, text);
    let right = execute_fixture(&store, text);
    let native_result = execute_fixture(&native, text);
    assert_eq!(left, right);
    assert_eq!(left, native_result);
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
fn valid_time_and_known_at_are_stable_across_engines_and_native_reopen() {
    let memory = MemoryEngine::new();
    seed_historical_corrections(&memory);
    let expected = historical_outcome(&memory);

    let fjall_root = tempfile::tempdir().unwrap();
    let fjall = Store::open(fjall_root.path()).unwrap();
    seed_historical_corrections(&fjall);
    assert_eq!(historical_outcome(&fjall), expected);

    let native_root = tempfile::tempdir().unwrap();
    let native_path = native_root.path().join("native");
    {
        let native = NativeEngine::open(&native_path).unwrap();
        seed_historical_corrections(&native);
        assert_eq!(historical_outcome(&native), expected);
    }
    let reopened = NativeEngine::open(&native_path).unwrap();
    assert_eq!(historical_outcome(&reopened), expected);
    assert_eq!(
        expected,
        (
            "Alpha".into(),
            "Alpha corrected".into(),
            "ready".into(),
            "corrected".into(),
            10,
            3,
        )
    );
}

#[test]
fn series_and_geo_queries_match_every_persistent_engine() {
    for (text, field, expected) in [
        (
            "FROM series:metric AT VALID 100 KNOWN HEAD WHERE series_id = \"latency\" PROJECT observed_at, value EXPLAIN CONTRACT",
            "value",
            RuntimeValue::Decimal("12.5".into()),
        ),
        (
            "FROM geo:location AT VALID 100 KNOWN HEAD WHERE subject_id = \"a\" PROJECT geometry_kind, longitude, latitude EXPLAIN CONTRACT",
            "longitude",
            RuntimeValue::Decimal("-122.4".into()),
        ),
    ] {
        let memory = MemoryEngine::new();
        let fjall_root = tempfile::tempdir().unwrap();
        let fjall = Store::open(fjall_root.path()).unwrap();
        let native_root = tempfile::tempdir().unwrap();
        let native = NativeEngine::open(&native_root.path().join("native")).unwrap();
        let left = execute_fixture(&memory, text);
        assert_eq!(left, execute_fixture(&fjall, text), "{text}");
        assert_eq!(left, execute_fixture(&native, text), "{text}");
        assert_eq!(left.returned_rows, 1, "{text}");
        assert_eq!(left.batches[0].rows[0].values[field], expected, "{text}");
    }

    for text in [
        "FROM series:metric AT VALID 99 KNOWN HEAD PROJECT *",
        "FROM geo:location AT VALID 9 KNOWN HEAD PROJECT *",
    ] {
        let engine = MemoryEngine::new();
        assert_eq!(execute_fixture(&engine, text).returned_rows, 0, "{text}");
    }
}

#[test]
fn typed_comparisons_match_every_persistent_engine_and_reject_unsupported_ordering() {
    let text = "FROM series:metric AT VALID 100 KNOWN HEAD WHERE observed_at >= 100 AND series_id != \"other\" PROJECT series_id, observed_at EXPLAIN CONTRACT";
    let memory = MemoryEngine::new();
    let fjall_root = tempfile::tempdir().unwrap();
    let fjall = Store::open(fjall_root.path()).unwrap();
    let native_root = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&native_root.path().join("native")).unwrap();
    let expected = execute_fixture(&memory, text);
    assert_eq!(expected, execute_fixture(&fjall, text));
    assert_eq!(expected, execute_fixture(&native, text));
    assert_eq!(expected.returned_rows, 1);
    assert_eq!(
        expected.batches[0].rows[0].values["observed_at"],
        RuntimeValue::Unsigned(100)
    );
    for predicate in [
        "observed_at = 100",
        "observed_at != 101",
        "observed_at < 101",
        "observed_at <= 100",
        "observed_at > 99",
        "observed_at >= 100",
    ] {
        let engine = MemoryEngine::new();
        let query = format!(
            "FROM series:metric AT VALID 100 KNOWN HEAD WHERE {predicate} PROJECT observed_at"
        );
        assert_eq!(execute_fixture(&engine, &query).returned_rows, 1, "{query}");
    }

    let parsed = parse(text).unwrap();
    assert_eq!(parsed.filters[1].comparison, ComparisonOperator::NotEqual);

    let catalog = Catalog::capture(&memory, &ScopeId::new("instance:test").unwrap()).unwrap();
    let decimal_ordering =
        parse("FROM series:metric AT VALID 100 KNOWN HEAD WHERE value > $minimum PROJECT value")
            .unwrap();
    let parameters = Parameters::from([("minimum".into(), RuntimeValue::Decimal("10.0".into()))]);
    assert!(matches!(
        bind(&decimal_ordering, &parameters, &catalog),
        Err(Error::Binding(_))
    ));
}

#[test]
fn bounded_graph_traversal_is_deterministic_across_every_engine() {
    for (text, expected_node) in [
        (
            "FROM traverse:depends_on START document:a DIRECTION OUTGOING DEPTH 3 AT VALID 100 KNOWN HEAD PROJECT node_id, depth, path EXPLAIN CONTRACT",
            "b",
        ),
        (
            "FROM traverse:depends_on START document:b DIRECTION INCOMING DEPTH 3 AT VALID 100 KNOWN HEAD PROJECT node_id, depth, path EXPLAIN CONTRACT",
            "a",
        ),
    ] {
        let memory = MemoryEngine::new();
        let fjall_root = tempfile::tempdir().unwrap();
        let fjall = Store::open(fjall_root.path()).unwrap();
        let native_root = tempfile::tempdir().unwrap();
        let native = NativeEngine::open(&native_root.path().join("native")).unwrap();
        let left = execute_fixture(&memory, text);
        assert_eq!(left, execute_fixture(&fjall, text), "{text}");
        assert_eq!(left, execute_fixture(&native, text), "{text}");
        assert_eq!(left.returned_rows, 1, "{text}");
        assert_eq!(
            left.batches[0].rows[0].values["node_id"],
            value(expected_node)
        );
        assert_eq!(
            left.batches[0].rows[0].values["depth"],
            RuntimeValue::Unsigned(1)
        );
    }
}

#[test]
fn all_source_families_execute_at_explicit_time() {
    let engine = MemoryEngine::new();
    engine.commit_runtime(&fixture_commit()).unwrap();
    let catalog = Catalog::capture(&engine, &ScopeId::new("instance:test").unwrap()).unwrap();
    for (text, identity) in [
        (
            "FROM relation:depends_on AT VALID 100 KNOWN HEAD WHERE id = \"a-b\" PROJECT id",
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
        (
            "FROM series:metric AT VALID 100 KNOWN HEAD WHERE series_id = \"latency\" PROJECT observed_at, value",
            "series:metric:latency:100:latency-100",
        ),
        (
            "FROM geo:location AT VALID 100 KNOWN HEAD WHERE subject_id = \"a\" PROJECT geometry_kind, longitude, latitude",
            "geo:location:alpha",
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
fn bound_event_cursor_uses_one_exact_authoritative_position_on_every_engine() {
    fn exercise<E: Engine>(engine: &E) -> rrd_query::QueryExecution {
        engine.commit_runtime(&fixture_commit()).unwrap();
        let catalog = Catalog::capture(engine, &ScopeId::new("instance:test").unwrap()).unwrap();
        let query = parse(
            "FROM event:tool_result AT VALID 100 KNOWN HEAD WHERE cursor = 5 AND ok = true PROJECT cursor",
        )
        .unwrap();
        let physical = plan(&bind(&query, &Parameters::new(), &catalog).unwrap()).unwrap();
        assert!(matches!(
            physical.operators.as_slice(),
            [
                PhysicalOperator::AuthoritativeEventCursorLookup {
                    cursor: 5,
                    through_cursor: 10,
                    exact: true,
                    ..
                },
                PhysicalOperator::DataFusionEvaluate
            ]
        ));
        assert_eq!(
            physical
                .explanation
                .candidates
                .iter()
                .find(|candidate| candidate.selected)
                .unwrap()
                .name,
            "authoritative_event_cursor_lookup"
        );
        execute(
            engine,
            &physical,
            &ExecutionBudget {
                max_scanned_changes: 1,
                ..ExecutionBudget::default()
            },
        )
        .unwrap()
    }

    let memory = MemoryEngine::new();
    let fjall_root = tempfile::tempdir().unwrap();
    let fjall = Store::open(fjall_root.path()).unwrap();
    let native_root = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&native_root.path().join("native")).unwrap();
    let expected = exercise(&memory);
    assert_eq!(expected, exercise(&fjall));
    assert_eq!(expected, exercise(&native));
    assert_eq!(expected.scanned_changes, 1);
    assert_eq!(expected.stamp_validation, "rfc9162_inclusion_proof");
    assert_eq!(expected.stamp_validation_max_changes, 1);
    assert!(expected.stamp_validation_proof_nodes > 0);
    assert_eq!(expected.returned_rows, 1);
    assert_eq!(expected.batches[0].rows[0].identity, "event:tool_result:5");
}

#[test]
fn event_cursor_outside_the_stamp_is_an_exact_empty_path() {
    let engine = MemoryEngine::new();
    engine.commit_runtime(&fixture_commit()).unwrap();
    let catalog = Catalog::capture(&engine, &ScopeId::new("instance:test").unwrap()).unwrap();
    let query =
        parse("FROM event:tool_result AT VALID 100 KNOWN HEAD WHERE cursor = 99 PROJECT cursor")
            .unwrap();
    let physical = plan(&bind(&query, &Parameters::new(), &catalog).unwrap()).unwrap();
    let result = execute(
        &engine,
        &physical,
        &ExecutionBudget {
            max_scanned_changes: 1,
            ..ExecutionBudget::default()
        },
    )
    .unwrap();
    assert_eq!(result.scanned_changes, 0);
    assert_eq!(result.returned_rows, 0);
}

#[test]
fn cursor_lookup_uses_authenticated_logarithmic_validation() {
    const EVENTS: u64 = 4_096;
    let engine = MemoryEngine::new();
    let fixture_head = engine
        .commit_runtime(&fixture_commit())
        .unwrap()
        .last_cursor;
    let subject = RuntimeRef::new("document", "a").unwrap();
    engine
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new("instance:test").unwrap(),
            at: 101,
            actor: "agent:bulk".into(),
            expected_cursor: fixture_head,
            mutations: (0..EVENTS)
                .map(|_| RuntimeMutation::Event {
                    event: RuntimeEvent {
                        kind: RuntimeType::new("tool_result").unwrap(),
                        subject: Some(subject.clone()),
                        properties: BTreeMap::from([("ok".into(), RuntimeValue::Bool(true))]),
                    },
                })
                .collect(),
        })
        .unwrap();
    let catalog = Catalog::capture(&engine, &ScopeId::new("instance:test").unwrap()).unwrap();
    let head = fixture_head + EVENTS;

    let point = parse(&format!(
        "FROM event:tool_result AT VALID 101 KNOWN HEAD WHERE cursor = {head} PROJECT cursor"
    ))
    .unwrap();
    let point_plan = plan(&bind(&point, &Parameters::new(), &catalog).unwrap()).unwrap();
    let point_result = execute(
        &engine,
        &point_plan,
        &ExecutionBudget {
            max_scanned_changes: 1,
            ..ExecutionBudget::default()
        },
    )
    .unwrap();
    assert_eq!(point_result.scanned_changes, 1);
    assert_eq!(point_result.stamp_validation, "rfc9162_inclusion_proof");
    assert_eq!(point_result.stamp_validation_max_changes, 1);
    assert!(point_result.stamp_validation_proof_nodes > 0);
    assert!(point_result.stamp_validation_proof_nodes <= 13);
    assert_eq!(point_result.returned_rows, 1);

    let unbound =
        parse("FROM event:tool_result AT VALID 101 KNOWN HEAD WHERE ok = true PROJECT cursor")
            .unwrap();
    let unbound_plan = plan(&bind(&unbound, &Parameters::new(), &catalog).unwrap()).unwrap();
    assert!(matches!(
        execute(
            &engine,
            &unbound_plan,
            &ExecutionBudget {
                max_scanned_changes: 1,
                ..ExecutionBudget::default()
            }
        ),
        Err(Error::Budget(_))
    ));
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
