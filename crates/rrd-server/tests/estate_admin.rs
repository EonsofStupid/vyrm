use rrd_contract::CanonicalId;
use rrd_estate::{
    EstateRepository, LocalEstatePermission, LocalOperatorPolicy, LOCAL_OPERATOR_POLICY_FORMAT,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Output};
use vyrm_store::{Engine, PersistentEngine};

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rrd-estate-admin"))
        .args(arguments)
        .output()
        .unwrap()
}

fn value(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[cfg(unix)]
fn private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(not(unix))]
fn private(_path: &Path) {}

#[test]
fn authorized_admin_mutations_replay_reopen_and_journal_exact_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("authority");
    let denied_database = temporary.path().join("denied-authority");
    let key_path = temporary.path().join("operator.key");
    let policy_path = temporary.path().join("operator.json");
    let key = [11_u8; 32];
    std::fs::write(&key_path, key).unwrap();
    private(&key_path);
    let policy = LocalOperatorPolicy {
        format: LOCAL_OPERATOR_POLICY_FORMAT,
        operator_id: CanonicalId::new("operator-one").unwrap(),
        key_sha256: Sha256::digest(key)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        not_before_unix_ms: 10,
        expires_at_unix_ms: 1_000,
        estates: BTreeMap::from([(
            "estate-a".into(),
            BTreeSet::from([
                LocalEstatePermission::Create,
                LocalEstatePermission::SetDesired,
            ]),
        )]),
    };
    std::fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    private(&policy_path);

    let denied = run(&[
        "create",
        "--db",
        denied_database.to_str().unwrap(),
        "--policy",
        policy_path.to_str().unwrap(),
        "--key",
        key_path.to_str().unwrap(),
        "--estate",
        "estate-b",
        "--at",
        "20",
        "--request",
        "create-request",
        "--operation",
        "create-estate",
    ]);
    assert!(!denied.status.success());
    assert!(!denied_database.exists());

    let create = [
        "create",
        "--db",
        database.to_str().unwrap(),
        "--policy",
        policy_path.to_str().unwrap(),
        "--key",
        key_path.to_str().unwrap(),
        "--estate",
        "estate-a",
        "--at",
        "20",
        "--request",
        "create-request",
        "--operation",
        "create-estate",
    ];
    assert_eq!(value(&run(&create))["idempotent_replay"], false);
    assert_eq!(value(&run(&create))["idempotent_replay"], true);

    let desired = [
        "set-desired",
        "--db",
        database.to_str().unwrap(),
        "--policy",
        policy_path.to_str().unwrap(),
        "--key",
        key_path.to_str().unwrap(),
        "--estate",
        "estate-a",
        "--at",
        "30",
        "--request",
        "desired-request",
        "--operation",
        "start-project",
        "--instance",
        "project-a",
        "--idempotency",
        "start-project",
        "--phase",
        "running",
        "--deployment",
        "rrd-server",
        "--version",
        "0.1.0",
        "--configuration-sha256",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ];
    assert_eq!(value(&run(&desired))["idempotent_replay"], false);
    assert_eq!(value(&run(&desired))["idempotent_replay"], true);

    let engine = PersistentEngine::open(&database).unwrap();
    let repository = EstateRepository::new(&engine, CanonicalId::new("estate-a").unwrap());
    let document = repository.load().unwrap().unwrap();
    assert_eq!(document.revision, 2);
    assert_eq!(document.instances.len(), 1);
    let journal = engine.control_journal_since(0, 10).unwrap();
    assert_eq!(journal.len(), 2);
    assert!(journal.iter().all(|entry| entry.actor == "operator-one"));
    assert_eq!(journal[0].action, "estate.create");
    assert_eq!(journal[1].action, "estate.desired.set");
}
