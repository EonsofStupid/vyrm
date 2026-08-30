use rrd_core::{Claim, Predicate, Producer, Reader, Subject};
use rrd_engine::{
    load_task_preflight_receipt, preflight_task, EvidenceDisposition, InstanceManifest,
};
use rrd_store::{Engine, MemoryEngine, PersistentEngine};

fn claim(subject: &str, object: &str, at: u64) -> Claim {
    Claim::new(
        Subject::new(subject).unwrap(),
        Predicate::new("status").unwrap(),
        object,
        at,
        at,
        Producer {
            actor: "test:context".into(),
            on_behalf_of: None,
            session: None,
        },
    )
}

fn project() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    InstanceManifest::ensure_dedicated(root.path()).unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='task-context'\nversion='0.1.0'\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("AGENTS.md"),
        "# Policy\nInspect the existing engine boundary before mutation.\n",
    )
    .unwrap();
    root
}

#[test]
fn one_task_routes_only_relevant_source_claims_and_all_required_receipt_stamps() {
    let root = project();
    std::fs::write(
        root.path().join("src/payments.rs"),
        "pub fn payments_handler() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/unrelated.rs"),
        "pub fn unrelated_handler() {}\n",
    )
    .unwrap();
    let store = MemoryEngine::new();
    Engine::assert(
        &store,
        &claim("platform:payments", "use-existing-ledger", 1),
    )
    .unwrap();
    Engine::assert(&store, &claim("platform:unrelated", "do-not-inject", 2)).unwrap();
    let reader = Reader::new("test:task-context").unwrap();

    let flight = preflight_task(
        &store,
        root.path(),
        Some("codex-cli"),
        &reader,
        10,
        1_500,
        Some("implement payments handler"),
    )
    .unwrap();
    assert!(flight.context.contains("src/payments.rs"));
    assert!(flight.context.contains("use-existing-ledger"));
    assert!(!flight.context.contains("src/unrelated.rs"));
    assert!(!flight.context.contains("do-not-inject"));

    let receipt = flight.context_receipt.unwrap();
    assert!(receipt.verify());
    assert_eq!(receipt.route_disposition, EvidenceDisposition::Current);
    assert_eq!(receipt.recall_disposition, EvidenceDisposition::Current);
    assert_eq!(
        receipt.pattern_disposition,
        EvidenceDisposition::Unavailable
    );
    assert_eq!(receipt.policy_disposition, EvidenceDisposition::Current);
    assert_eq!(receipt.topology_sha256.len(), 64);
    assert_eq!(receipt.profile_sha256.len(), 64);
    assert_eq!(receipt.source_tree_sha256.len(), 64);
    assert_eq!(receipt.route_sha256.len(), 64);
    assert_eq!(receipt.pattern_sha256.len(), 64);
    assert_eq!(receipt.policy_sha256.len(), 64);
    assert_eq!(receipt.projection_sha256.len(), 64);
    assert_eq!(receipt.rrd_reads_sha256.len(), 64);
    assert!(receipt
        .rrd_reads
        .iter()
        .any(|read| read.purpose == "attunement"));
    assert!(receipt
        .rrd_reads
        .iter()
        .any(|read| read.purpose == "task-context"));
    assert_eq!(load_task_preflight_receipt(&store).unwrap(), Some(receipt));
}

#[test]
fn truncated_and_stale_evidence_is_rendered_and_sealed_explicitly() {
    let root = project();
    let mut alpha = String::from("pub fn alpha() {}\n");
    let mut beta = String::from("pub fn beta() {}\n");
    for _ in 0..2_100 {
        alpha.push_str("// alpha padding\n");
        beta.push_str("// beta padding\n");
    }
    std::fs::write(root.path().join("src/alpha.rs"), alpha).unwrap();
    std::fs::write(root.path().join("src/beta.rs"), beta).unwrap();
    let store = MemoryEngine::new();
    Engine::assert(&store, &claim("platform:alpha", &"a".repeat(200), 1)).unwrap();
    Engine::assert(&store, &claim("platform:beta", &"b".repeat(200), 2)).unwrap();
    let reader = Reader::new("test:truncated-context").unwrap();

    let flight = preflight_task(
        &store,
        root.path(),
        None,
        &reader,
        10,
        1,
        Some("change alpha beta"),
    )
    .unwrap();
    let receipt = flight.context_receipt.unwrap();
    assert_eq!(receipt.route_disposition, EvidenceDisposition::Truncated);
    assert_eq!(receipt.recall_disposition, EvidenceDisposition::Truncated);
    assert_eq!(receipt.projection_disposition, EvidenceDisposition::Stale);
    assert!(flight.context.contains("EXCLUDED source route"));
    assert!(flight.context.contains("EXCLUDED recall"));
    assert!(flight.context.contains("STALE projection"));
    assert!(flight.context.contains("pattern=unavailable"));
}

#[test]
fn the_task_receipt_survives_engine_reopen_with_the_same_digest() {
    let root = project();
    std::fs::write(
        root.path().join("src/durable.rs"),
        "pub fn durable_context() {}\n",
    )
    .unwrap();
    let database = root.path().join(".rrflow/rrd");
    let reader = Reader::new("test:context-reopen").unwrap();
    let expected = {
        let store = PersistentEngine::open(&database).unwrap();
        preflight_task(
            &store,
            root.path(),
            None,
            &reader,
            10,
            1_500,
            Some("inspect durable context"),
        )
        .unwrap()
        .context_receipt
        .unwrap()
    };
    let store = PersistentEngine::open(&database).unwrap();
    let loaded = load_task_preflight_receipt(&store).unwrap().unwrap();
    assert_eq!(loaded, expected);
    assert!(loaded.verify());
}
