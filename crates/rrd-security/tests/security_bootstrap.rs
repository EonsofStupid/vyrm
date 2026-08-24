use rrd_contract::{CanonicalId, ResourceId, ResourceKind, ResourcePath};
use rrd_security::SecurityRepository;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use rrd_store::PersistentEngine;

#[test]
fn bootstrap_is_private_idempotent_restart_safe_and_drift_denying() {
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("instance");
    let secret_directory = temporary.path().join("secrets");
    fs::create_dir(&secret_directory).unwrap();
    let credential = secret_directory.join("operator.key");
    fs::write(&credential, b"operator-secret-value").unwrap();
    make_private(&credential);
    let manifest = temporary.path().join("bootstrap.json");
    write_manifest(&manifest, &credential);

    let first = invoke(&database, &manifest);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(String::from_utf8(first.stdout)
        .unwrap()
        .contains("initialized"));
    let second = invoke(&database, &manifest);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(String::from_utf8(second.stdout)
        .unwrap()
        .contains("unchanged"));

    let engine = PersistentEngine::open(&database).unwrap();
    let state = SecurityRepository::new(&engine, CanonicalId::new("kube-instance").unwrap())
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(state.revision, 1);
    assert_eq!(state.principals.len(), 1);
    let encoded = serde_json::to_string(&state).unwrap();
    assert!(!encoded.contains("operator-secret-value"));
    drop(engine);

    make_owner_writable(&credential);
    fs::write(&credential, b"different-secret").unwrap();
    make_private(&credential);
    let drift = invoke(&database, &manifest);
    assert!(!drift.status.success());
    assert!(String::from_utf8_lossy(&drift.stderr).contains("different policy"));
}

fn invoke(database: &Path, manifest: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rrd-security-bootstrap"))
        .args([
            "--db",
            database.to_str().unwrap(),
            "--instance",
            "kube-instance",
            "--manifest",
            manifest.to_str().unwrap(),
            "--at-unix-ms",
            "100",
        ])
        .output()
        .unwrap()
}

fn write_manifest(path: &Path, credential: &PathBuf) {
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, "kube-instance").unwrap()],
    };
    let manifest = json!({
        "format_version": 1,
        "revision": 1,
        "principals": [{
            "id": "operator",
            "kind": "service",
            "credential_file": credential,
            "not_before_unix_ms": 1,
            "expires_at_unix_ms": u64::MAX,
            "grants": [{
                "action": "session_create",
                "resource_prefix": resource,
            }],
        }],
    });
    fs::write(path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}

#[cfg(unix)]
fn make_private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    // Kubernetes projected Secrets commonly become group-readable through
    // `fsGroup`; prove that read-only group access remains admissible.
    fs::set_permissions(path, fs::Permissions::from_mode(0o440)).unwrap();
}

#[cfg(unix)]
fn make_owner_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(not(unix))]
fn make_private(_path: &Path) {}

#[cfg(not(unix))]
fn make_owner_writable(_path: &Path) {}
