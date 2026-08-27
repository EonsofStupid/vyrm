pub(super) use rrd_contract::{
    LifecycleEnforcementLevelV1, LifecycleEventCommandV1, LifecycleEventEnvelopeV1,
    LifecycleEventTypeV1, LifecyclePayloadV1, LifecyclePhaseV1, LifecycleRiskV1,
    LifecycleTaskKindV1, LifecycleTraceContextV1, LifecycleTurnStatusV1, WorkPlanDefinition,
    LIFECYCLE_SPEC_VERSION,
};
pub(super) use rrd_core::{
    digest, RuntimeCommit, RuntimeEvent, RuntimeMutation, RuntimeProperties, RuntimeRef,
    RuntimeType, RuntimeValue, ScopeId,
};
pub(super) use rrd_engine::runtime::{
    activate_work_item, append_lifecycle_event, authorize_lifecycle_tool, complete_lifecycle_tool,
    consume_lifecycle_tool_authorization, install_work_plan, load_lifecycle_events,
    load_lifecycle_session, record_work_item_plan, LifecycleSessionSnapshotV1,
    LifecycleSupervisorContextV1, LifecycleToolAuthorizationV1, LifecycleToolCompletionV1,
    LifecycleToolRequestV1, WorkItemPlanRecord, LIFECYCLE_RUNTIME_EVENT_TYPE,
    LIFECYCLE_RUNTIME_SESSION_TYPE,
};
pub(super) use rrd_store::{Engine, MemoryEngine, NativeEngine, Store};

pub(super) const SCOPE: &str = "project:foundation";
pub(super) const PROJECT: &str = "foundation";
pub(super) const SESSION: &str = "session-1";
const TURN: &str = "turn-1";
const REASONING: &str = "reasoning-1";
const ATTEMPT: &str = "attempt-1";
const TOOL: &str = "tool-1";

pub(super) fn sha(byte: char) -> String {
    byte.to_string().repeat(64)
}

fn verification_commands() -> Vec<Vec<String>> {
    vec![vec!["cargo".into(), "test".into()]]
}

fn verification_plan_sha256() -> String {
    digest::sha256_hex(&serde_json::to_vec(&verification_commands()).unwrap())
}

pub(super) fn work_plan_definition() -> WorkPlanDefinition {
    toml::from_str(
        r#"
schema_version = 1
plan_id = "rrflow-foundation"
title = "Lifecycle contract fixture"
authority = "RRD"
status_policy = "Evidence only"

[[gate]]
id = "G00"
title = "Control"
depends_on = []

[[item]]
id = "G00-W02"
gate = "G00"
title = "Lifecycle"
depends_on = []
acceptance = ["canonical lifecycle binds active work"]
"#,
    )
    .unwrap()
}

pub(super) fn prepare_work_plan<E: Engine>(engine: &E) {
    install_work_plan(engine, work_plan_definition(), 1, "test", "install-plan").unwrap();
    activate_work_item(
        engine,
        "rrflow-foundation",
        "G00-W02",
        2,
        "test",
        "activate-plan",
    )
    .unwrap();
    record_work_item_plan(
        engine,
        "rrflow-foundation",
        WorkItemPlanRecord {
            work_item_id: "G00-W02".into(),
            source_tree_sha256: sha('4'),
            attunement_receipt_sha256: sha('6'),
            plan_payload_sha256: sha('e'),
            verification_commands: verification_commands(),
        },
        3,
        "test",
        "record-plan",
    )
    .unwrap();
}

fn is_session_event(event_type: LifecycleEventTypeV1) -> bool {
    matches!(
        event_type,
        LifecycleEventTypeV1::SessionOpened
            | LifecycleEventTypeV1::ProjectAttunementRequested
            | LifecycleEventTypeV1::ProjectAttunementCompleted
            | LifecycleEventTypeV1::ProjectAttunementFailed
            | LifecycleEventTypeV1::SessionCompactionStarted
            | LifecycleEventTypeV1::SessionCompactionCompleted
            | LifecycleEventTypeV1::SessionClosed
    )
}

