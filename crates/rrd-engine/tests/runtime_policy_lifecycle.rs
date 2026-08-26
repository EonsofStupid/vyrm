use rrd_core::{digest, DecisionKind, Reader, ReasoningPayload, ReasoningState};
use rrd_engine::{
    activate_work_item, active_reasoning_run, handle, preflight, record_reasoning,
    record_work_item_plan, HookContext, HookEvent, WorkItemPlanRecord,
};
use rrd_store::MemoryEngine;

#[test]
fn one_attempt_authorizes_one_tool_and_post_tool_closes_it_with_evidence() {
    let root = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    std::fs::write(root.path().join("lib.rs"), "pub fn item() {}\n").unwrap();
    let store = MemoryEngine::new();
    for (at, payload) in [
        (
            1,
            ReasoningPayload::Goal {
                statement: "change item".into(),
                acceptance: vec!["tests pass".into()],
            },
        ),
        (
            2,
            ReasoningPayload::Plan {
                hypothesis: "one edit fixes it".into(),
                steps: vec!["edit".into(), "test".into()],
            },
        ),
        (
            3,
            ReasoningPayload::Attempt {
                summary: "edit item".into(),
                actions: vec!["Edit lib.rs".into()],
            },
        ),
    ] {
        record_reasoning(&store, "policy-run", at, "agent", payload).unwrap();
    }
    let reader = Reader::new("test:policy").unwrap();
    let ctx = HookContext {
        store: &store,
        root: root.path(),
        harness: Some("test"),
        reader: &reader,
        now: 4,
        budget: 1_500,
    };
    let flight = preflight(&store, root.path(), Some("test"), &reader, 4, 1_500).unwrap();
    assert!(flight.attunement.is_some());
    let edit = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": "lib.rs"},
        "tool_response": {"success": true}
    });
    assert!(handle(&ctx, HookEvent::PreToolUse, &edit)
        .unwrap()
        .stdout
        .is_empty());
    let competing = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": "other.rs"}
    });
    let denied = handle(&ctx, HookEvent::PreToolUse, &competing).unwrap();
    assert!(denied.stdout.contains("permissionDecision"));
    assert!(denied.stdout.contains("another unobserved tool request"));
    let post = handle(&ctx, HookEvent::PostToolUse, &edit).unwrap();
    assert!(post.detail.unwrap().contains("observation #4"));
    assert_eq!(
        active_reasoning_run(&store).unwrap().unwrap().state(),
        ReasoningState::NeedsDecision
    );
    let denied = handle(&ctx, HookEvent::PreToolUse, &edit).unwrap();
    assert!(denied.stdout.contains("permissionDecision"));
    assert!(denied.stdout.contains("NeedsDecision"));

    record_reasoning(
        &store,
        "policy-run",
        5,
        "agent",
        ReasoningPayload::Decision {
            decision: DecisionKind::Verify,
            rationale: "edit result is ready for tests".into(),
        },
    )
    .unwrap();
    let verify = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "cargo test"},
        "tool_response": {"exitCode": 0}
    });
    let verify_ctx = HookContext { now: 6, ..ctx };
    assert!(handle(&verify_ctx, HookEvent::PreToolUse, &verify)
        .unwrap()
        .stdout
        .is_empty());
    handle(&verify_ctx, HookEvent::PostToolUse, &verify).unwrap();
    assert_eq!(
        active_reasoning_run(&store).unwrap().unwrap().state(),
        ReasoningState::NeedsOutcome
    );
}

#[test]
fn checked_in_work_plan_denies_unscoped_mutation_and_allows_bound_work() {
    let root = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    std::fs::write(root.path().join("lib.rs"), "pub fn item() {}\n").unwrap();
    std::fs::write(
        root.path().join("rrflow.workplan.toml"),
        r#"
schema_version = 1
plan_id = "foundation"
title = "Foundation"
authority = "RRD"
status_policy = "Evidence only"

[[gate]]
id = "G00"
title = "Control"
depends_on = []

[[item]]
id = "G00-W01"
gate = "G00"
title = "Enforcement"
depends_on = []
acceptance = ["hook denies unscoped mutation"]
"#,
    )
    .unwrap();
    let store = MemoryEngine::new();
    for (at, payload) in [
        (
            1,
            ReasoningPayload::Goal {
                statement: "change item".into(),
                acceptance: vec!["tests pass".into()],
            },
        ),
        (
            2,
            ReasoningPayload::Plan {
                hypothesis: "one edit fixes it".into(),
                steps: vec!["edit".into()],
            },
        ),
        (
            3,
            ReasoningPayload::Attempt {
                summary: "edit item".into(),
                actions: vec!["Edit lib.rs".into()],
            },
        ),
    ] {
        record_reasoning(&store, "workplan-run", at, "agent", payload).unwrap();
    }
    let reader = Reader::new("test:workplan-policy").unwrap();
    let flight = preflight(&store, root.path(), None, &reader, 4, 1_500).unwrap();
    let receipt = flight.attunement.unwrap();
    assert_eq!(
        flight.work_plan.unwrap().plan_id,
        "foundation",
        "preflight must install the checked-in plan"
    );
    let edit = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": "lib.rs"},
        "tool_response": {"success": true}
    });
    let ctx = HookContext {
        store: &store,
        root: root.path(),
        harness: Some("test"),
        reader: &reader,
        now: 5,
        budget: 1_500,
    };
    let denied = handle(&ctx, HookEvent::PreToolUse, &edit).unwrap();
    assert!(denied.stdout.contains("no active item"));

    activate_work_item(&store, "foundation", "G00-W01", 6, "agent", "activate-1").unwrap();
    record_work_item_plan(
        &store,
        "foundation",
        WorkItemPlanRecord {
            work_item_id: "G00-W01".into(),
            source_tree_sha256: receipt.source_tree_sha256,
            attunement_receipt_sha256: receipt.receipt_sha256,
            plan_payload_sha256: digest::sha256_hex(b"reviewed plan"),
            verification_commands: vec![vec!["cargo".into(), "test".into()]],
        },
        7,
        "agent",
        "plan-1",
    )
    .unwrap();
    let allowed = handle(&HookContext { now: 8, ..ctx }, HookEvent::PreToolUse, &edit).unwrap();
    assert!(allowed.stdout.is_empty());
    assert!(allowed
        .detail
        .unwrap()
        .contains("enforced work item G00-W01"));
}
