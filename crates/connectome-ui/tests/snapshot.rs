use vyrm_core::{
    Claim, Evidence, Predicate, Producer, ReasoningPayload, RuntimeCommit, RuntimeEventSchema,
    RuntimeMutation, RuntimeProperties, RuntimeSchemaRegistry, RuntimeTraceEvent, RuntimeType,
    ScopeId, Subject, TraceDataClass, TraceDomain, TraceOutcome,
};
use vyrm_store::Engine;

#[test]
fn snapshot_exposes_runtime_objects_without_mutating_the_store() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("lib.rs"), "pub fn connectome() {}\n").unwrap();
    vyrm_node::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    let db = root.path().join(vyrm_node::STORE_DIR);
    let store = vyrm_store::PersistentEngine::open(&db).unwrap();
    Engine::assert(
        &store,
        &Claim::new(
            Subject::new("runtime").unwrap(),
            Predicate::new("status").unwrap(),
            "ready",
            1,
            1,
            Producer {
                actor: "test".into(),
                on_behalf_of: None,
                session: None,
            },
        ),
    )
    .unwrap();
    for (at, payload) in [
        (
            2,
            ReasoningPayload::Goal {
                statement: "inspect runtime".into(),
                acceptance: vec!["state visible".into()],
            },
        ),
        (
            3,
            ReasoningPayload::Plan {
                hypothesis: "snapshot exposes it".into(),
                steps: vec!["inspect".into()],
            },
        ),
        (
            4,
            ReasoningPayload::Attempt {
                summary: "open workbench".into(),
                actions: vec!["GET snapshot".into()],
            },
        ),
        (
            5,
            ReasoningPayload::Observation {
                summary: "snapshot loaded".into(),
                evidence: vec![Evidence {
                    source: "GET /api/snapshot".into(),
                    digest: "abc1230000000000000000000000000000000000000000000000000000000000"
                        .into(),
                    summary: "runtime objects returned".into(),
                }],
            },
        ),
    ] {
        vyrm_node::record_reasoning(&store, "ui-run", at, "test", payload).unwrap();
    }
    vyrm_node::ensure_routing_fresh(&store, root.path()).unwrap();
    let binding = vyrm_node::InstanceBinding::discover(root.path()).unwrap();
    let workflow = vyrm_node::WorkflowObservation {
        contract_version: vyrm_node::WORKFLOW_FORMAT,
        event: "package:bun:test".into(),
        manifest_digest: "a".repeat(64),
        command: "bun test".into(),
        arguments_supplied: 0,
        command_digest: "b".repeat(64),
        response_digest: "c".repeat(64),
        exit_code: Some(0),
        status: vyrm_node::WorkflowStatus::Passed,
        at: 9,
    };
    store
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new(binding.manifest.id.clone()).unwrap(),
            at: 9,
            actor: "hook:test".into(),
            expected_cursor: store.runtime_cursor().unwrap(),
            mutations: vec![
                RuntimeMutation::Schema {
                    registry: {
                        let mut registry = RuntimeSchemaRegistry::empty(
                            1,
                            "install package workflow evidence contract",
                        );
                        registry.events.insert(
                            RuntimeType::new("workflow-observation").unwrap(),
                            RuntimeEventSchema::default(),
                        );
                        registry
                    },
                },
                RuntimeMutation::Claim {
                    claim: Claim::new(
                        Subject::new("package:bun:test").unwrap(),
                        Predicate::new("status").unwrap(),
                        serde_json::to_string(&workflow).unwrap(),
                        9,
                        9,
                        Producer {
                            actor: "hook:test".into(),
                            on_behalf_of: None,
                            session: None,
                        },
                    ),
                },
            ],
        })
        .unwrap();
    let trace_identity = vyrm_node::TraceIdentity::derive(&[b"connectome-workflow-trace"]).unwrap();
    vyrm_node::record_runtime_trace(
        &store,
        &ScopeId::new(vyrm_node::REASONING_SCOPE).unwrap(),
        "hook:test",
        RuntimeTraceEvent::finish(
            trace_identity.trace_id,
            trace_identity.span_id,
            None,
            TraceDomain::Lifecycle,
            "lifecycle.pre-tool-use",
            10,
            250,
            TraceOutcome::Ok,
            TraceDataClass::Control,
            Vec::new(),
            RuntimeProperties::new(),
        )
        .unwrap(),
    )
    .unwrap();
    let plan_identity = vyrm_node::TraceIdentity::derive(&[b"connectome-planning-trace"]).unwrap();
    vyrm_node::record_runtime_trace(
        &store,
        &ScopeId::new(vyrm_node::REASONING_SCOPE).unwrap(),
        "query:test",
        RuntimeTraceEvent::finish(
            plan_identity.trace_id,
            plan_identity.span_id,
            None,
            TraceDomain::Planning,
            "vyrmmx.plan",
            10,
            125,
            TraceOutcome::Ok,
            TraceDataClass::Control,
            Vec::new(),
            RuntimeProperties::new(),
        )
        .unwrap(),
    )
    .unwrap();
    for (seed, actor, domain, name, duration_micros) in [
        (
            b"connectome-vector-trace".as_slice(),
            "vector:test",
            TraceDomain::Search,
            "vector.search",
            375,
        ),
        (
            b"connectome-embedding-trace".as_slice(),
            "embedding:test",
            TraceDomain::Embedding,
            "embedding.run",
            500,
        ),
        (
            b"connectome-storage-trace".as_slice(),
            "storage:test",
            TraceDomain::Storage,
            "vyrmkv.runtime_read",
            75,
        ),
        (
            b"connectome-operator-trace".as_slice(),
            "adapter:test",
            TraceDomain::Adapter,
            "operator.knowledge.search",
            625,
        ),
    ] {
        let identity = vyrm_node::TraceIdentity::derive(&[seed]).unwrap();
        vyrm_node::record_runtime_trace(
            &store,
            &ScopeId::new(vyrm_node::REASONING_SCOPE).unwrap(),
            actor,
            RuntimeTraceEvent::finish(
                identity.trace_id,
                identity.span_id,
                None,
                domain,
                name,
                10,
                duration_micros,
                TraceOutcome::Ok,
                TraceDataClass::Control,
                Vec::new(),
                RuntimeProperties::new(),
            )
            .unwrap(),
        )
        .unwrap();
    }
    let lease = store
        .open_runtime_snapshot(
            &vyrm_core::ScopeId::new("instance:default").unwrap(),
            "workbench:test",
            10,
            100,
        )
        .unwrap();
    let before = store.sequence().unwrap();
    let snapshot = connectome_ui::snapshot(&store, &binding, 10).unwrap();

    assert_eq!(snapshot.instance.mode, "dedicated");
    assert_eq!(snapshot.health.storage_backend, "vyrmkv_native");
    assert_eq!(snapshot.health.current_claims, 2);
    assert_eq!(snapshot.health.runtime_cursor, 18);
    assert_eq!(snapshot.health.schema_revision, Some(2));
    assert_eq!(snapshot.health.snapshot_leases, 1);
    assert_eq!(snapshot.health.retention_pins, 1);
    assert_eq!(snapshot.health.oldest_retained_cursor, Some(18));
    assert_eq!(snapshot.cluster.project_scope, binding.manifest.id);
    assert_eq!(snapshot.cluster.total_samples, 0);
    assert!(snapshot.cluster.samples.is_empty());
    let retention = connectome_ui::runtime_retention(&store, 10).unwrap();
    assert_eq!(retention.snapshots, vec![lease.clone()]);
    assert_eq!(retention.pins[0].snapshot_id, lease.id);
    let schema = snapshot
        .schema
        .as_ref()
        .expect("reasoning write installs schema");
    assert!(schema
        .records
        .keys()
        .any(|kind| kind.as_str() == "reasoning_run"));
    assert!(schema
        .events
        .keys()
        .any(|kind| kind.as_str() == "reasoning_event"));
    assert_eq!(snapshot.health.active_run.as_deref(), Some("ui-run"));
    assert_eq!(snapshot.files.len(), 1);
    assert!(snapshot.graph.nodes.iter().any(|node| node.kind == "claim"));
    assert!(snapshot.graph.nodes.iter().any(|node| node.kind == "run"));
    assert!(snapshot
        .graph
        .nodes
        .iter()
        .any(|node| node.kind == "evidence"));
    let workflow_event = snapshot
        .temporal_events
        .iter()
        .find(|event| event.family == "workflow")
        .expect("project-scoped workflow mutation is visible in the global stream");
    assert_eq!(workflow_event.scope, binding.manifest.id);
    assert_eq!(workflow_event.label, "package:bun:test");
    assert!(workflow_event.audit.is_some());
    let trace_event = snapshot
        .temporal_events
        .iter()
        .find(|event| event.action == "trace_finish")
        .expect("durable lifecycle trace is visible in the temporal stream");
    assert_eq!(trace_event.family, "workflow");
    assert_eq!(trace_event.label, "lifecycle.pre-tool-use");
    assert_eq!(trace_event.detail, "lifecycle; ok; 250 µs");
    let plan_trace = snapshot
        .temporal_events
        .iter()
        .find(|event| event.label == "vyrmmx.plan")
        .expect("durable planning trace is visible in the temporal stream");
    assert_eq!(plan_trace.family, "routing");
    assert_eq!(plan_trace.action, "trace_finish");
    assert_eq!(plan_trace.detail, "planning; ok; 125 µs");
    for (label, family, detail) in [
        ("vector.search", "search", "search; ok; 375 µs"),
        ("embedding.run", "search", "embedding; ok; 500 µs"),
        ("vyrmkv.runtime_read", "storage", "storage; ok; 75 µs"),
        ("operator.knowledge.search", "search", "adapter; ok; 625 µs"),
    ] {
        let event = snapshot
            .temporal_events
            .iter()
            .find(|event| event.label == label)
            .unwrap_or_else(|| panic!("{label} trace is visible in the temporal stream"));
        assert_eq!(event.family, family);
        assert_eq!(event.action, "trace_finish");
        assert_eq!(event.detail, detail);
        assert!(event.audit.is_some());
    }
    assert_eq!(
        store.sequence().unwrap(),
        before,
        "inspection must remain read-only"
    );

    let runtime = connectome_ui::runtime_graph(
        &store,
        vyrm_core::ScopeId::new("instance:default").unwrap(),
        10,
        None,
    )
    .unwrap();
    assert!(runtime
        .records
        .iter()
        .any(|record| record.reference.kind.as_str() == "reasoning_run"));
    assert_eq!(
        runtime
            .records
            .iter()
            .filter(|record| record.reference.kind.as_str() == "reasoning_event")
            .count(),
        4
    );
    assert_eq!(runtime.relations.len(), 4);

    let query = connectome_ui::runtime_query(
        &store,
        vyrm_core::ScopeId::new("instance:default").unwrap(),
        "FROM record:reasoning_run AT VALID 10 KNOWN HEAD PROJECT id EXPLAIN CONTRACT",
        &vyrm_mx::ExecutionBudget::default(),
    )
    .unwrap();
    assert_eq!(query.execution.returned_rows, 1);
    assert!(query.plan.explanation.contract.exact);
    assert_eq!(
        query.plan.explanation.candidates[0].name,
        "authoritative_log_scan"
    );
}

