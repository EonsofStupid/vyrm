use rrd_contract::{
    LifecycleEnforcementLevelV1, LifecycleEventCommandV1, LifecycleEventEnvelopeV1,
    LifecycleEventTypeV1, LifecyclePayloadV1, LifecycleProjectionRefreshV1,
    LifecycleSupervisorContextV1, LifecycleToolAuthorizationV1, LifecycleToolCompletionV1,
    LifecycleToolRequestV1, LifecycleTraceContextV1, LIFECYCLE_SPEC_VERSION,
    MAX_LIFECYCLE_EVENT_BYTES,
};

fn sha(byte: char) -> String {
    byte.to_string().repeat(64)
}

fn command(
    event_type: LifecycleEventTypeV1,
    payload: LifecyclePayloadV1,
) -> LifecycleEventCommandV1 {
    LifecycleEventCommandV1 {
        event_type,
        occurred_at_unix_ms: 100,
        instance_id: "instance-1".into(),
        project_id: "project-1".into(),
        member_id: None,
        session_id: "session-1".into(),
        turn_id: None,
        reasoning_run_id: None,
        attempt_id: None,
        tool_call_id: None,
        correlation_id: "correlation-1".into(),
        actor: "contract-test".into(),
        adapter_kind: "orchestrator".into(),
        adapter_version: "1.0.0".into(),
        enforcement_level: LifecycleEnforcementLevelV1::Orchestrated,
        scope: "project".into(),
        payload,
        read_stamp: None,
        trace: LifecycleTraceContextV1 {
            trace_id: "1".repeat(32),
            span_id: "2".repeat(16),
            parent_span_id: None,
        },
    }
}

fn seal(command: LifecycleEventCommandV1) -> LifecycleEventEnvelopeV1 {
    let mut event = LifecycleEventEnvelopeV1 {
        spec_version: LIFECYCLE_SPEC_VERSION,
        sequence: 1,
        event_id: "event-1".into(),
        event_type: command.event_type,
        occurred_at_unix_ms: command.occurred_at_unix_ms,
        recorded_at_unix_ms: command.occurred_at_unix_ms,
        instance_id: command.instance_id,
        project_id: command.project_id,
        member_id: command.member_id,
        session_id: command.session_id,
        turn_id: command.turn_id,
        reasoning_run_id: command.reasoning_run_id,
        attempt_id: command.attempt_id,
        tool_call_id: command.tool_call_id,
        correlation_id: command.correlation_id,
        causation_id: None,
        actor: command.actor,
        adapter_kind: command.adapter_kind,
        adapter_version: command.adapter_version,
        enforcement_level: command.enforcement_level,
        scope: command.scope,
        payload: command.payload,
        payload_sha256: String::new(),
        previous_state_sha256: None,
        read_stamp: command.read_stamp,
        trace: command.trace,
        previous_event_sha256: None,
        event_sha256: String::new(),
    };
    event.seal().unwrap();
    event
}

#[test]
fn sealed_envelope_detects_payload_and_chain_tampering() {
    let event = seal(command(
        LifecycleEventTypeV1::SessionOpened,
        LifecyclePayloadV1::SessionOpened {
            resumed: false,
            provider_session_sha256: Some(sha('a')),
        },
    ));
    event.verify().unwrap();

    let mut tampered = event.clone();
    tampered.actor = "another-actor".into();
    assert!(tampered.verify().is_err());

    let mut tampered = event;
    tampered.payload = LifecyclePayloadV1::SessionOpened {
        resumed: true,
        provider_session_sha256: Some(sha('a')),
    };
    assert!(tampered.verify().is_err());
}

#[test]
fn event_payload_and_reasoning_coordinates_fail_closed() {
    let mismatch = command(
        LifecycleEventTypeV1::SessionOpened,
        LifecyclePayloadV1::TurnOpened,
    );
    assert!(mismatch.validate().is_err());

    let mut tool = command(
        LifecycleEventTypeV1::ToolProposed,
        LifecyclePayloadV1::ToolProposed {
            tool_name: "apply_patch".into(),
            tool_request_sha256: sha('b'),
            mutation: true,
        },
    );
    tool.turn_id = Some("turn-1".into());
    tool.tool_call_id = Some("tool-1".into());
    tool.attempt_id = Some("attempt-1".into());
    assert!(tool.validate().is_err());
    tool.reasoning_run_id = Some("reasoning-1".into());
    tool.validate().unwrap();
}

