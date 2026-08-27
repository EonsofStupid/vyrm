#[allow(dead_code, unused_imports)]
#[path = "runtime_lifecycle_contract/fixture.rs"]
mod fixture;

use fixture::*;

fn drive_to_recorded_plan<E: Engine>(engine: &E) {
    prepare_work_plan(engine);
    for command in canonical_commands().into_iter().take(14) {
        let at = command.occurred_at_unix_ms;
        append_lifecycle_event(engine, command, at).unwrap();
    }
}

fn context() -> LifecycleSupervisorContextV1 {
    LifecycleSupervisorContextV1 {
        scope: SCOPE.into(),
        project_id: PROJECT.into(),
        session_id: SESSION.into(),
        actor: "supervisor-test".into(),
        adapter_kind: "command_proxy".into(),
        adapter_version: "1.0.0".into(),
        enforcement_level: LifecycleEnforcementLevelV1::Proxied,
    }
}

fn request() -> LifecycleToolRequestV1 {
    LifecycleToolRequestV1 {
        tool_name: "RRFlowExec".into(),
        tool_request_sha256: sha('a'),
        tool_call_id: "tool-supervised-1".into(),
        mutation: true,
    }
}

#[test]
fn supervisor_authorizes_consumes_and_completes_one_exact_request() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);

    let authorization = authorize_lifecycle_tool(&engine, &context(), &request(), 15).unwrap();
    assert_eq!(
        load_lifecycle_session(&engine, SCOPE, PROJECT, SESSION)
            .unwrap()
            .unwrap()
            .phase,
        LifecyclePhaseV1::ToolAuthorized
    );
    consume_lifecycle_tool_authorization(&engine, &context(), &authorization, 16).unwrap();
    complete_lifecycle_tool(
        &engine,
        &context(),
        &authorization,
        &LifecycleToolCompletionV1 {
            observation_sha256: sha('b'),
            success: true,
            project_state_changed: false,
        },
        17,
    )
    .unwrap();
    assert_eq!(
        load_lifecycle_session(&engine, SCOPE, PROJECT, SESSION)
            .unwrap()
            .unwrap()
            .phase,
        LifecyclePhaseV1::ToolCompleted
    );
}

#[test]
fn consumed_supervisor_authorization_cannot_start_twice() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    let authorization = authorize_lifecycle_tool(&engine, &context(), &request(), 15).unwrap();
    consume_lifecycle_tool_authorization(&engine, &context(), &authorization, 16).unwrap();
    let before = engine.runtime_cursor().unwrap();
    let error = consume_lifecycle_tool_authorization(&engine, &context(), &authorization, 17)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("already consumed"),
        "unexpected denial: {error}"
    );
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn supervisor_rejects_substituted_authorization_and_result() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    let authorization = authorize_lifecycle_tool(&engine, &context(), &request(), 15).unwrap();
    let mut substituted = authorization.clone();
    substituted.decision_sha256 = sha('f');
    assert!(consume_lifecycle_tool_authorization(&engine, &context(), &substituted, 16).is_err());
    consume_lifecycle_tool_authorization(&engine, &context(), &authorization, 16).unwrap();
    let mut substituted = authorization.clone();
    substituted.tool_request_sha256 = sha('e');
    assert!(complete_lifecycle_tool(
        &engine,
        &context(),
        &substituted,
        &LifecycleToolCompletionV1 {
            observation_sha256: sha('b'),
            success: true,
            project_state_changed: false,
        },
        17,
    )
    .is_err());
}

#[test]
fn supervisor_recovers_the_same_proposal_but_denies_a_competing_request() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    let proposal = canonical_commands()[14].clone();
    append_lifecycle_event(&engine, proposal, 15).unwrap();

    let canonical_request = LifecycleToolRequestV1 {
        tool_name: "apply_patch".into(),
        tool_request_sha256: sha('a'),
        tool_call_id: "tool-1".into(),
        mutation: true,
    };
    authorize_lifecycle_tool(&engine, &context(), &canonical_request, 16).unwrap();

    let mut competing = canonical_request;
    competing.tool_call_id = "tool-competing".into();
    assert!(authorize_lifecycle_tool(&engine, &context(), &competing, 17).is_err());
}

