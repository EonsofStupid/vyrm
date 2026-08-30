use rrd_contract::{
    LifecycleEnforcementLevelV1, LifecycleEventCommandV1, LifecycleEventTypeV1, LifecyclePayloadV1,
    LifecyclePhaseV1, LifecycleSessionSnapshotV1, LifecycleTraceContextV1,
};
use rrd_engine::{InstanceBinding, InstanceManifest, RrdEngine};
use serde_json::json;

fn command(
    project_id: &str,
    event_type: LifecycleEventTypeV1,
    payload: LifecyclePayloadV1,
    at: u64,
) -> LifecycleEventCommandV1 {
    LifecycleEventCommandV1 {
        event_type,
        occurred_at_unix_ms: at,
        instance_id: project_id.into(),
        project_id: project_id.into(),
        member_id: None,
        session_id: "surface-session-1".into(),
        turn_id: None,
        reasoning_run_id: None,
        attempt_id: None,
        tool_call_id: None,
        correlation_id: format!("surface-{at}"),
        actor: "test:lifecycle-surface".into(),
        adapter_kind: "mcp".into(),
        adapter_version: "1.0.0".into(),
        enforcement_level: LifecycleEnforcementLevelV1::Cooperative,
        scope: "instance:default".into(),
        payload,
        read_stamp: None,
        trace: LifecycleTraceContextV1 {
            trace_id: format!("{at:032x}"),
            span_id: format!("{at:016x}"),
            parent_span_id: None,
        },
    }
}

fn apply(
    engine: &RrdEngine,
    root: &std::path::Path,
    command: LifecycleEventCommandV1,
    at: u64,
) -> LifecycleSessionSnapshotV1 {
    let result = engine
        .call_runtime_tool(
            root,
            "rrflow_lifecycle",
            &json!({"command": command, "recorded_at_unix_ms": at}),
            at,
        )
        .unwrap();
    serde_json::from_str(&result.text).unwrap()
}

#[test]
fn generated_lifecycle_tool_appends_the_canonical_chain_across_reopen() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    InstanceManifest::ensure_dedicated(&root).unwrap();
    let binding = InstanceBinding::discover(&root).unwrap();
    let project_id = binding.manifest.id.clone();

    let engine = RrdEngine::open_bound(&binding).unwrap();
    let opened = apply(
        &engine,
        &root,
        command(
            &project_id,
            LifecycleEventTypeV1::SessionOpened,
            LifecyclePayloadV1::SessionOpened {
                resumed: false,
                provider_session_sha256: None,
            },
            100,
        ),
        100,
    );
    assert_eq!(opened.phase, LifecyclePhaseV1::SessionOpened);
    assert_eq!(opened.event_count, 1);
    drop(engine);

    let engine = RrdEngine::open_bound(&binding).unwrap();
    let attunement = apply(
        &engine,
        &root,
        command(
            &project_id,
            LifecycleEventTypeV1::ProjectAttunementRequested,
            LifecyclePayloadV1::AttunementRequested {
                source_fingerprint_sha256: "a".repeat(64),
            },
            101,
        ),
        101,
    );
    assert_eq!(attunement.phase, LifecyclePhaseV1::AttunementPending);
    assert_eq!(attunement.event_count, 2);
    assert_eq!(
        attunement.latest_event.previous_event_sha256,
        Some(opened.latest_event.event_sha256)
    );
}

#[test]
fn lifecycle_tool_rejects_the_retired_hook_shape() {
    let temporary = tempfile::tempdir().unwrap();
    InstanceManifest::ensure_dedicated(temporary.path()).unwrap();
    let binding = InstanceBinding::discover(temporary.path()).unwrap();
    let engine = RrdEngine::open_bound(&binding).unwrap();
    let error = engine
        .call_runtime_tool(
            temporary.path(),
            "rrflow_lifecycle",
            &json!({"event":"pre-tool-use","input":{}}),
            100,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field") || error.contains("missing field"));
}
