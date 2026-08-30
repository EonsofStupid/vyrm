use rrd_contract::{
    AdapterCoverageEntryV1, AdapterCoveragePointV1, AdapterCoverageStateV1,
    AdapterCoverageVectorV1, AdapterKindV1, AdapterMutationClassV1, CanonicalAdapterEventV1,
    LifecycleEnforcementLevelV1, LifecycleEventTypeV1, ADAPTER_CONFORMANCE_FORMAT_VERSION,
};

fn coverage() -> AdapterCoverageVectorV1 {
    let mut coverage = AdapterCoverageVectorV1 {
        format: ADAPTER_CONFORMANCE_FORMAT_VERSION,
        adapter: AdapterKindV1::OpenAiOrchestrator,
        adapter_version: "responses-v1".into(),
        enforcement_level: LifecycleEnforcementLevelV1::Orchestrated,
        planning_enforced: true,
        mutation_enforced: true,
        entries: AdapterCoveragePointV1::ALL
            .into_iter()
            .map(|point| AdapterCoverageEntryV1 {
                point,
                state: AdapterCoverageStateV1::Enforced,
                reason: "RRFlow owns this orchestrated transition".into(),
            })
            .collect(),
        coverage_sha256: String::new(),
    };
    coverage.seal().unwrap();
    coverage
}

#[test]
fn coverage_is_complete_sealed_and_cannot_overstate_readiness() {
    let value = coverage();
    value.verify().unwrap();
    assert!(value.uncovered().is_empty());

    let mut omitted = value.clone();
    omitted.entries.pop();
    omitted.seal().unwrap_err();

    let mut substituted = value.clone();
    substituted.entries[0].reason = "substituted".into();
    substituted.verify().unwrap_err();

    let mut overstated = value;
    overstated.entries[1].state = AdapterCoverageStateV1::Observed;
    overstated.seal().unwrap_err();
}

#[test]
fn canonical_adapter_event_binds_native_and_tool_identity() {
    let mut event = CanonicalAdapterEventV1 {
        format: ADAPTER_CONFORMANCE_FORMAT_VERSION,
        adapter: AdapterKindV1::Codex,
        adapter_version: "hooks-v1".into(),
        native_event: "PreToolUse".into(),
        canonical_event: LifecycleEventTypeV1::ToolProposed,
        session_id: "session-one".into(),
        tool_call_id: "call-one".into(),
        tool_name: "apply_patch".into(),
        tool_input_sha256: "1".repeat(64),
        mutation_class: AdapterMutationClassV1::Patch,
        event_sha256: String::new(),
    };
    event.seal().unwrap();
    event.verify().unwrap();
    event.tool_name = "unknown".into();
    event.verify().unwrap_err();
}