#[test]
fn unknown_fields_and_event_types_are_rejected() {
    let mut value = serde_json::to_value(command(
        LifecycleEventTypeV1::SessionOpened,
        LifecyclePayloadV1::SessionOpened {
            resumed: false,
            provider_session_sha256: None,
        },
    ))
    .unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<LifecycleEventCommandV1>(value).is_err());

    let unknown = serde_json::json!({
        "event_type": "session.invented",
        "occurred_at_unix_ms": 100
    });
    assert!(serde_json::from_value::<LifecycleEventCommandV1>(unknown).is_err());
}

#[test]
fn oversized_event_is_rejected() {
    let mut event = command(
        LifecycleEventTypeV1::SessionClosed,
        LifecyclePayloadV1::SessionClosed {
            reason_code: "x".repeat(MAX_LIFECYCLE_EVENT_BYTES),
        },
    );
    event.adapter_version = "1".into();
    assert!(event.validate().is_err());
}

#[test]
fn compaction_may_preserve_an_active_turn_coordinate() {
    for turn_id in [None, Some("turn-1".into())] {
        let mut event = command(
            LifecycleEventTypeV1::SessionCompactionStarted,
            LifecyclePayloadV1::Compaction {
                state_sha256: sha('c'),
            },
        );
        event.turn_id = turn_id;
        event.validate().unwrap();
    }
}

#[test]
fn golden_v1_command_roundtrips_without_hidden_defaults() {
    let source = include_str!("../fixtures/lifecycle-command-v1.json");
    let command: LifecycleEventCommandV1 = serde_json::from_str(source).unwrap();
    command.validate().unwrap();
    let encoded = serde_json::to_value(&command).unwrap();
    let golden: serde_json::Value = serde_json::from_str(source).unwrap();
    assert_eq!(encoded, golden);
}

#[test]
fn supervisor_contracts_are_strict_and_validate_exact_identities() {
    let context = LifecycleSupervisorContextV1 {
        scope: "project:foundation".into(),
        project_id: "foundation".into(),
        session_id: "session-1".into(),
        actor: "contract-test".into(),
        adapter_kind: "command_proxy".into(),
        adapter_version: "1.0.0".into(),
        enforcement_level: LifecycleEnforcementLevelV1::Proxied,
    };
    context.validate().unwrap();
    let mut encoded = serde_json::to_value(&context).unwrap();
    encoded["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<LifecycleSupervisorContextV1>(encoded).is_err());

    LifecycleToolRequestV1 {
        tool_name: "RRFlowExec".into(),
        tool_request_sha256: sha('a'),
        tool_call_id: "tool-1".into(),
        mutation: true,
    }
    .validate()
    .unwrap();
    assert!(LifecycleToolAuthorizationV1 {
        attempt_id: "attempt-1".into(),
        tool_call_id: "tool-1".into(),
        tool_request_sha256: sha('a'),
        decision_sha256: "not-a-digest".into(),
    }
    .validate()
    .is_err());
    assert!(LifecycleToolCompletionV1 {
        observation_sha256: "not-a-digest".into(),
        success: true,
        project_state_changed: false,
    }
    .validate()
    .is_err());
    let refresh = LifecycleProjectionRefreshV1 {
        before_tree_sha256: sha('1'),
        after_tree_sha256: sha('2'),
        changed_paths_sha256: sha('3'),
        projection_sha256: sha('4'),
        evidence_sha256: sha('5'),
    };
    refresh.validate().unwrap();
    let mut encoded = serde_json::to_value(refresh).unwrap();
    encoded["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<LifecycleProjectionRefreshV1>(encoded).is_err());
}
