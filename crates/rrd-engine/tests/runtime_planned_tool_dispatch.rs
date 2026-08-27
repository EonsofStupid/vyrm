#[path = "support/planned_lifecycle.rs"]
mod planned_lifecycle;

use planned_lifecycle::{seed_planned_lifecycle, PlannedLifecycleFixture};
use rrd_core::{digest, Reader, ReasoningPayload};
use rrd_engine::{
    activate_work_item, load_lifecycle_session, preflight, record_reasoning, record_work_item_plan,
    InstanceBinding, InstanceManifest, LifecycleEnforcementLevelV1, LifecyclePhaseV1, RrdEngine,
    ServiceError, WorkItemPlanRecord, REASONING_SCOPE,
};
use rrd_store::PersistentEngine;
use serde_json::json;

#[test]
fn engine_owned_runtime_mutation_crosses_one_planned_lifecycle_boundary() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    InstanceManifest::ensure_dedicated(root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn planned_dispatch() {}\n").unwrap();
    std::fs::write(
        root.join("rrflow.workplan.toml"),
        r#"
schema_version = 1
plan_id = "planned-dispatch"
title = "Planned dispatch"
authority = "RRD"
status_policy = "Evidence only"

[[gate]]
id = "G00"
title = "Control"
depends_on = []

[[item]]
id = "G00-W03"
gate = "G00"
title = "Bind runtime mutation"
depends_on = []
acceptance = ["engine dispatcher consumes one canonical authorization"]
"#,
    )
    .unwrap();

    let binding = InstanceBinding::discover(root).unwrap();
    let database = binding.expected_store();
    let store = PersistentEngine::open(&database).unwrap();
    let reader = Reader::new("test:planned-dispatch").unwrap();
    let flight = preflight(&store, root, Some("rrflow-mcp-dispatch"), &reader, 1, 1_500).unwrap();
    let receipt = flight.attunement.unwrap();
    activate_work_item(
        &store,
        "planned-dispatch",
        "G00-W03",
        2,
        "test",
        "activate-planned-dispatch",
    )
    .unwrap();
    record_work_item_plan(
        &store,
        "planned-dispatch",
        WorkItemPlanRecord {
            work_item_id: "G00-W03".into(),
            execution_mode: rrd_engine::WorkItemExecutionMode::Change,
            source_tree_sha256: receipt.source_tree_sha256.clone(),
            attunement_receipt_sha256: receipt.receipt_sha256.clone(),
            plan_payload_sha256: digest::sha256_hex(b"reviewed planned dispatch"),
            verification_commands: vec![vec!["cargo".into(), "test".into()]],
        },
        3,
        "test",
        "record-planned-dispatch",
    )
    .unwrap();
    for (at, payload) in [
        (
            4,
            ReasoningPayload::Goal {
                statement: "create one vector collection".into(),
                acceptance: vec!["one canonical authorization is consumed".into()],
            },
        ),
        (
            5,
            ReasoningPayload::Plan {
                hypothesis: "the engine-owned dispatcher enforces the active work item".into(),
                steps: vec!["authorize".into(), "execute".into(), "observe".into()],
            },
        ),
        (
            6,
            ReasoningPayload::Attempt {
                summary: "ensure the planned vector collection".into(),
                actions: vec!["rrflow_vector_collection_ensure".into()],
            },
        ),
    ] {
        record_reasoning(&store, "planned-tool-run", at, "test", payload).unwrap();
    }
    seed_planned_lifecycle(
        &store,
        &receipt,
        PlannedLifecycleFixture {
            project_id: &binding.manifest.id,
            session_id: "session-planned-tool-1",
            plan_id: "planned-dispatch",
            work_item_id: "G00-W03",
            reasoning_run_id: "planned-tool-run",
            actor: "hook:rrflow-mcp-dispatch",
            adapter_kind: "rrflow-mcp-dispatch",
            enforcement_level: LifecycleEnforcementLevelV1::Orchestrated,
            start_at: 10,
        },
    );
    drop(store);

    let engine = RrdEngine::open_bound(&binding).unwrap();
    let arguments = json!({
        "idempotency_key":"planned-vector-collection-1",
        "scope":format!("instance:{}", binding.manifest.id),
        "collection_id":"planned-documents",
        "vectors":[{
            "name":"title",
            "field":"title-embedding",
            "kind":"dense",
            "dimensions":2,
            "metric":"cosine",
            "memory_tier":"cached"
        }],
        "rrflow_lifecycle":{
            "session_id":"session-planned-tool-1",
            "tool_call_id":"tool-planned-vector-1"
        }
    });
    let result = engine
        .call_runtime_tool(root, "rrflow_vector_collection_ensure", &arguments, 30)
        .unwrap();
    assert!(result.text.contains("planned-documents"));

    for (offset, payload) in [
        json!({
            "kind":"decision",
            "decision":"continue",
            "rationale":"exercise the terminal tool-call identity"
        }),
        json!({
            "kind":"attempt",
            "summary":"retry the exact terminal tool call",
            "actions":["rrflow_vector_collection_ensure"]
        }),
    ]
    .into_iter()
    .enumerate()
    {
        let at = 31 + offset as u64;
        engine
            .call_runtime_tool(
                root,
                "rrflow_reasoning_record",
                &json!({
                    "run_id":"planned-tool-run",
                    "actor":"test",
                    "at":at,
                    "payload":payload
                }),
                at,
            )
            .unwrap();
    }
    let replay = engine.call_runtime_tool(root, "rrflow_vector_collection_ensure", &arguments, 34);
    assert!(
        matches!(replay, Err(ServiceError::Runtime(_))),
        "terminal tool-call replay unexpectedly executed: {replay:?}"
    );
    drop(engine);

    let reopened = PersistentEngine::open(&database).unwrap();
    let snapshot = load_lifecycle_session(
        &reopened,
        REASONING_SCOPE,
        &binding.manifest.id,
        "session-planned-tool-1",
    )
    .unwrap()
    .unwrap();
    assert_eq!(snapshot.phase, LifecyclePhaseV1::ToolCompleted);
    assert_eq!(snapshot.active_tool_call_id, None);
}
