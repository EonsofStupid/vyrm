use std::collections::BTreeMap;
use vyrm_core::{
    RuntimeCommit, RuntimeMutation, RuntimeProperties, RuntimePropertySchema, RuntimeRecord,
    RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType, RuntimeValue,
    RuntimeValueType, ScopeId,
};
use vyrm_node::{execute_traced_query, query_parameters_from_json, ExecutionBudget, Parameters};
use vyrm_store::{Engine, MemoryEngine, NativeEngine, Store};

fn scope() -> ScopeId {
    ScopeId::new("instance:query-trace").unwrap()
}

fn fixture<E: Engine>(store: &E) {
    let mut registry = RuntimeSchemaRegistry::empty(1, "traced query fixture");
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
    store
        .commit_runtime(&RuntimeCommit {
            scope: scope(),
            at: 10,
            actor: "fixture".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema { registry },
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: RuntimeRef::new("document", "a").unwrap(),
                        valid_from: 1,
                        valid_to: None,
                        properties: RuntimeProperties::from([
                            (
                                "status".into(),
                                RuntimeValue::String("operator-secret".into()),
                            ),
                            ("title".into(), RuntimeValue::String("Alpha".into())),
                        ]),
                    },
                },
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: RuntimeRef::new("document", "b").unwrap(),
                        valid_from: 1,
                        valid_to: None,
                        properties: RuntimeProperties::from([
                            ("status".into(), RuntimeValue::String("closed".into())),
                            ("title".into(), RuntimeValue::String("Beta".into())),
                        ]),
                    },
                },
            ],
        })
        .unwrap();
}

#[derive(Debug, PartialEq, Eq)]
struct TraceView {
    name: String,
    phase: String,
    domain: String,
    outcome: String,
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    attributes: RuntimeProperties,
    encoded: String,
}

fn trace_views<E: Engine>(store: &E) -> Vec<TraceView> {
    store
        .runtime_changes_since(0, usize::MAX, Some(&scope()))
        .unwrap()
        .changes
        .into_iter()
        .filter_map(|change| match change.mutation {
            RuntimeMutation::Event { event } if event.kind.as_str() == "runtime_trace" => {
                let string = |name: &str| match &event.properties[name] {
                    RuntimeValue::String(value) => value.clone(),
                    value => panic!("{name} had unexpected value {value:?}"),
                };
                let parent_span_id = event.properties.get("parent_span_id").map(|value| {
                    let RuntimeValue::String(value) = value else {
                        panic!("parent_span_id had unexpected value {value:?}")
                    };
                    value.clone()
                });
                let RuntimeValue::Map(attributes) = &event.properties["attributes"] else {
                    panic!("trace attributes had the wrong shape")
                };
                Some(TraceView {
                    name: string("name"),
                    phase: string("phase"),
                    domain: string("domain"),
                    outcome: string("outcome"),
                    trace_id: string("trace_id"),
                    span_id: string("span_id"),
                    parent_span_id,
                    attributes: attributes.clone(),
                    encoded: serde_json::to_string(&event).unwrap(),
                })
            }
            _ => None,
        })
        .collect()
}

fn parameters() -> Parameters {
    query_parameters_from_json(&serde_json::json!({"status":"operator-secret"})).unwrap()
}

fn exercise<E: Engine>(store: &E) -> (vyrm_node::TracedQueryExecution, Vec<TraceView>) {
    fixture(store);
    let result = execute_traced_query(
        store,
        scope(),
        "FROM record:document AT VALID 10 KNOWN HEAD WHERE status = $status PROJECT id, title EXPLAIN CONTRACT",
        &parameters(),
        &ExecutionBudget::default(),
        "operator:test",
        100,
    )
    .unwrap();
    (result, trace_views(store))
}

