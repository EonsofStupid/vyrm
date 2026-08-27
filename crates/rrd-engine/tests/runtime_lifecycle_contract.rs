#[allow(dead_code, unused_imports)]
#[path = "runtime_lifecycle_contract/fixture.rs"]
mod fixture;

use fixture::*;

#[test]
fn canonical_lifecycle_is_hash_chained_in_the_shared_runtime() {
    let engine = MemoryEngine::new();
    let snapshot = drive_canonical_session(&engine);
    assert_chain(&engine, &snapshot);
    assert_eq!(
        engine.runtime_cursor().unwrap(),
        1 + snapshot.event_count * 2
    );
    assert_eq!(
        load_lifecycle_session(&engine, SCOPE, PROJECT, SESSION)
            .unwrap()
            .unwrap(),
        snapshot
    );
}

#[test]
fn invalid_transition_does_not_advance_rrd() {
    let engine = MemoryEngine::new();
    let opened = canonical_commands().remove(0);
    append_lifecycle_event(&engine, opened, 1).unwrap();
    let before = engine.runtime_cursor().unwrap();
    let invalid = command(
        LifecycleEventTypeV1::PromptReceived,
        LifecyclePayloadV1::PromptReceived {
            prompt_sha256: sha('a'),
            prompt_bytes: 1,
        },
        2,
    );
    let error = append_lifecycle_event(&engine, invalid, 2)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("active turn") || error.contains("invalid lifecycle transition"),
        "unexpected denial: {error}"
    );
    assert_eq!(engine.runtime_cursor().unwrap(), before);
    assert_eq!(
        load_lifecycle_session(&engine, SCOPE, PROJECT, SESSION)
            .unwrap()
            .unwrap()
            .phase,
        LifecyclePhaseV1::SessionOpened
    );
}

#[test]
fn semantically_forged_runtime_event_fails_closed_on_replay() {
    let engine = MemoryEngine::new();
    let opened = append_lifecycle_event(&engine, canonical_commands().remove(0), 1).unwrap();
    let payload = LifecyclePayloadV1::AttunementRequested {
        source_fingerprint_sha256: sha('a'),
    };
    let payload_sha256 = digest::sha256_hex(&serde_json::to_vec(&payload).unwrap());
    let mut forged = LifecycleEventEnvelopeV1 {
        spec_version: LIFECYCLE_SPEC_VERSION,
        sequence: 99,
        event_id: "forged-event".into(),
        event_type: LifecycleEventTypeV1::ProjectAttunementRequested,
        occurred_at_unix_ms: 2,
        recorded_at_unix_ms: 2,
        instance_id: opened.instance_id.clone(),
        project_id: PROJECT.into(),
        member_id: opened.member_id.clone(),
        session_id: SESSION.into(),
        turn_id: None,
        reasoning_run_id: None,
        attempt_id: None,
        tool_call_id: None,
        correlation_id: "forged-correlation".into(),
        causation_id: Some(opened.latest_event.event_id.clone()),
        actor: "lifecycle-test".into(),
        adapter_kind: "orchestrator".into(),
        adapter_version: "1.0.0".into(),
        enforcement_level: LifecycleEnforcementLevelV1::Orchestrated,
        scope: SCOPE.into(),
        payload,
        payload_sha256,
        previous_state_sha256: Some(opened.state_sha256.clone()),
        read_stamp: None,
        trace: LifecycleTraceContextV1 {
            trace_id: format!("{:032x}", 2),
            span_id: format!("{:016x}", 2),
            parent_span_id: None,
        },
        previous_event_sha256: Some(opened.latest_event.event_sha256.clone()),
        event_sha256: String::new(),
    };
    forged.seal().unwrap();

    let mut properties = RuntimeProperties::from([
        (
            "envelope_json".into(),
            RuntimeValue::String(serde_json::to_string(&forged).unwrap()),
        ),
        (
            "event_sha256".into(),
            RuntimeValue::Digest(forged.event_sha256.clone()),
        ),
        ("sequence".into(), RuntimeValue::Unsigned(forged.sequence)),
        (
            "event_type".into(),
            RuntimeValue::String(forged.event_type.as_str().into()),
        ),
        ("project_id".into(), RuntimeValue::String(PROJECT.into())),
        ("session_id".into(), RuntimeValue::String(SESSION.into())),
    ]);
    let identity = digest::sha256_hex(
        &serde_json::to_vec(&(LIFECYCLE_SPEC_VERSION, PROJECT, SESSION)).unwrap(),
    );
    engine
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new(SCOPE).unwrap(),
            at: 2,
            actor: "lifecycle-test".into(),
            expected_cursor: engine.runtime_cursor().unwrap(),
            mutations: vec![RuntimeMutation::Event {
                event: RuntimeEvent {
                    kind: RuntimeType::new(LIFECYCLE_RUNTIME_EVENT_TYPE).unwrap(),
                    subject: Some(
                        RuntimeRef::new(
                            LIFECYCLE_RUNTIME_SESSION_TYPE,
                            format!("lifecycle-{identity}"),
                        )
                        .unwrap(),
                    ),
                    properties: std::mem::take(&mut properties),
                },
            }],
        })
        .unwrap();
    assert!(load_lifecycle_session(&engine, SCOPE, PROJECT, SESSION).is_err());
}

