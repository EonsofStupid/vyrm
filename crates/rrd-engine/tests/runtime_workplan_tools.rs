use rrd_engine::{InstanceBinding, InstanceManifest, RrdEngine, ServiceError, WorkPlanOperation};
use serde_json::{json, Value};

fn fixture() -> (tempfile::TempDir, InstanceBinding) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    InstanceManifest::ensure_dedicated_as(root, "workplan-surface").unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn workplan_surface() {}\n").unwrap();
    std::fs::write(
        root.join("rrflow.workplan.toml"),
        r#"
schema_version = 1
plan_id = "surface-plan"
title = "Surface plan"
authority = "RRD"
status_policy = "Evidence only"

[[gate]]
id = "G00"
title = "Control"
depends_on = []

[[item]]
id = "G00-W05"
gate = "G00"
title = "Generated work-plan surfaces"
depends_on = []
acceptance = ["all surfaces derive from one engine catalogue"]
"#,
    )
    .unwrap();
    let binding = InstanceBinding::discover(root).unwrap();
    (temporary, binding)
}

fn call(
    engine: &RrdEngine,
    root: &std::path::Path,
    operation: WorkPlanOperation,
    args: Value,
    at: u64,
) -> Value {
    let result = engine
        .call_runtime_tool(root, operation.runtime_tool_name(), &args, at)
        .unwrap();
    serde_json::from_str(&result.text).unwrap()
}

fn prepare_qualification(
    engine: &RrdEngine,
    root: &std::path::Path,
    verification_command: Vec<String>,
) {
    call(engine, root, WorkPlanOperation::Sync, json!({"at":10}), 10);
    call(
        engine,
        root,
        WorkPlanOperation::Activate,
        json!({"plan_id":"surface-plan","item_id":"G00-W05","at":11}),
        11,
    );
    engine
        .call_runtime_tool(root, "rrflow_preflight", &json!({"at":12}), 12)
        .unwrap();
    call(
        engine,
        root,
        WorkPlanOperation::Record,
        json!({
            "plan_id":"surface-plan",
            "item_id":"G00-W05",
            "plan_payload":"reviewed generated-surface plan",
            "qualification_only":true,
            "verification_commands":[verification_command],
            "at":13
        }),
        13,
    );
}

#[test]
fn all_generated_operations_execute_and_verified_state_survives_reopen() {
    let (temporary, binding) = fixture();
    let root = temporary.path();
    let engine = RrdEngine::open_bound(&binding).unwrap();
    let synced = call(&engine, root, WorkPlanOperation::Sync, json!({"at":1}), 1);
    assert_eq!(synced["plan_id"], "surface-plan");
    let status = call(
        &engine,
        root,
        WorkPlanOperation::Status,
        json!({"plan_id":"surface-plan"}),
        2,
    );
    assert_eq!(status["items"][0]["status"], "pending");
    call(
        &engine,
        root,
        WorkPlanOperation::Activate,
        json!({"plan_id":"surface-plan","item_id":"G00-W05","at":3}),
        3,
    );
    engine
        .call_runtime_tool(root, "rrflow_preflight", &json!({"at":4}), 4)
        .unwrap();
    call(
        &engine,
        root,
        WorkPlanOperation::Record,
        json!({
            "plan_id":"surface-plan",
            "item_id":"G00-W05",
            "plan_payload":"reviewed generated-surface plan",
            "qualification_only":true,
            "verification_commands":[[
                std::env::current_exe().unwrap().to_string_lossy(),
                "--list"
            ]],
            "at":5
        }),
        5,
    );
    let verified = call(
        &engine,
        root,
        WorkPlanOperation::Verify,
        json!({"plan_id":"surface-plan","at":6}),
        6,
    );
    assert_eq!(verified["items"][0]["status"], "verified");
    assert!(verified["items"][0]["verification_sha256"].is_string());
    drop(engine);

    let reopened = RrdEngine::open_bound(&binding).unwrap();
    let replayed = call(
        &reopened,
        root,
        WorkPlanOperation::Status,
        json!({"plan_id":"surface-plan"}),
        7,
    );
    assert_eq!(replayed["items"][0]["status"], "verified");
    assert_eq!(replayed["active_item_id"], Value::Null);
}

#[test]
fn failed_exact_command_cannot_verify_and_remains_active_after_reopen() {
    let (temporary, binding) = fixture();
    let root = temporary.path();
    let engine = RrdEngine::open_bound(&binding).unwrap();
    prepare_qualification(
        &engine,
        root,
        vec![
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            "--rrflow-invalid-verification-option".into(),
        ],
    );
    let result = engine.call_runtime_tool(
        root,
        WorkPlanOperation::Verify.runtime_tool_name(),
        &json!({"plan_id":"surface-plan","at":14}),
        14,
    );
    assert!(matches!(result, Err(ServiceError::Runtime(_))));
    drop(engine);

    let reopened = RrdEngine::open_bound(&binding).unwrap();
    let status = call(
        &reopened,
        root,
        WorkPlanOperation::Status,
        json!({"plan_id":"surface-plan"}),
        15,
    );
    assert_eq!(status["items"][0]["status"], "active");
    assert_eq!(status["active_item_id"], "G00-W05");
}

#[test]
fn stale_project_tree_cannot_cross_the_verified_transition() {
    let (temporary, binding) = fixture();
    let root = temporary.path();
    let engine = RrdEngine::open_bound(&binding).unwrap();
    prepare_qualification(
        &engine,
        root,
        vec![
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            "--list".into(),
        ],
    );
    std::fs::write(
        root.join("changed.rs"),
        "pub fn changed_after_record() {}\n",
    )
    .unwrap();
    let result = engine.call_runtime_tool(
        root,
        WorkPlanOperation::Verify.runtime_tool_name(),
        &json!({"plan_id":"surface-plan","at":14}),
        14,
    );
    assert!(matches!(result, Err(ServiceError::Runtime(_))));
    let status = call(
        &engine,
        root,
        WorkPlanOperation::Status,
        json!({"plan_id":"surface-plan"}),
        15,
    );
    assert_eq!(status["items"][0]["status"], "active");
}
