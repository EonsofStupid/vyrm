use rrd_engine::{
    adapter_conformance_fixtures, adapter_coverage, classify_adapter_tool,
    normalize_adapter_fixture, normalize_hook_input, AdapterCoveragePointV1,
    AdapterCoverageStateV1, AdapterKindV1, AdapterToolClass, LifecycleEnforcementLevelV1,
};
use std::collections::BTreeSet;

#[test]
fn every_provider_and_local_fixture_emits_one_canonical_event() {
    let document = adapter_conformance_fixtures().unwrap();
    assert_eq!(document.fixtures.len(), AdapterKindV1::ALL.len());
    let mut seen = BTreeSet::new();
    for fixture in document.fixtures {
        assert!(seen.insert(fixture.adapter));
        let event = normalize_adapter_fixture(&fixture).unwrap();
        event.verify().unwrap();
        assert_eq!(event.canonical_event, fixture.expected.canonical_event);
        assert_eq!(event.session_id, fixture.expected.session_id);
        if let Some(tool_call_id) = fixture.expected.tool_call_id {
            assert_eq!(event.tool_call_id, tool_call_id);
        } else {
            assert!(event.tool_call_id.starts_with("derived-"));
        }
        assert_eq!(event.tool_name, fixture.expected.tool_name);
        assert_eq!(event.mutation_class, fixture.expected.mutation_class);
    }
    assert_eq!(seen, AdapterKindV1::ALL.into_iter().collect());
}

#[test]
fn coverage_vectors_are_complete_and_never_overstate_cooperative_paths() {
    for adapter in AdapterKindV1::ALL {
        let coverage = adapter_coverage(adapter);
        coverage.verify().unwrap();
        assert_eq!(coverage.entries.len(), AdapterCoveragePointV1::ALL.len());
    }

    let mcp = adapter_coverage(AdapterKindV1::Mcp);
    assert_eq!(
        mcp.enforcement_level,
        LifecycleEnforcementLevelV1::Cooperative
    );
    assert!(!mcp.planning_enforced);
    assert!(!mcp.mutation_enforced);
    assert!(!mcp.uncovered().is_empty());

    let copilot = adapter_coverage(AdapterKindV1::Copilot);
    assert!(!copilot.mutation_enforced);
    assert!(copilot.entries.iter().any(|entry| {
        entry.point == AdapterCoveragePointV1::TimeoutFailure
            && entry.state == AdapterCoverageStateV1::Uncovered
    }));

    for adapter in [
        AdapterKindV1::OpenAiOrchestrator,
        AdapterKindV1::XaiOrchestrator,
    ] {
        let coverage = adapter_coverage(adapter);
        assert_eq!(
            coverage.enforcement_level,
            LifecycleEnforcementLevelV1::ObserveOnly
        );
        assert!(!coverage.planning_enforced);
        assert!(!coverage.mutation_enforced);
        assert!(!coverage.uncovered().is_empty());
        assert!(coverage.entries.iter().any(|entry| {
            entry.point == AdapterCoveragePointV1::PreToolExternal
                && entry.state == AdapterCoverageStateV1::Observed
        }));
    }
    let local = adapter_coverage(AdapterKindV1::LocalRuntime);
    assert!(local.planning_enforced);
    assert!(local.mutation_enforced);
}

#[test]
fn unknown_and_malformed_mutation_payloads_fail_closed() {
    assert_eq!(
        classify_adapter_tool("future_mutator"),
        AdapterToolClass::Unknown
    );
    assert!(normalize_hook_input(
        Some("codex-cli"),
        "pre-tool-use",
        &serde_json::json!({
            "session_id":"session",
            "tool_name":"future_mutator",
            "tool_input":{}
        })
    )
    .unwrap_err()
    .contains("unknown tool"));
    assert!(normalize_hook_input(
        Some("github-copilot"),
        "pre-tool-use",
        &serde_json::json!({"sessionId":"session","toolName":"edit"})
    )
    .is_err());
    assert_eq!(classify_adapter_tool("Read"), AdapterToolClass::ReadOnly);

    let non_tool = normalize_hook_input(
        Some("github-copilot"),
        "session-start",
        &serde_json::json!({"sessionId":"session","source":"resume"}),
    )
    .unwrap();
    assert_eq!(non_tool["session_id"], "session");
    assert!(non_tool.get("tool_input").is_none());
}
