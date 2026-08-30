use rrd_contract::{CanonicalId, ResourceId, ResourceKind, ResourcePath};
use rrd_core::digest;
use rrd_engine::{load_or_create_token_key, InstanceBinding, InstanceManifest, RrdEngine};
use rrd_security::{
    Action, Principal, PrincipalKind, ResourceGrant, SecurityRepository, SecurityState,
    SECURITY_FORMAT,
};
use rrd_server::RrdHttpServer;
use rrd_store::PersistentEngine;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::process::{Command, Output};

fn rrflow(arguments: Vec<OsString>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rrflow"))
        .args(arguments)
        .output()
        .expect("run rrflow")
}

fn text(output: &Output) -> (&str, &str) {
    (
        std::str::from_utf8(&output.stdout).unwrap(),
        std::str::from_utf8(&output.stderr).unwrap(),
    )
}

#[test]
fn embedded_and_authenticated_daemon_modes_share_the_generated_runtime() {
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    InstanceManifest::ensure_dedicated_as(&project, "cli-runtime-test").unwrap();
    let binding = InstanceBinding::discover(&project).unwrap();
    let store = binding.expected_store();
    let instance = CanonicalId::new("cli-runtime-test").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance.as_str()).unwrap()],
    };
    let api_key = "cli-runtime-api-key";
    let principal = Principal {
        id: CanonicalId::new("cli-runtime-client").unwrap(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(api_key.as_bytes()),
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: Default::default(),
        grants: [
            Action::SessionCreate,
            Action::SessionClose,
            Action::RuntimeToolCatalogueRead,
            Action::ServiceInspect,
        ]
        .into_iter()
        .map(|action| ResourceGrant {
            action,
            resource_prefix: resource.clone(),
            data_policy: None,
        })
        .collect(),
    };
    let storage = PersistentEngine::open(&store).unwrap();
    SecurityRepository::new(&storage, instance.clone())
        .initialize(
            SecurityState {
                format_version: SECURITY_FORMAT,
                revision: 1,
                principals: BTreeMap::from([(principal.id.clone(), principal)]),
                roles: BTreeMap::new(),
                identity_bindings: BTreeMap::new(),
                jwt_issuers: BTreeMap::new(),
            },
            1,
            "bootstrap",
            "request-bootstrap",
            "operation-bootstrap",
        )
        .unwrap();
    drop(storage);

    let embedded_list = rrflow(vec![
        "--json".into(),
        "--db".into(),
        store.as_os_str().to_owned(),
        "runtime".into(),
        "--mode".into(),
        "embedded".into(),
        "--root".into(),
        project.as_os_str().to_owned(),
        "list".into(),
    ]);
    let (embedded_list_out, embedded_list_err) = text(&embedded_list);
    assert!(embedded_list.status.success(), "{embedded_list_err}");
    let embedded_catalogue: Value = serde_json::from_str(embedded_list_out).unwrap();

    let embedded_call = rrflow(vec![
        "--json".into(),
        "--db".into(),
        store.as_os_str().to_owned(),
        "runtime".into(),
        "--mode".into(),
        "embedded".into(),
        "--root".into(),
        project.as_os_str().to_owned(),
        "call".into(),
        "--tool".into(),
        "rrflow_service_status".into(),
        "--arguments".into(),
        r#"{"at":500}"#.into(),
    ]);
    let (embedded_call_out, embedded_call_err) = text(&embedded_call);
    assert!(embedded_call.status.success(), "{embedded_call_err}");
    let embedded_envelope: Value = serde_json::from_str(embedded_call_out).unwrap();
    let embedded_status: Value =
        serde_json::from_str(embedded_envelope["content"].as_str().unwrap()).unwrap();

    let token_key = load_or_create_token_key(&store.join("RRD.SERVER.SECRET")).unwrap();
    let engine =
        RrdEngine::open_bound_with_token_key(&binding, instance.clone(), token_key, 2).unwrap();
    let server = RrdHttpServer::bind_project(
        engine,
        binding.authority_binding().unwrap(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
    let address = server.local_addr();
    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let server_thread = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(server.serve_until(async move {
                let _ = receiver.await;
            }))
    });

    let api_key_file = temporary.path().join("cli-api-key");
    std::fs::write(&api_key_file, api_key).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&api_key_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let daemon_arguments = |action: &str| {
        let mut arguments = vec![
            "--json".into(),
            "runtime".into(),
            "--mode".into(),
            "daemon".into(),
            "--url".into(),
            format!("http://{address}").into(),
            "--instance".into(),
            instance.as_str().into(),
            "--principal".into(),
            "cli-runtime-client".into(),
            "--api-key-file".into(),
            api_key_file.as_os_str().to_owned(),
            action.into(),
        ];
        if action == "call" {
            arguments.extend([
                "--tool".into(),
                "rrflow_service_status".into(),
                "--arguments".into(),
                r#"{"at":500}"#.into(),
            ]);
        }
        arguments
    };
    let daemon_list = rrflow(daemon_arguments("list"));
    let (daemon_list_out, daemon_list_err) = text(&daemon_list);
    assert!(daemon_list.status.success(), "{daemon_list_err}");
    let daemon_catalogue: Value = serde_json::from_str(daemon_list_out).unwrap();
    assert_eq!(daemon_catalogue, embedded_catalogue);

    let daemon_call = rrflow(daemon_arguments("call"));
    let (daemon_call_out, daemon_call_err) = text(&daemon_call);
    assert!(daemon_call.status.success(), "{daemon_call_err}");
    let daemon_envelope: Value = serde_json::from_str(daemon_call_out).unwrap();
    let daemon_status: Value =
        serde_json::from_str(daemon_envelope["content"].as_str().unwrap()).unwrap();
    for field in [
        "observed_at",
        "instance_id",
        "security_enforced",
        "endpoint_count",
        "endpoints",
        "executable_mcp_tool_count",
        "executable_mcp_tools",
        "mcp_task_catalogue",
        "product_capabilities",
    ] {
        assert_eq!(
            daemon_status[field], embedded_status[field],
            "field {field}"
        );
    }

    shutdown.send(()).unwrap();
    server_thread.join().unwrap().unwrap();
    let storage = PersistentEngine::open(&store).unwrap();
    let audit = SecurityRepository::new(&storage, instance)
        .audit_since(0, 256)
        .unwrap();
    assert!(audit.records.iter().any(|(_, record)| {
        record.principal_id.as_ref().map(CanonicalId::as_str) == Some("cli-runtime-client")
            && record.action == Action::ServiceInspect
            && record.phase == rrd_contract::AuditPhase::Completed
            && record.decision == rrd_contract::AuditDecision::Allowed
    }));
}

#[test]
fn daemon_runtime_and_offline_recovery_coordinates_are_mutually_exclusive() {
    let daemon = rrflow(vec![
        "--db".into(),
        "/unused-cli-runtime-store".into(),
        "runtime".into(),
        "--mode".into(),
        "daemon".into(),
        "--url".into(),
        "http://127.0.0.1:9".into(),
        "--instance".into(),
        "unused-instance".into(),
        "--principal".into(),
        "unused-principal".into(),
        "--api-key-file".into(),
        "/does-not-exist".into(),
        "list".into(),
    ]);
    let (_, daemon_err) = text(&daemon);
    assert!(!daemon.status.success());
    assert!(daemon_err.contains("daemon runtime mode does not accept --db or --root"));

    let storage = rrflow(vec![
        "--db".into(),
        "/unused-cli-runtime-store".into(),
        "storage".into(),
        "status".into(),
        "--mode".into(),
        "daemon".into(),
    ]);
    let (_, storage_err) = text(&storage);
    assert!(!storage.status.success());
    assert!(storage_err.contains("unexpected argument '--mode'"));
}
