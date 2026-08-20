use vyrm_core::{DecisionKind, Reader, ReasoningPayload, ReasoningState};
use vyrm_node::{active_reasoning_run, handle, record_reasoning, HookContext, HookEvent};
use vyrm_store::MemoryEngine;

#[test]
fn one_attempt_authorizes_one_tool_and_post_tool_closes_it_with_evidence() {
    let root = tempfile::tempdir().unwrap();
    vyrm_node::InstanceManifest::ensure_dedicated(root.path()).unwrap();
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
    let edit = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": "lib.rs"},
        "tool_response": {"success": true}
    });
    assert!(handle(&ctx, HookEvent::PreToolUse, &edit).unwrap().stdout.is_empty());
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
    assert!(handle(&verify_ctx, HookEvent::PreToolUse, &verify).unwrap().stdout.is_empty());
    handle(&verify_ctx, HookEvent::PostToolUse, &verify).unwrap();
    assert_eq!(
        active_reasoning_run(&store).unwrap().unwrap().state(),
        ReasoningState::NeedsOutcome
    );
}
