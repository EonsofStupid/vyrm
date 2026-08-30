use rrd_core::Reader;
use rrd_core::ReasoningPayload;
use rrd_engine::{
    ensure_routing_fresh, handle, load_project_artifacts, load_routing, preflight, reset_routing,
    HookContext, HookEvent,
};
use rrd_store::{Engine, MemoryEngine, PersistentEngine};

fn edit() -> serde_json::Value {
    serde_json::json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": "src/lib.rs"}
    })
}

fn declare_attempt(store: &impl Engine, run: &str) {
    for (at, payload) in [
        (
            1,
            ReasoningPayload::Goal {
                statement: "edit source".into(),
                acceptance: vec!["source is current".into()],
            },
        ),
        (
            2,
            ReasoningPayload::Plan {
                hypothesis: "the edit is required".into(),
                steps: vec!["edit".into()],
            },
        ),
        (
            3,
            ReasoningPayload::Attempt {
                summary: "apply edit".into(),
                actions: vec!["Edit src/lib.rs".into()],
            },
        ),
    ] {
        rrd_engine::record_reasoning(store, run, at, "agent:test", payload).unwrap();
    }
}

#[test]
fn attunement_receipt_survives_engine_reopen() {
    let root = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    std::fs::write(root.path().join("lib.rs"), "pub fn durable() {}\n").unwrap();
    let database = root.path().join(".rrflow/rrd");
    let reader = Reader::new("test:attunement-reopen").unwrap();

    {
        let store = PersistentEngine::open(&database).unwrap();
        declare_attempt(&store, "attunement-reopen");
        let flight = preflight(&store, root.path(), None, &reader, 1_000, 1_500).unwrap();
        assert!(flight.attunement.is_some());
    }

    let store = PersistentEngine::open(&database).unwrap();
    let ctx = HookContext {
        store: &store,
        root: root.path(),
        harness: None,
        reader: &reader,
        now: 2_000,
        budget: 1_500,
    };
    let response = handle(&ctx, HookEvent::PreToolUse, &edit()).unwrap();
    assert!(response.stdout.is_empty());
    assert!(response.detail.unwrap().contains("attunement receipt"));
}

#[test]
fn manifest_drift_denies_even_when_the_source_index_is_unchanged() {
    let root = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    std::fs::write(root.path().join("lib.rs"), "pub fn item() {}\n").unwrap();
    std::fs::write(root.path().join("Cargo.toml"), "[package]\nname='before'\n").unwrap();
    let store = MemoryEngine::new();
    let reader = Reader::new("test:manifest-drift").unwrap();
    declare_attempt(&store, "manifest-drift");
    preflight(&store, root.path(), None, &reader, 1_000, 1_500).unwrap();

    std::fs::write(root.path().join("Cargo.toml"), "[package]\nname='after'\n").unwrap();
    let ctx = HookContext {
        store: &store,
        root: root.path(),
        harness: None,
        reader: &reader,
        now: 2_000,
        budget: 1_500,
    };
    let response = handle(&ctx, HookEvent::PreToolUse, &edit()).unwrap();
    assert!(response.stdout.contains("\"permissionDecision\":\"deny\""));
    assert!(response.stdout.contains("changed after planning"));
}

#[test]
fn preflight_receipt_denies_a_stale_tree_until_reattunement() {
    let root = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    let source = root.path().join("src/lib.rs");
    std::fs::write(&source, "pub fn alpha() {}\n").unwrap();
    let store = MemoryEngine::new();
    let reader = Reader::new("test:routing").unwrap();
    declare_attempt(&store, "routing-refresh");

    let flight = preflight(&store, root.path(), None, &reader, 1_000, 1_500).unwrap();
    let ready = flight.routing.expect("preflight establishes routing");
    assert_eq!(ready.files, 1);
    assert!(flight
        .context
        .contains("[rrflow] routing: built generation 1"));
    let index = load_routing(&store, root.path()).unwrap().unwrap();
    assert_eq!(index.route("alpha", 5)[0].path, source);

    // Same-size, immediate rewrite: nanosecond timestamp evidence catches the
    // stale tree without relying on a sleep or a length change.
    std::fs::write(&source, "pub fn omega() {}\n").unwrap();
    let ctx = HookContext {
        store: &store,
        root: root.path(),
        harness: None,
        reader: &reader,
        now: 2_000,
        budget: 1_500,
    };
    let response = handle(&ctx, HookEvent::PreToolUse, &edit()).unwrap();
    assert!(response.stdout.contains("\"permissionDecision\":\"deny\""));
    assert!(response.stdout.contains("changed after planning"));

    let index = load_routing(&store, root.path()).unwrap().unwrap();
    assert!(index.route("alpha", 5).is_empty());
    assert_eq!(index.route("omega", 5)[0].path, source);

    let second = preflight(&store, root.path(), None, &reader, 2_001, 1_500).unwrap();
    assert_ne!(
        second.attunement.unwrap().receipt_sha256,
        flight.attunement.unwrap().receipt_sha256
    );
    let response = handle(&ctx, HookEvent::PreToolUse, &edit()).unwrap();
    assert!(
        response.stdout.is_empty(),
        "fresh re-attunement opens the gate"
    );
}

