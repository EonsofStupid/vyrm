use vyrm_core::{
    RuntimeCommit, RuntimeMutation, RuntimeProperties, RuntimeSchemaRegistry, RuntimeTraceEvent,
    RuntimeType, RuntimeValue, ScopeId, SpanId, TraceDataClass, TraceDomain, TraceId, TraceLink,
    TraceOutcome,
};
use vyrm_store::{Engine, MemoryEngine, NativeEngine, Store};

fn trace_id() -> TraceId {
    TraceId::new("0123456789abcdef0123456789abcdef").unwrap()
}

fn span_id() -> SpanId {
    SpanId::new("0123456789abcdef").unwrap()
}

fn exercise(engine: &dyn Engine) -> Vec<Vec<u8>> {
    let scope = ScopeId::new("instance:trace-differential").unwrap();
    let mut schema = RuntimeSchemaRegistry::empty(1, "install persisted runtime tracing");
    schema.events.insert(
        RuntimeType::new("runtime_trace").unwrap(),
        RuntimeTraceEvent::event_schema(),
    );

    let start = RuntimeTraceEvent::start(
        trace_id(),
        span_id(),
        None,
        TraceDomain::Search,
        "operator.pgvector.search",
        1_000,
        TraceDataClass::Operator,
        vec![TraceLink::OperatorKnowledge {
            adapter: "pgvector".into(),
            project_id: "trace-differential".into(),
            source_revision: "postgres-lsn:0/16B6C50".into(),
        }],
        RuntimeProperties::from([("requested_limit".into(), RuntimeValue::Unsigned(10))]),
    )
    .unwrap();
    engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 1_000,
            actor: "adapter:pgvector".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema { registry: schema },
                RuntimeMutation::Event {
                    event: start.into_runtime_event().unwrap(),
                },
            ],
        })
        .unwrap();

    let finish = RuntimeTraceEvent::finish(
        trace_id(),
        span_id(),
        None,
        TraceDomain::Search,
        "operator.pgvector.search",
        1_001,
        840,
        TraceOutcome::Ok,
        TraceDataClass::Operator,
        vec![TraceLink::OperatorKnowledge {
            adapter: "pgvector".into(),
            project_id: "trace-differential".into(),
            source_revision: "postgres-lsn:0/16B6C50".into(),
        }],
        RuntimeProperties::from([("result_count".into(), RuntimeValue::Unsigned(7))]),
    )
    .unwrap();
    engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 1_001,
            actor: "adapter:pgvector".into(),
            expected_cursor: 2,
            mutations: vec![RuntimeMutation::Event {
                event: finish.into_runtime_event().unwrap(),
            }],
        })
        .unwrap();

    let page = engine.runtime_changes_since(0, 10, Some(&scope)).unwrap();
    assert_eq!(page.head_cursor, 3);
    assert_eq!(page.changes.len(), 3);
    assert!(page.changes.iter().all(|change| change.verify_digest()));
    page.changes
        .into_iter()
        .map(|change| serde_json::to_vec(&change.mutation).unwrap())
        .collect()
}

#[test]
fn persisted_trace_events_are_identical_across_reference_compatibility_and_native_engines() {
    let memory = MemoryEngine::new();
    let fjall_root = tempfile::tempdir().unwrap();
    let fjall = Store::open(fjall_root.path()).unwrap();
    let native_root = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&native_root.path().join("native")).unwrap();

    let expected = exercise(&memory);
    assert_eq!(exercise(&fjall), expected);
    assert_eq!(exercise(&native), expected);
}