#[test]
fn default_path_resolution_is_instance_local_and_foreign_paths_fail() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    vyrm_node::InstanceManifest::ensure_dedicated(first.path()).unwrap();
    vyrm_node::InstanceManifest::ensure_dedicated(second.path()).unwrap();

    let (_, local) = connectome_ui::resolve_paths(first.path(), None).unwrap();
    assert_eq!(local, first.path().join(".vyrm/store"));
    let error =
        connectome_ui::resolve_paths(first.path(), Some(&second.path().join(".vyrm/store")))
            .unwrap_err()
            .to_string();
    assert!(error.contains("does not belong"));
    assert!(!second.path().join(".vyrm/store").exists());
}

#[test]
fn trace_export_is_causal_bounded_and_deny_by_default_for_content() {
    let root = tempfile::tempdir().unwrap();
    vyrm_node::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    let binding = vyrm_node::InstanceBinding::discover(root.path()).unwrap();
    let store = vyrm_store::PersistentEngine::open(&binding.expected_store()).unwrap();
    let scope = ScopeId::new(vyrm_node::REASONING_SCOPE).unwrap();
    let root_identity = vyrm_node::TraceIdentity::derive(&[b"trace-export-root"]).unwrap();
    let child_identity = root_identity.child(&[b"tool"]).unwrap();
    for event in [
        RuntimeTraceEvent::start(
            root_identity.trace_id.clone(),
            root_identity.span_id.clone(),
            None,
            TraceDomain::Model,
            "provider.invoke",
            100,
            TraceDataClass::Control,
            Vec::new(),
            RuntimeProperties::new(),
        )
        .unwrap(),
        RuntimeTraceEvent::start(
            child_identity.trace_id.clone(),
            child_identity.span_id.clone(),
            Some(root_identity.span_id.clone()),
            TraceDomain::Tool,
            "provider.tool.command_execution",
            101,
            TraceDataClass::Control,
            Vec::new(),
            RuntimeProperties::new(),
        )
        .unwrap(),
        RuntimeTraceEvent::finish(
            child_identity.trace_id,
            child_identity.span_id.clone(),
            Some(root_identity.span_id.clone()),
            TraceDomain::Tool,
            "provider.tool.command_execution",
            102,
            800,
            TraceOutcome::Ok,
            TraceDataClass::Control,
            Vec::new(),
            RuntimeProperties::new(),
        )
        .unwrap(),
        RuntimeTraceEvent::finish(
            root_identity.trace_id,
            root_identity.span_id.clone(),
            None,
            TraceDomain::Model,
            "provider.invoke",
            103,
            2_500,
            TraceOutcome::Ok,
            TraceDataClass::Control,
            Vec::new(),
            RuntimeProperties::new(),
        )
        .unwrap(),
    ] {
        vyrm_node::record_runtime_trace(&store, &scope, "connectome:test", event).unwrap();
    }
    let content_identity = vyrm_node::TraceIdentity::derive(&[b"trace-export-content"]).unwrap();
    vyrm_node::record_runtime_trace(
        &store,
        &scope,
        "connectome:test",
        RuntimeTraceEvent::finish(
            content_identity.trace_id,
            content_identity.span_id,
            None,
            TraceDomain::Model,
            "provider.content",
            104,
            1,
            TraceOutcome::Ok,
            TraceDataClass::Content,
            Vec::new(),
            RuntimeProperties::new(),
        )
        .unwrap(),
    )
    .unwrap();

    let before = store.runtime_cursor().unwrap();
    let control =
        connectome_ui::runtime_traces(&store, &binding.manifest.id, 4_096, &["control"]).unwrap();
    assert_eq!(control.format, "vyrm-trace-export-v1");
    assert_eq!(control.included_data_classes, vec!["control"]);
    assert_eq!(control.events.len(), 4);
    assert_eq!(control.traces.len(), 1);
    assert_eq!(control.traces[0].status, "complete");
    assert_eq!(control.traces[0].critical_path_duration_micros, Some(2_500));
    assert_eq!(
        control.traces[0].critical_path_span_ids,
        vec![
            root_identity.span_id.to_string(),
            child_identity.span_id.to_string()
        ]
    );
    assert!(control
        .events
        .iter()
        .all(|event| event.audit_digest.is_some()));

    let content =
        connectome_ui::runtime_traces(&store, &binding.manifest.id, 4_096, &["content"]).unwrap();
    assert_eq!(content.events.len(), 1);
    assert_eq!(content.traces[0].status, "summary");
    assert!(
        connectome_ui::runtime_traces(&store, &binding.manifest.id, 4_096, &["unknown"]).is_err()
    );
    assert_eq!(
        store.runtime_cursor().unwrap(),
        before,
        "trace analysis and export must remain read-only"
    );
}
