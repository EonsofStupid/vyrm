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
fn cooperative_and_observe_only_adapters_fail_closed_for_mutations() {
    for level in [
        LifecycleEnforcementLevelV1::Cooperative,
        LifecycleEnforcementLevelV1::ObserveOnly,
    ] {
        let engine = MemoryEngine::new();
        drive_to_recorded_plan(&engine);
        let denied_context = LifecycleSupervisorContextV1 {
            enforcement_level: level,
            ..context()
        };
        let error = authorize_planned_lifecycle_tool(
            &engine,
            &denied_context,
            &request(),
            &sha('4'),
            &sha('6'),
            15,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("is not an enforcement boundary"),
            "unexpected denial for {level:?}: {error}"
        );
        assert_eq!(
            load_lifecycle_session(&engine, SCOPE, PROJECT, SESSION)
                .unwrap()
                .unwrap()
                .phase,
            LifecyclePhaseV1::PlanRecorded,
        );
    }
}

#[test]
fn planned_supervisor_advances_both_authorities_and_replays_completion() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    let authorization =
        authorize_planned_lifecycle_tool(&engine, &context(), &request(), &sha('4'), &sha('6'), 15)
            .unwrap();
    consume_lifecycle_tool_authorization(&engine, &context(), &authorization, 16).unwrap();
    let completion = LifecycleToolCompletionV1 {
        observation_sha256: sha('b'),
        success: true,
        project_state_changed: false,
    };
    complete_planned_lifecycle_tool(
        &engine,
        &context(),
        &authorization,
        &completion,
        Some(&sha('4')),
        17,
    )
    .unwrap();
    let runtime_before = engine.runtime_cursor().unwrap();
    let control_before = engine.control_sequence().unwrap();
    complete_planned_lifecycle_tool(
        &engine,
        &context(),
        &authorization,
        &completion,
        Some(&sha('4')),
        18,
    )
    .unwrap();
    assert_eq!(engine.runtime_cursor().unwrap(), runtime_before);
    assert_eq!(engine.control_sequence().unwrap(), control_before);
}

#[test]
fn changed_project_state_requires_and_accepts_explicit_projection_refresh() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    let authorization =
        authorize_planned_lifecycle_tool(&engine, &context(), &request(), &sha('4'), &sha('6'), 15)
            .unwrap();
    consume_lifecycle_tool_authorization(&engine, &context(), &authorization, 16).unwrap();
    complete_planned_lifecycle_tool(
        &engine,
        &context(),
        &authorization,
        &LifecycleToolCompletionV1 {
            observation_sha256: sha('b'),
            success: true,
            project_state_changed: true,
        },
        Some(&sha('c')),
        17,
    )
    .unwrap();
    assert!(authorize_lifecycle_tool(&engine, &context(), &request(), 18).is_err());
    let refresh = LifecycleProjectionRefreshV1 {
        before_tree_sha256: sha('4'),
        after_tree_sha256: sha('c'),
        changed_paths_sha256: sha('d'),
        projection_sha256: sha('e'),
        evidence_sha256: sha('f'),
    };
    refresh_lifecycle_projection(&engine, &context(), &refresh, 18).unwrap();
    assert_eq!(
        load_lifecycle_session(&engine, SCOPE, PROJECT, SESSION)
            .unwrap()
            .unwrap()
            .phase,
        LifecyclePhaseV1::ProjectionReady
    );
    let cursor = engine.runtime_cursor().unwrap();
    refresh_lifecycle_projection(&engine, &context(), &refresh, 19).unwrap();
    assert_eq!(engine.runtime_cursor().unwrap(), cursor);
}

#[test]
fn planned_supervisor_leaves_a_failed_materialization_non_executable() {
    let engine = MemoryEngine::new();
    drive_to_recorded_plan(&engine);
    let error =
        authorize_planned_lifecycle_tool(&engine, &context(), &request(), &sha('4'), &sha('f'), 15)
            .unwrap_err()
            .to_string();
    assert!(error.contains("stale"), "unexpected denial: {error}");
    assert_eq!(
        load_lifecycle_session(&engine, SCOPE, PROJECT, SESSION)
            .unwrap()
            .unwrap()
            .phase,
        LifecyclePhaseV1::ToolProposed
    );
    let events = load_lifecycle_events(&engine, SCOPE, PROJECT, SESSION).unwrap();
    assert!(!events
        .iter()
        .any(|event| event.event_type == LifecycleEventTypeV1::ToolStarted));
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
            execution_mode: rrd_engine::WorkItemExecutionMode::Change,
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
            execution_mode: rrd_engine::WorkItemExecutionMode::Change,
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
    assert_eq!(
        load_active_lifecycle_tool_authorization(&engine, &context(), &request()).unwrap(),
        authorization,
    );
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