#[test]
fn native_reopen_replays_an_identical_aggregate() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("native");
    let engine = NativeEngine::open(&path).unwrap();
    let before = drive_canonical_session(&engine);
    drop(engine);

    let reopened = NativeEngine::open(&path).unwrap();
    let after = load_lifecycle_session(&reopened, SCOPE, PROJECT, SESSION)
        .unwrap()
        .unwrap();
    assert_eq!(after, before);
    assert_chain(&reopened, &after);
}

#[test]
fn compatibility_backend_reopen_replays_an_identical_aggregate() {
    let root = tempfile::tempdir().unwrap();
    let engine = Store::open(root.path()).unwrap();
    let before = drive_canonical_session(&engine);
    drop(engine);

    let reopened = Store::open(root.path()).unwrap();
    let after = load_lifecycle_session(&reopened, SCOPE, PROJECT, SESSION)
        .unwrap()
        .unwrap();
    assert_eq!(after, before);
    assert_chain(&reopened, &after);
}

#[test]
fn forged_compaction_source_is_denied_without_advancing_rrd() {
    let engine = MemoryEngine::new();
    drive_until_turn_closed(&engine);
    let before = engine.runtime_cursor().unwrap();
    let forged = command(
        LifecycleEventTypeV1::SessionCompactionStarted,
        LifecyclePayloadV1::Compaction {
            state_sha256: sha('0'),
        },
        24,
    );
    assert!(append_lifecycle_event(&engine, forged, 24).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn compaction_requires_reattunement_before_close() {
    let engine = MemoryEngine::new();
    let turn = drive_until_turn_closed(&engine);
    let source = turn.state_sha256;
    append_lifecycle_event(
        &engine,
        command(
            LifecycleEventTypeV1::SessionCompactionStarted,
            LifecyclePayloadV1::Compaction {
                state_sha256: source.clone(),
            },
            24,
        ),
        24,
    )
    .unwrap();
    append_lifecycle_event(
        &engine,
        command(
            LifecycleEventTypeV1::SessionCompactionCompleted,
            LifecyclePayloadV1::Compaction {
                state_sha256: source,
            },
            25,
        ),
        25,
    )
    .unwrap();
    let close = command(
        LifecycleEventTypeV1::SessionClosed,
        LifecyclePayloadV1::SessionClosed {
            reason_code: "stale-after-compaction".into(),
        },
        26,
    );
    assert!(append_lifecycle_event(&engine, close, 26).is_err());
    assert_eq!(
        load_lifecycle_session(&engine, SCOPE, PROJECT, SESSION)
            .unwrap()
            .unwrap()
            .phase,
        LifecyclePhaseV1::CompactionCompleted
    );
}

#[test]
fn reasoning_run_substitution_is_denied() {
    let engine = MemoryEngine::new();
    prepare_work_plan(&engine);
    let commands = canonical_commands();
    for command in commands.iter().take(14).cloned() {
        let at = command.occurred_at_unix_ms;
        append_lifecycle_event(&engine, command, at).unwrap();
    }
    let before = engine.runtime_cursor().unwrap();
    let mut proposed = commands[14].clone();
    proposed.reasoning_run_id = Some("reasoning-forged".into());
    assert!(append_lifecycle_event(&engine, proposed, 15).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn tool_attempt_substitution_is_denied() {
    let engine = MemoryEngine::new();
    prepare_work_plan(&engine);
    let commands = canonical_commands();
    for command in commands.iter().take(15).cloned() {
        let at = command.occurred_at_unix_ms;
        append_lifecycle_event(&engine, command, at).unwrap();
    }
    let before = engine.runtime_cursor().unwrap();
    let mut authorized = commands[15].clone();
    authorized.attempt_id = Some("attempt-forged".into());
    assert!(append_lifecycle_event(&engine, authorized, 16).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn native_reopen_mid_compaction_preserves_the_recovery_gate() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("native-mid-compaction");
    let engine = NativeEngine::open(&path).unwrap();
    let turn = drive_until_turn_closed(&engine);
    let source = turn.state_sha256;
    append_lifecycle_event(
        &engine,
        command(
            LifecycleEventTypeV1::SessionCompactionStarted,
            LifecyclePayloadV1::Compaction {
                state_sha256: source.clone(),
            },
            24,
        ),
        24,
    )
    .unwrap();
    drop(engine);

    let reopened = NativeEngine::open(&path).unwrap();
    let completed = append_lifecycle_event(
        &reopened,
        command(
            LifecycleEventTypeV1::SessionCompactionCompleted,
            LifecyclePayloadV1::Compaction {
                state_sha256: source,
            },
            25,
        ),
        25,
    )
    .unwrap();
    assert_eq!(completed.phase, LifecyclePhaseV1::CompactionCompleted);
    let close = command(
        LifecycleEventTypeV1::SessionClosed,
        LifecyclePayloadV1::SessionClosed {
            reason_code: "cannot-skip-refresh".into(),
        },
        26,
    );
    assert!(append_lifecycle_event(&reopened, close, 26).is_err());
}

#[test]
fn native_and_reference_engines_produce_identical_lifecycle_state() {
    let memory = MemoryEngine::new();
    let expected = drive_canonical_session(&memory);
    let root = tempfile::tempdir().unwrap();
    let native = NativeEngine::open(&root.path().join("native-differential")).unwrap();
    let actual = drive_canonical_session(&native);
    assert_eq!(actual, expected);
    assert_eq!(
        load_lifecycle_events(&native, SCOPE, PROJECT, SESSION).unwrap(),
        load_lifecycle_events(&memory, SCOPE, PROJECT, SESSION).unwrap()
    );
}

#[test]
fn recorded_plan_must_match_the_authorized_plan_and_work_item() {
    let engine = MemoryEngine::new();
    prepare_work_plan(&engine);
    let commands = canonical_commands();
    for command in commands.iter().take(13).cloned() {
        let at = command.occurred_at_unix_ms;
        append_lifecycle_event(&engine, command, at).unwrap();
    }
    let before = engine.runtime_cursor().unwrap();
    let mut mismatched = commands[13].clone();
    let LifecyclePayloadV1::PlanRecorded { work_item_id, .. } = &mut mismatched.payload else {
        panic!("fixture event 14 must be plan.recorded")
    };
    *work_item_id = "G00-W99".into();
    assert!(append_lifecycle_event(&engine, mismatched, 14).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn expired_planning_permit_cannot_authorize_a_tool() {
    let engine = MemoryEngine::new();
    prepare_work_plan(&engine);
    let mut commands = canonical_commands();
    let LifecyclePayloadV1::PlanningAuthorized {
        expires_at_unix_ms, ..
    } = &mut commands[11].payload
    else {
        panic!("fixture event 12 must be planning.authorized")
    };
    *expires_at_unix_ms = 15;
    for command in commands.iter().take(14).cloned() {
        let at = command.occurred_at_unix_ms;
        append_lifecycle_event(&engine, command, at).unwrap();
    }
    let before = engine.runtime_cursor().unwrap();
    let error = append_lifecycle_event(&engine, commands[14].clone(), 15)
        .unwrap_err()
        .to_string();
    assert!(error.contains("expired"), "unexpected denial: {error}");
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn tool_result_must_match_the_consumed_request() {
    let engine = MemoryEngine::new();
    prepare_work_plan(&engine);
    let commands = canonical_commands();
    for command in commands.iter().take(17).cloned() {
        let at = command.occurred_at_unix_ms;
        append_lifecycle_event(&engine, command, at).unwrap();
    }
    let before = engine.runtime_cursor().unwrap();
    let mut substituted = commands[17].clone();
    let LifecyclePayloadV1::ToolFinished {
        tool_request_sha256,
        ..
    } = &mut substituted.payload
    else {
        panic!("fixture event 18 must be tool.completed")
    };
    *tool_request_sha256 = sha('f');
    assert!(append_lifecycle_event(&engine, substituted, 18).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn consumed_tool_authorization_cannot_start_twice() {
    let engine = MemoryEngine::new();
    prepare_work_plan(&engine);
    let commands = canonical_commands();
    for command in commands.iter().take(17).cloned() {
        let at = command.occurred_at_unix_ms;
        append_lifecycle_event(&engine, command, at).unwrap();
    }
    let before = engine.runtime_cursor().unwrap();
    let mut replay = commands[16].clone();
    replay.occurred_at_unix_ms = 18;
    replay.correlation_id = "correlation-start-replay".into();
    assert!(append_lifecycle_event(&engine, replay, 18).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn substituted_tool_authorization_cannot_start() {
    let engine = MemoryEngine::new();
    prepare_work_plan(&engine);
    let commands = canonical_commands();
    for command in commands.iter().take(16).cloned() {
        let at = command.occurred_at_unix_ms;
        append_lifecycle_event(&engine, command, at).unwrap();
    }
    let before = engine.runtime_cursor().unwrap();
    let mut substituted = commands[16].clone();
    let LifecyclePayloadV1::ToolStarted {
        authorization_sha256,
    } = &mut substituted.payload
    else {
        panic!("fixture event 17 must be tool.started")
    };
    *authorization_sha256 = sha('f');
    assert!(append_lifecycle_event(&engine, substituted, 17).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}

#[test]
fn planning_authorization_requires_an_active_recorded_work_item() {
    let engine = MemoryEngine::new();
    install_work_plan(&engine, work_plan_definition(), 1, "test", "install-only").unwrap();
    let commands = canonical_commands();
    for command in commands.iter().take(11).cloned() {
        let at = command.occurred_at_unix_ms;
        append_lifecycle_event(&engine, command, at).unwrap();
    }
    let before = engine.runtime_cursor().unwrap();
    let error = append_lifecycle_event(&engine, commands[11].clone(), 12)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("active work-plan revision and item"),
        "unexpected denial: {error}"
    );
    assert_eq!(engine.runtime_cursor().unwrap(), before);
}
