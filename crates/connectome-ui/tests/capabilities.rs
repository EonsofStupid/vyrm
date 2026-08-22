use connectome_ui::{capabilities, CapabilityMaturity};

#[test]
fn diagnostics_handshake_is_replayable_and_truthfully_matured() {
    let view = capabilities(false);

    assert_eq!(view.protocol, "vyrm-diagnostics");
    assert_eq!(view.version, 1);
    assert!(view.developer_diagnostics);
    assert!(!view.runners_enabled);
    assert_eq!(view.providers, ["observe"]);
    assert!(view.replay.persisted);
    assert!(view.replay.restart_recoverable);
    assert!(view.replay.seekable);
    assert!(view.replay.reversible);
    assert_eq!(view.replay.speeds, [0.5, 1.0, 2.0, 4.0, 8.0]);

    let maturity = |id| {
        view.engine
            .iter()
            .find(|item| item.id == id)
            .unwrap_or_else(|| panic!("missing capability {id}"))
            .maturity
    };
    assert_eq!(maturity("filtered_hnsw"), CapabilityMaturity::Alpha);
    assert_eq!(maturity("multi_model_engine"), CapabilityMaturity::Alpha);
    assert_eq!(
        maturity("native_embedding_generation"),
        CapabilityMaturity::Alpha
    );
    assert_eq!(maturity("offline_edge"), CapabilityMaturity::Alpha);
    assert_eq!(maturity("audit_logging"), CapabilityMaturity::Partial);
    assert_eq!(maturity("gpu_indexing"), CapabilityMaturity::Experimental);
    assert_eq!(
        maturity("multivector_late_interaction"),
        CapabilityMaturity::Partial
    );
    assert_eq!(maturity("turboquant"), CapabilityMaturity::Planned);
    assert_eq!(
        maturity("kubernetes_hybrid_cloud"),
        CapabilityMaturity::Planned
    );
}

#[test]
fn enabling_frontier_runners_changes_only_the_exposed_runner_envelope() {
    let disabled = capabilities(false);
    let enabled = capabilities(true);

    assert!(!disabled.runners_enabled);
    assert!(enabled.runners_enabled);
    assert_eq!(enabled.providers, ["observe", "codex", "claude"]);
    assert_eq!(disabled.replay.persisted, enabled.replay.persisted);
    assert_eq!(disabled.engine.len(), enabled.engine.len());
}