#[test]
fn supervisor_reopen_preserves_one_shot_authorization() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("supervisor-reopen");
    let engine = NativeEngine::open(&path).unwrap();
    drive_to_recorded_plan(&engine);
    let authorization = authorize_lifecycle_tool(&engine, &context(), &request(), 15).unwrap();
    drop(engine);

    let reopened = NativeEngine::open(&path).unwrap();
    consume_lifecycle_tool_authorization(&reopened, &context(), &authorization, 16).unwrap();
    drop(reopened);

    let reopened = NativeEngine::open(&path).unwrap();
    assert!(
        consume_lifecycle_tool_authorization(&reopened, &context(), &authorization, 17).is_err()
    );
}

#[test]
fn changed_authoritative_plan_invalidates_the_lifecycle_permit() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    record_work_item_plan(
        &engine,
        "rrflow-foundation",
        WorkItemPlanRecord {
            work_item_id: "G00-W02".into(),
            source_tree_sha256: sha('4'),
            attunement_receipt_sha256: sha('6'),
            plan_payload_sha256: sha('f'),
            verification_commands: vec![vec!["cargo".into(), "test".into()]],
        },
        15,
        "test",
        "replace-plan",
    )
    .unwrap();
    let before = engine.runtime_cursor().unwrap();
    let error = authorize_lifecycle_tool(&engine, &context(), &request(), 16)
        .unwrap_err()
        .to_string();
    assert!(error.contains("stale"), "unexpected denial: {error}");
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn plan_change_after_proposal_is_rechecked_before_authorization() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    append_lifecycle_event(&engine, canonical_commands()[14].clone(), 15).unwrap();
    record_work_item_plan(
        &engine,
        "rrflow-foundation",
        WorkItemPlanRecord {
            work_item_id: "G00-W02".into(),
            source_tree_sha256: sha('4'),
            attunement_receipt_sha256: sha('6'),
            plan_payload_sha256: sha('f'),
            verification_commands: vec![vec!["cargo".into(), "test".into()]],
        },
        16,
        "test",
        "replace-proposed-plan",
    )
    .unwrap();
    let canonical_request = LifecycleToolRequestV1 {
        tool_name: "apply_patch".into(),
        tool_request_sha256: sha('a'),
        tool_call_id: "tool-1".into(),
        mutation: true,
    };
    let before = engine.runtime_cursor().unwrap();
    assert!(authorize_lifecycle_tool(&engine, &context(), &canonical_request, 17).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn permit_expiry_is_rechecked_at_authorization_and_consumption() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    let mut proposal = canonical_commands()[14].clone();
    proposal.occurred_at_unix_ms = 9_999;
    append_lifecycle_event(&engine, proposal, 9_999).unwrap();
    let canonical_request = LifecycleToolRequestV1 {
        tool_name: "apply_patch".into(),
        tool_request_sha256: sha('a'),
        tool_call_id: "tool-1".into(),
        mutation: true,
    };
    assert!(authorize_lifecycle_tool(&engine, &context(), &canonical_request, 10_000).is_err());

    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    let authorization = authorize_lifecycle_tool(&engine, &context(), &request(), 9_999).unwrap();
    let before = engine.runtime_cursor().unwrap();
    assert!(
        consume_lifecycle_tool_authorization(&engine, &context(), &authorization, 10_000).is_err()
    );
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn terminal_completion_replays_exactly_and_tool_call_identity_cannot_execute_again() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    let authorization = authorize_lifecycle_tool(&engine, &context(), &request(), 15).unwrap();
    consume_lifecycle_tool_authorization(&engine, &context(), &authorization, 16).unwrap();
    let completion = LifecycleToolCompletionV1 {
        observation_sha256: sha('b'),
        success: true,
        project_state_changed: false,
    };
    complete_lifecycle_tool(&engine, &context(), &authorization, &completion, 17).unwrap();
    let before = engine.runtime_cursor().unwrap();
    complete_lifecycle_tool(&engine, &context(), &authorization, &completion, 18).unwrap();
    assert_eq!(engine.runtime_cursor().unwrap(), before);
    assert!(authorize_lifecycle_tool(&engine, &context(), &request(), 18).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), before);

    let mut substituted = completion;
    substituted.observation_sha256 = sha('c');
    assert!(
        complete_lifecycle_tool(&engine, &context(), &authorization, &substituted, 18).is_err()
    );
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}