fn is_reasoning_event(event_type: LifecycleEventTypeV1) -> bool {
    matches!(
        event_type,
        LifecycleEventTypeV1::PlanRecorded
            | LifecycleEventTypeV1::ToolProposed
            | LifecycleEventTypeV1::ToolAuthorized
            | LifecycleEventTypeV1::ToolDenied
            | LifecycleEventTypeV1::ToolStarted
            | LifecycleEventTypeV1::ToolCompleted
            | LifecycleEventTypeV1::ToolFailed
            | LifecycleEventTypeV1::ProjectStateInvalidated
            | LifecycleEventTypeV1::ProjectionRefreshCompleted
            | LifecycleEventTypeV1::ProjectionRefreshFailed
            | LifecycleEventTypeV1::VerificationCompleted
            | LifecycleEventTypeV1::ReasoningOutcomeRecorded
    )
}

fn is_tool_event(event_type: LifecycleEventTypeV1) -> bool {
    matches!(
        event_type,
        LifecycleEventTypeV1::ToolProposed
            | LifecycleEventTypeV1::ToolAuthorized
            | LifecycleEventTypeV1::ToolDenied
            | LifecycleEventTypeV1::ToolStarted
            | LifecycleEventTypeV1::ToolCompleted
            | LifecycleEventTypeV1::ToolFailed
    )
}

pub(super) fn command(
    event_type: LifecycleEventTypeV1,
    payload: LifecyclePayloadV1,
    at: u64,
) -> LifecycleEventCommandV1 {
    LifecycleEventCommandV1 {
        event_type,
        occurred_at_unix_ms: at,
        instance_id: "rrd-1".into(),
        project_id: PROJECT.into(),
        member_id: Some("member-1".into()),
        session_id: SESSION.into(),
        turn_id: (!is_session_event(event_type)).then(|| TURN.into()),
        reasoning_run_id: is_reasoning_event(event_type).then(|| REASONING.into()),
        attempt_id: is_tool_event(event_type).then(|| ATTEMPT.into()),
        tool_call_id: is_tool_event(event_type).then(|| TOOL.into()),
        correlation_id: format!("correlation-{at}"),
        actor: "lifecycle-test".into(),
        adapter_kind: "orchestrator".into(),
        adapter_version: "1.0.0".into(),
        enforcement_level: LifecycleEnforcementLevelV1::Orchestrated,
        scope: SCOPE.into(),
        payload,
        read_stamp: None,
        trace: LifecycleTraceContextV1 {
            trace_id: format!("{at:032x}"),
            span_id: format!("{at:016x}"),
            parent_span_id: None,
        },
    }
}