#[test]
fn traced_query_is_observer_safe_causal_and_equal_across_all_engines() {
    let memory = MemoryEngine::new();
    let fjall_root = tempfile::tempdir().unwrap();
    let fjall = Store::open(fjall_root.path()).unwrap();
    let native_root = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&native_root.path().join("native")).unwrap();

    let (memory_result, memory_traces) = exercise(&memory);
    let (fjall_result, fjall_traces) = exercise(&fjall);
    let (native_result, native_traces) = exercise(&native);
    assert_eq!(memory_result, fjall_result);
    assert_eq!(memory_result, native_result);
    assert_eq!(memory_result.execution.known_at_cursor, 3);
    assert_eq!(memory_result.execution.scanned_changes, 3);
    assert_eq!(memory_result.execution.returned_rows, 1);
    assert_eq!(
        memory_result.execution.batches[0].rows[0].identity,
        "record:document:a"
    );
    assert_eq!(memory.runtime_cursor().unwrap(), 14);

    let normalize_logical = |traces: &[TraceView]| {
        traces
            .iter()
            .filter(|trace| trace.name != "vyrmkv.runtime_read")
            .map(|trace| {
                (
                    trace.name.clone(),
                    trace.phase.clone(),
                    trace.domain.clone(),
                    trace.outcome.clone(),
                    trace.trace_id.clone(),
                    trace.span_id.clone(),
                    trace.parent_span_id.clone(),
                    trace.attributes.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        normalize_logical(&memory_traces),
        normalize_logical(&fjall_traces)
    );
    assert_eq!(
        normalize_logical(&memory_traces),
        normalize_logical(&native_traces)
    );
    assert_eq!(
        memory_traces
            .iter()
            .map(|trace| (
                trace.name.as_str(),
                trace.phase.as_str(),
                trace.outcome.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            ("query.run", "start", "running"),
            ("vyrmql.parse_bind", "start", "running"),
            ("vyrmql.parse_bind", "finish", "ok"),
            ("vyrmmx.plan", "start", "running"),
            ("vyrmmx.plan", "finish", "ok"),
            ("vyrmmx.execute", "start", "running"),
            ("vyrmkv.runtime_read", "start", "running"),
            ("vyrmkv.runtime_read", "finish", "ok"),
            ("vyrmmx.execute", "finish", "ok"),
            ("query.run", "finish", "ok"),
        ]
    );
    let root_span = &memory_traces[0].span_id;
    assert!(memory_traces
        .iter()
        .filter(|trace| trace.name != "query.run" && trace.name != "vyrmkv.runtime_read")
        .all(|trace| trace.parent_span_id.as_ref() == Some(root_span)));
    let execution_span = &memory_traces[5].span_id;
    assert!(memory_traces
        .iter()
        .filter(|trace| trace.name == "vyrmkv.runtime_read")
        .all(|trace| trace.parent_span_id.as_ref() == Some(execution_span)));
    assert!(memory_traces
        .iter()
        .all(|trace| trace.trace_id == memory_traces[0].trace_id));
    assert!(memory_traces
        .iter()
        .all(|trace| !trace.encoded.contains("operator-secret")));
    assert_eq!(
        memory_traces[4].attributes["selected_paths"],
        RuntimeValue::List(vec![RuntimeValue::String("authoritative_log_scan".into())])
    );
    assert_eq!(
        memory_traces[8].attributes["returned_rows"],
        RuntimeValue::Unsigned(1)
    );
    assert_eq!(
        memory_traces[7].attributes["backend"],
        RuntimeValue::String("memory".into())
    );
    assert_eq!(
        fjall_traces[7].attributes["backend"],
        RuntimeValue::String("fjall_compatibility".into())
    );
    assert_eq!(
        native_traces[7].attributes["physical_evidence"],
        RuntimeValue::String("native_counters".into())
    );
    assert!(native_traces[7]
        .attributes
        .contains_key("block_bytes_loaded_delta"));
    assert!(native_traces[7]
        .attributes
        .contains_key("filter_checks_delta"));
    assert!(native_traces[7]
        .attributes
        .contains_key("filter_negatives_delta"));
    assert!(native_traces[7]
        .attributes
        .contains_key("compaction_debt_segments"));
    assert!(native_traces[7]
        .attributes
        .contains_key("compaction_target_segment_bytes"));
}

#[test]
fn parse_and_budget_failures_finish_the_active_tree_with_typed_evidence() {
    let parse_store = MemoryEngine::new();
    fixture(&parse_store);
    let error = execute_traced_query(
        &parse_store,
        scope(),
        "this is not vyrmQL and contains operator-secret",
        &Parameters::new(),
        &ExecutionBudget::default(),
        "operator:test",
        100,
    )
    .unwrap_err();
    assert!(error.to_string().contains("query parse error"));
    let parse_traces = trace_views(&parse_store);
    assert_eq!(parse_traces.len(), 4);
    assert_eq!(parse_traces[2].outcome, "error");
    assert_eq!(
        parse_traces[2].attributes["error_class"],
        RuntimeValue::String("parse".into())
    );
    assert_eq!(parse_traces[3].name, "query.run");
    assert!(parse_traces
        .iter()
        .all(|trace| !trace.encoded.contains("operator-secret")));

    let budget_store = MemoryEngine::new();
    fixture(&budget_store);
    let budget = ExecutionBudget {
        max_scanned_changes: 1,
        ..ExecutionBudget::default()
    };
    let error = execute_traced_query(
        &budget_store,
        scope(),
        "FROM record:document AT VALID 10 KNOWN HEAD PROJECT id",
        &Parameters::new(),
        &budget,
        "operator:test",
        100,
    )
    .unwrap_err();
    assert!(error.to_string().contains("budget allows 1"));
    let budget_traces = trace_views(&budget_store);
    assert_eq!(budget_traces.len(), 10);
    assert_eq!(budget_traces[7].outcome, "denied");
    assert_eq!(budget_traces[8].outcome, "denied");
    assert_eq!(budget_traces[9].outcome, "denied");
    assert_eq!(
        budget_traces[7].attributes["error_class"],
        RuntimeValue::String("budget".into())
    );
}

#[test]
fn json_parameters_are_scalar_bounded_and_unambiguous() {
    let parameters = query_parameters_from_json(&serde_json::json!({
        "null": null,
        "bool": true,
        "signed": -1,
        "unsigned": 1,
        "string": "one"
    }))
    .unwrap();
    assert_eq!(parameters["null"], RuntimeValue::Null);
    assert_eq!(parameters["bool"], RuntimeValue::Bool(true));
    assert_eq!(parameters["signed"], RuntimeValue::Integer(-1));
    assert_eq!(parameters["unsigned"], RuntimeValue::Unsigned(1));
    assert_eq!(parameters["string"], RuntimeValue::String("one".into()));
    assert!(query_parameters_from_json(&serde_json::json!({"nested": [1]})).is_err());
}