#[test]
fn corrupt_or_unreadable_routing_state_fails_closed_until_explicit_reset() {
    let root = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    let source = root.path().join("lib.rs");
    std::fs::write(&source, "pub fn sound() {}\n").unwrap();
    let store = MemoryEngine::new();
    let reader = Reader::new("test:routing-gate").unwrap();
    declare_attempt(&store, "routing-corruption");
    ensure_routing_fresh(&store, root.path()).unwrap();
    store
        .put_projection(rrd_engine::routing::ROUTING_PROJECTION, b"not json")
        .unwrap();

    let flight = preflight(&store, root.path(), None, &reader, 1_000, 1_500).unwrap();
    assert!(flight.routing.is_none());
    assert!(flight
        .warnings
        .iter()
        .any(|warning| warning.contains("reset-routing")));

    let ctx = HookContext {
        store: &store,
        root: root.path(),
        harness: None,
        reader: &reader,
        now: 2_000,
        budget: 1_500,
    };
    let response = handle(&ctx, HookEvent::PreToolUse, &edit()).unwrap();
    assert!(response.stdout.contains("\"permissionDecision\":\"deny\""));
    assert!(response.stdout.contains("reset-routing"));

    reset_routing(&store, root.path()).unwrap();
    let response = handle(&ctx, HookEvent::PreToolUse, &edit()).unwrap();
    assert!(response.stdout.contains("\"permissionDecision\":\"deny\""));
    assert!(response.stdout.contains("attunement"));
    preflight(&store, root.path(), None, &reader, 2_001, 1_500).unwrap();
    let response = handle(&ctx, HookEvent::PreToolUse, &edit()).unwrap();
    assert!(
        response.stdout.is_empty(),
        "explicit reset plus fresh attunement reopens the gate"
    );

    // An indexable file that cannot be decoded must never leave an older
    // source entry looking current.
    std::fs::write(&source, [0xff; 18]).unwrap();
    let response = handle(&ctx, HookEvent::PreToolUse, &edit()).unwrap();
    assert!(response.stdout.contains("\"permissionDecision\":\"deny\""));
    assert!(response
        .stdout
        .contains("cannot establish routing freshness"));
}

#[test]
fn a_projection_is_bound_to_one_canonical_project_root() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let store = MemoryEngine::new();
    ensure_routing_fresh(&store, first.path()).unwrap();

    let error = ensure_routing_fresh(&store, second.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("belongs to"));
    assert!(error.contains("reset-routing"));
}

#[test]
fn automatic_refresh_and_manual_rebuild_persist_identical_project_artifacts() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='topology'\nversion='0.1.0'\n",
    )
    .unwrap();
    std::fs::write(root.path().join("lib.rs"), "pub fn topology() {}\n").unwrap();
    let store = MemoryEngine::new();

    let automatic = ensure_routing_fresh(&store, root.path()).unwrap();
    let (topology, profile) = load_project_artifacts(&store, root.path())
        .unwrap()
        .unwrap();
    assert_eq!(automatic.topology_sha256, topology.digest);
    assert_eq!(automatic.profile_sha256, profile.digest);

    let manual = reset_routing(&store, root.path()).unwrap();
    let (rebuilt_topology, rebuilt_profile) = load_project_artifacts(&store, root.path())
        .unwrap()
        .unwrap();
    assert_eq!(manual.topology_sha256, automatic.topology_sha256);
    assert_eq!(manual.profile_sha256, automatic.profile_sha256);
    assert_eq!(rebuilt_topology, topology);
    assert_eq!(rebuilt_profile, profile);
}

#[test]
fn project_artifacts_survive_reopen_and_malformed_evidence_denies_refresh() {
    let root = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    std::fs::write(
        root.path().join("package.json"),
        "{\"name\":\"durable\",\"scripts\":{\"test\":\"vitest\"}}",
    )
    .unwrap();
    std::fs::write(
        root.path().join("pnpm-workspace.yaml"),
        "packages:\n  - .\n",
    )
    .unwrap();
    let database = root.path().join(".rrflow/rrd");
    let expected = {
        let store = PersistentEngine::open(&database).unwrap();
        let ready = ensure_routing_fresh(&store, root.path()).unwrap();
        (ready.topology_sha256, ready.profile_sha256)
    };
    let store = PersistentEngine::open(&database).unwrap();
    let (topology, profile) = load_project_artifacts(&store, root.path())
        .unwrap()
        .unwrap();
    assert_eq!(expected, (topology.digest, profile.digest));

    std::fs::write(root.path().join("package.json"), "{broken").unwrap();
    let error = ensure_routing_fresh(&store, root.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("package.json"));
    assert!(error.contains("malformed"));
}