pub(super) fn canonical_commands() -> Vec<LifecycleEventCommandV1> {
    let mut at = 0;
    let mut next = |event_type, payload| {
        at += 1;
        command(event_type, payload, at)
    };
    vec![
        next(
            LifecycleEventTypeV1::SessionOpened,
            LifecyclePayloadV1::SessionOpened {
                resumed: false,
                provider_session_sha256: Some(sha('0')),
            },
        ),
        next(
            LifecycleEventTypeV1::ProjectAttunementRequested,
            LifecyclePayloadV1::AttunementRequested {
                source_fingerprint_sha256: sha('1'),
            },
        ),
        next(
            LifecycleEventTypeV1::ProjectAttunementCompleted,
            LifecyclePayloadV1::AttunementCompleted {
                topology_sha256: sha('2'),
                profile_sha256: sha('3'),
                source_tree_sha256: sha('4'),
            },
        ),
        next(
            LifecycleEventTypeV1::TurnOpened,
            LifecyclePayloadV1::TurnOpened,
        ),
        next(
            LifecycleEventTypeV1::PromptReceived,
            LifecyclePayloadV1::PromptReceived {
                prompt_sha256: sha('5'),
                prompt_bytes: 128,
            },
        ),
        next(
            LifecycleEventTypeV1::PreflightStarted,
            LifecyclePayloadV1::PreflightStarted {
                source_tree_sha256: sha('4'),
            },
        ),
        next(
            LifecycleEventTypeV1::PreflightCompleted,
            LifecyclePayloadV1::PreflightCompleted {
                receipt_sha256: sha('6'),
                context_sha256: sha('7'),
            },
        ),
        next(
            LifecycleEventTypeV1::TaskClassificationCompleted,
            LifecyclePayloadV1::TaskClassified {
                task: LifecycleTaskKindV1::CodeChange,
                risk: LifecycleRiskV1::Medium,
                classification_sha256: sha('8'),
            },
        ),
        next(
            LifecycleEventTypeV1::ArchitectureAssessmentRequested,
            LifecyclePayloadV1::ArchitectureRequested {
                questions_sha256: sha('9'),
            },
        ),
        next(
            LifecycleEventTypeV1::ArchitectureAssessmentCompleted,
            LifecyclePayloadV1::ArchitectureCompleted {
                assessment_sha256: sha('a'),
                owner_boundary: "rrd-engine".into(),
                dependency_direction: "contract -> engine -> adapters".into(),
            },
        ),
        next(
            LifecycleEventTypeV1::PatternSelectionCompleted,
            LifecyclePayloadV1::PatternSelected {
                selection_sha256: sha('b'),
                pattern_count: 1,
            },
        ),
        next(
            LifecycleEventTypeV1::PlanningAuthorized,
            LifecyclePayloadV1::PlanningAuthorized {
                permit_sha256: sha('c'),
                expires_at_unix_ms: 10_000,
                plan_id: "rrflow-foundation".into(),
                plan_revision: 1,
                work_item_id: "G00-W02".into(),
            },
        ),
        next(
            LifecycleEventTypeV1::ContextAssembled,
            LifecyclePayloadV1::ContextAssembled {
                context_sha256: sha('d'),
                context_bytes: 1_024,
                truncated: false,
            },
        ),
        next(
            LifecycleEventTypeV1::PlanRecorded,
            LifecyclePayloadV1::PlanRecorded {
                plan_sha256: sha('e'),
                plan_id: "rrflow-foundation".into(),
                plan_revision: 1,
                work_item_id: "G00-W02".into(),
                source_tree_sha256: sha('4'),
                verification_plan_sha256: verification_plan_sha256(),
            },
        ),
        next(
            LifecycleEventTypeV1::ToolProposed,
            LifecyclePayloadV1::ToolProposed {
                tool_name: "apply_patch".into(),
                tool_request_sha256: sha('a'),
                mutation: true,
            },
        ),
        next(
            LifecycleEventTypeV1::ToolAuthorized,
            LifecyclePayloadV1::ToolDecision {
                decision_sha256: sha('b'),
                allowed: true,
                reason_code: "fresh-permit".into(),
            },
        ),
        next(
            LifecycleEventTypeV1::ToolStarted,
            LifecyclePayloadV1::ToolStarted {
                authorization_sha256: sha('b'),
            },
        ),
        next(
            LifecycleEventTypeV1::ToolCompleted,
            LifecyclePayloadV1::ToolFinished {
                tool_request_sha256: sha('a'),
                observation_sha256: sha('c'),
                success: true,
                project_state_changed: true,
            },
        ),
        next(
            LifecycleEventTypeV1::ProjectStateInvalidated,
            LifecyclePayloadV1::ProjectStateInvalidated {
                before_tree_sha256: sha('4'),
                after_tree_sha256: sha('d'),
                changed_paths_sha256: sha('e'),
            },
        ),
        next(
            LifecycleEventTypeV1::ProjectionRefreshCompleted,
            LifecyclePayloadV1::ProjectionRefresh {
                projection_sha256: sha('f'),
                evidence_sha256: sha('0'),
                fresh: true,
            },
        ),
        next(
            LifecycleEventTypeV1::VerificationCompleted,
            LifecyclePayloadV1::VerificationCompleted {
                verification_sha256: sha('1'),
                checks: 3,
                passed: true,
            },
        ),
        next(
            LifecycleEventTypeV1::ReasoningOutcomeRecorded,
            LifecyclePayloadV1::OutcomeRecorded {
                outcome_sha256: sha('2'),
                success: true,
            },
        ),
        next(
            LifecycleEventTypeV1::TurnClosed,
            LifecyclePayloadV1::TurnClosed {
                status: LifecycleTurnStatusV1::Completed,
            },
        ),
    ]
}

