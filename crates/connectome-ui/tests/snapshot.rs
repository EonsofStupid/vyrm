use vyrm_core::{Claim, Evidence, Predicate, Producer, ReasoningPayload, Subject};
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
    assert_eq!(snapshot.health.current_claims, 1);
    assert_eq!(snapshot.health.runtime_cursor, 9);
    assert_eq!(snapshot.health.schema_revision, Some(1));
    assert_eq!(snapshot.health.snapshot_leases, 1);
    assert_eq!(snapshot.health.retention_pins, 1);
    assert_eq!(snapshot.health.oldest_retained_cursor, Some(9));
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