pub(super) fn drive_canonical_session<E: Engine>(engine: &E) -> LifecycleSessionSnapshotV1 {
    prepare_work_plan(engine);
    let commands = canonical_commands();
    let mut snapshot = None;
    for command in commands {
        let at = command.occurred_at_unix_ms;
        snapshot = Some(append_lifecycle_event(engine, command, at).unwrap());
    }
    let compaction_source = snapshot.as_ref().unwrap().state_sha256.clone();
    let tail = [
        command(
            LifecycleEventTypeV1::SessionCompactionStarted,
            LifecyclePayloadV1::Compaction {
                state_sha256: compaction_source.clone(),
            },
            24,
        ),
        command(
            LifecycleEventTypeV1::SessionCompactionCompleted,
            LifecyclePayloadV1::Compaction {
                state_sha256: compaction_source,
            },
            25,
        ),
        command(
            LifecycleEventTypeV1::ProjectAttunementRequested,
            LifecyclePayloadV1::AttunementRequested {
                source_fingerprint_sha256: sha('4'),
            },
            26,
        ),
        command(
            LifecycleEventTypeV1::ProjectAttunementCompleted,
            LifecyclePayloadV1::AttunementCompleted {
                topology_sha256: sha('5'),
                profile_sha256: sha('6'),
                source_tree_sha256: sha('7'),
            },
            27,
        ),
        command(
            LifecycleEventTypeV1::SessionClosed,
            LifecyclePayloadV1::SessionClosed {
                reason_code: "completed".into(),
            },
            28,
        ),
    ];
    for command in tail {
        let at = command.occurred_at_unix_ms;
        snapshot = Some(append_lifecycle_event(engine, command, at).unwrap());
    }
    let snapshot = snapshot.unwrap();
    assert_eq!(snapshot.phase, LifecyclePhaseV1::SessionClosed);
    assert_eq!(snapshot.event_count, 28);
    assert!(snapshot.active_turn_id.is_none());
    assert!(snapshot.active_tool_call_id.is_none());
    snapshot
}

pub(super) fn drive_until_turn_closed<E: Engine>(engine: &E) -> LifecycleSessionSnapshotV1 {
    prepare_work_plan(engine);
    let mut snapshot = None;
    for command in canonical_commands() {
        let at = command.occurred_at_unix_ms;
        snapshot = Some(append_lifecycle_event(engine, command, at).unwrap());
    }
    let snapshot = snapshot.unwrap();
    assert_eq!(snapshot.phase, LifecyclePhaseV1::TurnClosed);
    snapshot
}

pub(super) fn assert_chain<E: Engine>(engine: &E, expected: &LifecycleSessionSnapshotV1) {
    let events = load_lifecycle_events(engine, SCOPE, PROJECT, SESSION).unwrap();
    assert_eq!(events.len() as u64, expected.event_count);
    let mut previous = None;
    for (index, event) in events.iter().enumerate() {
        event.verify().unwrap();
        assert_eq!(event.sequence, index as u64 + 1);
        assert_eq!(event.previous_event_sha256.as_deref(), previous);
        previous = Some(event.event_sha256.as_str());
    }
    assert_eq!(events.last(), Some(&expected.latest_event));
}
