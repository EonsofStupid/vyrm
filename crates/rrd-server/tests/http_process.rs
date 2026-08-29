use rrd_contract::{
    transaction_operation_sha256, CanonicalId, CorrelationId, DeploymentConformanceCorpus,
    TransactionMutation,
};
use rrd_core::{
    RuntimeCommit, RuntimeEventSchema, RuntimeMutation, RuntimeProperties, RuntimePropertySchema,
    RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType,
    RuntimeValue, RuntimeValueType, ScopeId,
};
use rrd_engine::{load_or_create_token_key, InstanceManifest, RrdEngine};
use rrd_security::{
    Action as SecurityAction, AuditDecision, AuditPhase, DataPolicy, JwtIssueRequest, JwtIssuer,
    Principal, PrincipalKind, ResourceGrant, SecurityRepository, SecurityState, SECURITY_FORMAT,
};
use rrd_server::{HttpError, RrdHttpServer, RrdJwtVerificationKey, RRD_MAX_BODY_BYTES};
use rrd_store::Engine;
use rrd_store::PersistentEngine;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const MAX_CONCURRENT_SERVER_FIXTURES: usize = 4;
const INTEGRATION_IO_TIMEOUT: Duration = Duration::from_secs(30);
static ACTIVE_SERVER_FIXTURES: Mutex<usize> = Mutex::new(0);
static SERVER_FIXTURE_AVAILABLE: Condvar = Condvar::new();

struct ServerFixturePermit;

fn acquire_server_fixture() -> ServerFixturePermit {
    let mut active = ACTIVE_SERVER_FIXTURES.lock().unwrap();
    while *active >= MAX_CONCURRENT_SERVER_FIXTURES {
        active = SERVER_FIXTURE_AVAILABLE.wait(active).unwrap();
    }
    *active += 1;
    ServerFixturePermit
}

impl Drop for ServerFixturePermit {
    fn drop(&mut self) {
        let mut active = ACTIVE_SERVER_FIXTURES.lock().unwrap();
        *active -= 1;
        SERVER_FIXTURE_AVAILABLE.notify_one();
    }
}

fn estate_context(at: u64, operation: &str) -> rrd_estate::MutationContext {
    rrd_estate::MutationContext {
        at,
        actor: "rrd-estate".into(),
        request_id: format!("request-{operation}"),
        operation_id: CanonicalId::new(operation).unwrap(),
    }
}

struct RunningServer {
    address: SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<std::result::Result<(), HttpError>>>,
    _fixture: ServerFixturePermit,
}

struct RunningServerProcess {
    address: SocketAddr,
    child: Option<Child>,
    shutdown_request: PathBuf,
    shutdown_complete: PathBuf,
}

impl RunningServerProcess {
    fn stop(mut self) {
        std::fs::write(&self.shutdown_request, b"stop\n").unwrap();
        let deadline = Instant::now() + INTEGRATION_IO_TIMEOUT;
        let status = loop {
            if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "rrd-server child did not stop");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success());
        assert!(self.shutdown_complete.is_file());
        self.child = None;
    }
}

impl Drop for RunningServerProcess {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = std::fs::write(&self.shutdown_request, b"stop\n");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl RunningServer {
    fn stop(mut self) {
        let _ = self.shutdown.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap().unwrap();
        }
    }
}

fn start(root: &Path) -> RunningServer {
    start_configured(root, None)
}

fn start_with_jwt(root: &Path, key: &[u8]) -> RunningServer {
    start_configured(
        root,
        Some(RrdJwtVerificationKey::new(key.to_vec()).unwrap()),
    )
}

fn start_configured(
    root: &Path,
    jwt_verification_key: Option<RrdJwtVerificationKey>,
) -> RunningServer {
    let fixture = acquire_server_fixture();
    let token_key = load_or_create_token_key(&root.join("RRD.SERVER.SECRET")).unwrap();
    let engine =
        RrdEngine::open(root, CanonicalId::new("socket-test").unwrap(), token_key).unwrap();
    let bind = "127.0.0.1:0".parse().unwrap();
    let server = match jwt_verification_key {
        Some(key) => RrdHttpServer::bind_with_jwt(engine, bind, key).unwrap(),
        None => RrdHttpServer::bind(engine, bind).unwrap(),
    };
    let address = server.local_addr();
    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(server.serve_until(async move {
                let _ = receiver.await;
            }))
    });
    RunningServer {
        address,
        shutdown: Some(shutdown),
        thread: Some(thread),
        _fixture: fixture,
    }
}

fn envelope(payload: Value, idempotency_key: Option<&str>, deadline: Option<u64>) -> Value {
    json!({
        "protocol": "rrd",
        "protocol_version": 1,
        "context": {
            "request_id": format!("request-{}", idempotency_key.unwrap_or("read")),
            "operation_id": format!("operation-{}", idempotency_key.unwrap_or("read")),
            "idempotency_key": idempotency_key,
            "deadline_unix_ms": deadline,
        },
        "resource": {
            "segments": [{"kind": "instance", "id": "socket-test"}],
        },
        "payload": payload,
    })
}

fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> (u16, Value) {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(INTEGRATION_IO_TIMEOUT))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    )
    .unwrap();
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").unwrap();
    }
    stream.write_all(b"\r\n").unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let head = String::from_utf8_lossy(&response[..split]);
    let status = head
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let body = serde_json::from_slice(&response[split + 4..]).unwrap();
    (status, body)
}

fn post(
    server: &RunningServer,
    path: &str,
    value: &Value,
    session: Option<(&str, &str)>,
) -> (u16, Value) {
    let body = serde_json::to_vec(value).unwrap();
    let mut headers = vec![("Content-Type", "application/json")];
    let authorization;
    if let Some((session_id, token)) = session {
        authorization = format!("Bearer {token}");
        headers.push(("X-RRD-Session", session_id));
        headers.push(("Authorization", &authorization));
    }
    http(server.address, "POST", path, &headers, &body)
}

fn post_with_api_key(
    server: &RunningServer,
    path: &str,
    value: &Value,
    principal: &str,
    credential: &str,
) -> (u16, Value) {
    let body = serde_json::to_vec(value).unwrap();
    let authorization = format!("ApiKey {credential}");
    http(
        server.address,
        "POST",
        path,
        &[
            ("Content-Type", "application/json"),
            ("X-RRD-Principal", principal),
            ("Authorization", &authorization),
        ],
        &body,
    )
}

fn assert_tree_excludes(root: &Path, secrets: &[&str]) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let file_type = entry.file_type().unwrap();
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                let bytes = std::fs::read(entry.path()).unwrap();
                for secret in secrets {
                    assert!(
                        !bytes
                            .windows(secret.len())
                            .any(|window| window == secret.as_bytes()),
                        "raw credential reached {}",
                        entry.path().display()
                    );
                }
            }
        }
    }
}

fn delete(
    server: &RunningServer,
    path: &str,
    value: &Value,
    session: (&str, &str),
) -> (u16, Value) {
    let body = serde_json::to_vec(value).unwrap();
    let authorization = format!("Bearer {}", session.1);
    http(
        server.address,
        "DELETE",
        path,
        &[
            ("Content-Type", "application/json"),
            ("X-RRD-Session", session.0),
            ("Authorization", &authorization),
        ],
        &body,
    )
}

fn payload(response: &Value) -> &Value {
    &response["outcome"]["payload"]
}

fn start_root() -> (tempfile::TempDir, PathBuf, RunningServer) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("instance");
    let server = start(&root);
    (temporary, root, server)
}

fn seed_query_fixture(root: &Path) {
    let engine = PersistentEngine::open(root).unwrap();
    let mut registry = RuntimeSchemaRegistry::empty(1, "RRD query fixture");
    registry.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema {
            properties: BTreeMap::from([(
                "title".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            )]),
            ..RuntimeRecordSchema::default()
        },
    );
    registry.events.insert(
        RuntimeType::new("observed").unwrap(),
        RuntimeEventSchema {
            subject_required: true,
            subject_types: [RuntimeType::new("document").unwrap()]
                .into_iter()
                .collect(),
            allow_additional_properties: true,
            ..RuntimeEventSchema::default()
        },
    );
    engine
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new("instance:socket-test").unwrap(),
            at: 100,
            actor: "rrd-query-fixture".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema { registry },
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: RuntimeRef::new("document", "alpha").unwrap(),
                        valid_from: 100,
                        valid_to: None,
                        properties: RuntimeProperties::from([(
                            "title".into(),
                            RuntimeValue::String("Alpha".into()),
                        )]),
                    },
                },
            ],
        })
        .unwrap();
}

fn deployment_corpus() -> DeploymentConformanceCorpus {
    let corpus = serde_json::from_str(include_str!(
        "../../../fixtures/rrd-deployment-conformance-v1.json"
    ))
    .unwrap();
    DeploymentConformanceCorpus::validate(&corpus).unwrap();
    corpus
}

fn deployment_mutations() -> Vec<TransactionMutation> {
    let corpus = deployment_corpus();
    let mut mutations = vec![json!({
        "mutation": "put_schema",
        "registry": {
            "revision": 1,
            "migration": "install deployment conformance corpus",
            "records": {
                "document": {
                    "properties": {
                        "body": {"value_type": "string", "required": true}
                    },
                    "allow_additional_properties": false,
                    "unique_properties": []
                }
            },
            "relations": {},
            "events": {}
        }
    })];
    mutations.extend(corpus.documents.iter().map(|document| {
        json!({
            "mutation": "put_record",
            "reference": {"kind": "document", "id": document.id},
            "valid_from": corpus.query.valid_at,
            "properties": {
                "body": {"type": "string", "value": document.text}
            }
        })
    }));
    serde_json::from_value(Value::Array(mutations)).unwrap()
}

#[test]
fn initialized_security_authority_binds_sessions_and_denies_ungranted_routes() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("instance");
    seed_query_fixture(&root);
    let engine = PersistentEngine::open(&root).unwrap();
    let instance = CanonicalId::new("socket-test").unwrap();
    let resource = rrd_contract::ResourcePath {
        segments: vec![rrd_contract::ResourceId::new(
            rrd_contract::ResourceKind::Instance,
            "socket-test",
        )
        .unwrap()],
    };
    let principal = Principal {
        id: CanonicalId::new("connectome-local").unwrap(),
        kind: PrincipalKind::User,
        credential_sha256: rrd_core::digest::sha256_hex(b"local-api-key"),
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: Default::default(),
        grants: vec![
            ResourceGrant {
                action: SecurityAction::SessionCreate,
                resource_prefix: resource.clone(),
                data_policy: None,
            },
            ResourceGrant {
                action: SecurityAction::QueryExecute,
                resource_prefix: resource,
                data_policy: None,
            },
            ResourceGrant {
                action: SecurityAction::AuditRead,
                resource_prefix: rrd_contract::ResourcePath {
                    segments: vec![rrd_contract::ResourceId::new(
                        rrd_contract::ResourceKind::Instance,
                        "socket-test",
                    )
                    .unwrap()],
                },
                data_policy: None,
            },
            ResourceGrant {
                action: SecurityAction::AuditExport,
                resource_prefix: rrd_contract::ResourcePath {
                    segments: vec![rrd_contract::ResourceId::new(
                        rrd_contract::ResourceKind::Instance,
                        "socket-test",
                    )
                    .unwrap()],
                },
                data_policy: None,
            },
        ],
    };
    SecurityRepository::new(&engine, instance)
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
    drop(engine);

    let server = start(&root);
    let (status, _) = http(server.address, "GET", "/v1/health/live", &[], &[]);
    assert_eq!(status, 200);
    let (status, _) = http(server.address, "GET", "/v1/not-a-route", &[], &[]);
    assert_eq!(status, 404);
    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2
            }
        }),
        Some("secured-session"),
        None,
    );
    let (status, _) = post(&server, "/v1/sessions", &create, None);
    assert_eq!(status, 401);
    let (status, _) = post_with_api_key(
        &server,
        "/v1/sessions",
        &create,
        "connectome-local",
        "wrong",
    );
    assert_eq!(status, 401);
    let (status, created) = post_with_api_key(
        &server,
        "/v1/sessions",
        &create,
        "connectome-local",
        "local-api-key",
    );
    assert_eq!(status, 200, "{created}");
    let session = payload(&created)["session_id"].as_str().unwrap();
    let token = payload(&created)["token"].as_str().unwrap();

    let query = envelope(
        json!({
            "scope": "instance:socket-test",
            "query": "FROM record:document AT VALID 100 KNOWN HEAD PROJECT id, title EXPLAIN CONTRACT",
            "parameters": {},
            "budget": {
                "max_scanned_changes": 100,
                "max_rows": 10,
                "max_output_bytes": 4096,
                "max_batch_rows": 10
            }
        }),
        Some("prepare-data"),
        None,
    );
    let (status, response) = post(&server, "/v1/query", &query, Some((session, token)));
    assert_eq!(status, 200, "{response}");

    let backup = envelope(
        json!({"label": "denied", "created_at_unix_ms": 500}),
        Some("denied-backup"),
        None,
    );
    let (status, denied) = post(&server, "/v1/backups", &backup, Some((session, token)));
    assert_eq!(status, 403, "{denied}");
    assert_eq!(denied["outcome"]["error"]["code"], "permission_denied");
    let (status, unauthenticated) = post(&server, "/v1/backups", &backup, None);
    assert_eq!(status, 401, "{unauthenticated}");

    let mut invalid_query = query.clone();
    invalid_query["payload"]["query"] = json!("NOT RRFLOWQL");
    let (status, failed) = post(&server, "/v1/query", &invalid_query, Some((session, token)));
    assert_eq!(status, 400, "{failed}");

    let oversized = "x".repeat(rrd_server::RRD_MAX_BODY_BYTES + 1);
    let (status, exhausted) = http(
        server.address,
        "POST",
        "/v1/query",
        &[("Content-Type", "application/json")],
        oversized.as_bytes(),
    );
    assert_eq!(status, 429, "{exhausted}");

    let audit = envelope(json!({"after_sequence": 0, "limit": 64}), None, None);
    let (status, audited) = post(&server, "/v1/audit/read", &audit, Some((session, token)));
    assert_eq!(status, 200, "{audited}");
    let records = payload(&audited)["records"].as_array().unwrap();
    assert!(records.len() >= 15, "{audited}");
    assert!(records
        .iter()
        .any(|record| record["action"] == "security_admin" && record["decision"] == "allowed"));
    assert!(records
        .iter()
        .any(|record| record["action"] == "service_inspect" && record["decision"] == "allowed"));
    assert!(records
        .iter()
        .any(|record| record["action"] == "unknown_request" && record["decision"] == "failed"));
    assert_eq!(
        records
            .iter()
            .filter(|record| {
                record["action"] == "session_create" && record["decision"] == "denied"
            })
            .count(),
        2
    );
    assert!(records
        .iter()
        .any(|record| { record["action"] == "session_create" && record["phase"] == "authorized" }));
    assert!(records.iter().any(|record| {
        record["action"] == "query_execute"
            && record["phase"] == "completed"
            && record["decision"] == "allowed"
    }));
    assert!(records.iter().any(|record| {
        record["action"] == "query_execute"
            && record["phase"] == "completed"
            && record["decision"] == "failed"
    }));
    assert_eq!(
        records
            .iter()
            .filter(|record| {
                record["action"] == "backup_create" && record["decision"] == "denied"
            })
            .count(),
        2
    );
    assert!(records
        .iter()
        .any(|record| { record["action"] == "audit_read" && record["phase"] == "authorized" }));
    assert!(records.iter().any(|record| {
        record["action"] == "query_execute"
            && record["phase"] == "completed"
            && record["status_code"] == 429
    }));
    assert!(records.windows(2).all(|pair| {
        pair[1]["previous_audit_sha256"].as_str() == pair[0]["audit_sha256"].as_str()
    }));
    let encoded = serde_json::to_string(records).unwrap();
    assert!(!encoded.contains("local-api-key"));
    assert!(!encoded.contains("wrong"));

    let export = envelope(json!({"after_sequence": 0, "limit": 64}), None, None);
    let (status, exported) = post(&server, "/v1/audit/export", &export, Some((session, token)));
    assert_eq!(status, 200, "{exported}");
    let exported = payload(&exported);
    let json_lines = exported["json_lines"].as_str().unwrap();
    assert_eq!(
        exported["content_sha256"],
        rrd_core::digest::sha256_hex(json_lines.as_bytes())
    );
    assert_eq!(
        exported["record_count"].as_u64().unwrap() as usize,
        json_lines.lines().count()
    );
    assert!(!json_lines.contains("local-api-key"));
    assert!(!json_lines.contains(token));
    server.stop();
    let reopened = PersistentEngine::open(&root).unwrap();
    assert_eq!(reopened.runtime_cursor().unwrap(), 2);
    let journal = reopened.control_journal_since(0, 64).unwrap();
    let encoded = serde_json::to_string(&journal).unwrap();
    assert!(!encoded.contains("local-api-key"));
    assert!(!encoded.contains("ApiKey"));
    drop(reopened);
    assert_tree_excludes(&root, &["local-api-key", token]);
}

#[test]
fn jwt_session_exchange_reopens_and_credential_rotation_revokes_token_and_lease() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("jwt-instance");
    seed_query_fixture(&root);
    let signing_key = b"rrd-jwt-test-signing-key-material";
    let issued_at = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let principal_id = CanonicalId::new("jwt-client").unwrap();
    let issuer_id = CanonicalId::new("rrd-test-issuer").unwrap();
    let resource = rrd_contract::ResourcePath {
        segments: vec![rrd_contract::ResourceId::new(
            rrd_contract::ResourceKind::Instance,
            "socket-test",
        )
        .unwrap()],
    };
    let principal = Principal {
        id: principal_id.clone(),
        kind: PrincipalKind::Service,
        credential_sha256: rrd_core::digest::sha256_hex(b"jwt-api-key"),
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: Default::default(),
        grants: vec![
            ResourceGrant {
                action: SecurityAction::SessionCreate,
                resource_prefix: resource.clone(),
                data_policy: None,
            },
            ResourceGrant {
                action: SecurityAction::QueryExecute,
                resource_prefix: resource.clone(),
                data_policy: Some(DataPolicy {
                    tenant: None,
                    rows: Vec::new(),
                    allowed_fields: Some(BTreeSet::from(["title".into()])),
                }),
            },
        ],
    };
    let issuer = JwtIssuer {
        id: issuer_id.clone(),
        issuer: "https://rrd.test".into(),
        audience: "rrflow-client".into(),
        key_id: CanonicalId::new("jwt-key-1").unwrap(),
        signing_key_sha256: rrd_core::digest::sha256_hex(signing_key),
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
    };
    let storage = PersistentEngine::open(&root).unwrap();
    SecurityRepository::new(&storage, CanonicalId::new("socket-test").unwrap())
        .initialize(
            SecurityState {
                format_version: SECURITY_FORMAT,
                revision: 1,
                principals: BTreeMap::from([(principal_id.clone(), principal)]),
                roles: BTreeMap::new(),
                identity_bindings: BTreeMap::new(),
                jwt_issuers: BTreeMap::from([(issuer_id.clone(), issuer)]),
            },
            1,
            "bootstrap",
            "request-jwt-bootstrap",
            "operation-jwt-bootstrap",
        )
        .unwrap();
    drop(storage);
    let token_key = load_or_create_token_key(&root.join("RRD.SERVER.SECRET")).unwrap();
    let engine =
        RrdEngine::open(&root, CanonicalId::new("socket-test").unwrap(), token_key).unwrap();
    let jwt = engine
        .issue_principal_jwt(
            &principal_id,
            b"jwt-api-key",
            signing_key,
            &JwtIssueRequest {
                issuer_id,
                token_id: CanonicalId::new("jwt-exchange-1").unwrap(),
                issued_at_unix_ms: issued_at,
                not_before_unix_ms: issued_at,
                expires_at_unix_ms: issued_at + 60_000,
            },
        )
        .unwrap();
    drop(engine);

    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 120_000,
                "max_open_transactions": 2
            }
        }),
        Some("jwt-session-create"),
        None,
    );
    let create_body = serde_json::to_vec(&create).unwrap();
    let authorization = format!("Bearer {}", jwt.token);
    let server = start_with_jwt(&root, signing_key);
    let (status, capabilities) = http(server.address, "GET", "/v1/capabilities", &[], &[]);
    assert_eq!(status, 200);
    let advertised = payload(&capabilities)["capabilities"].as_array().unwrap();
    assert!(advertised.iter().any(|entry| {
        entry["name"] == "jwt-session-credentials" && entry["status"] == "available"
    }));
    let (status, created) = http(
        server.address,
        "POST",
        "/v1/sessions",
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &authorization),
        ],
        &create_body,
    );
    assert_eq!(status, 200, "{created}");
    let session_id = payload(&created)["session_id"].as_str().unwrap().to_owned();
    let session_token = payload(&created)["token"].as_str().unwrap().to_owned();
    let query = envelope(
        json!({
            "scope": "instance:socket-test",
            "query": "FROM record:document AT VALID 100 KNOWN HEAD PROJECT title",
            "parameters": {},
            "budget": {
                "max_scanned_changes": 100,
                "max_rows": 10,
                "max_output_bytes": 4096,
                "max_batch_rows": 10
            }
        }),
        None,
        None,
    );
    let (status, queried) = post(
        &server,
        "/v1/query",
        &query,
        Some((&session_id, &session_token)),
    );
    assert_eq!(status, 200, "{queried}");
    let mut forbidden_field_query = query.clone();
    forbidden_field_query["payload"]["query"] =
        json!("FROM record:document AT VALID 100 KNOWN HEAD PROJECT secret");
    let (status, denied) = post(
        &server,
        "/v1/query",
        &forbidden_field_query,
        Some((&session_id, &session_token)),
    );
    assert_eq!(status, 403, "{denied}");
    assert_eq!(denied["outcome"]["error"]["code"], "permission_denied");
    server.stop();

    let server = start_with_jwt(&root, signing_key);
    let (status, replay) = http(
        server.address,
        "POST",
        "/v1/sessions",
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &authorization),
        ],
        &create_body,
    );
    assert_eq!(status, 200, "{replay}");
    assert_eq!(payload(&replay)["session_id"], session_id);
    server.stop();

    let storage = PersistentEngine::open(&root).unwrap();
    let repository = SecurityRepository::new(&storage, CanonicalId::new("socket-test").unwrap());
    let audit = repository.audit_since(0, 64).unwrap();
    assert!(audit.records.iter().any(|(_, record)| {
        record.action == SecurityAction::QueryExecute
            && record.phase == AuditPhase::Completed
            && record.decision == AuditDecision::Denied
            && record.status_code == 403
    }));
    let mut rotated = repository.load().unwrap().unwrap();
    rotated.revision = 2;
    let principal = rotated.principals.get_mut(&principal_id).unwrap();
    principal.credential_revision = 2;
    principal.credential_sha256 = rrd_core::digest::sha256_hex(b"rotated-jwt-api-key");
    repository
        .replace(
            1,
            rotated,
            issued_at + 1,
            "security-admin",
            "request-jwt-rotate",
            "operation-jwt-rotate",
        )
        .unwrap();
    drop(storage);

    let server = start_with_jwt(&root, signing_key);
    let (status, _) = http(
        server.address,
        "POST",
        "/v1/sessions",
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &authorization),
        ],
        &create_body,
    );
    assert_eq!(status, 401);
    let (status, _) = post(
        &server,
        "/v1/query",
        &query,
        Some((&session_id, &session_token)),
    );
    assert_eq!(status, 401);
    server.stop();
    assert_tree_excludes(
        &root,
        &[&jwt.token, std::str::from_utf8(signing_key).unwrap()],
    );
}

#[test]
fn authenticated_query_exposes_exact_rrflowql_rrd_query_executor_contract() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("instance");
    seed_query_fixture(&root);
    let server = start(&root);
    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2
            }
        }),
        Some("query-session"),
        None,
    );
    let (status, created) = post(&server, "/v1/sessions", &create, None);
    assert_eq!(status, 200);
    let session_id = payload(&created)["session_id"].as_str().unwrap();
    let token = payload(&created)["token"].as_str().unwrap();
    let query = envelope(
        json!({
            "scope": "instance:socket-test",
            "query": "FROM record:document AT VALID 100 KNOWN HEAD PROJECT id, title EXPLAIN CONTRACT",
            "parameters": {},
            "budget": {
                "max_scanned_changes": 100,
                "max_rows": 10,
                "max_output_bytes": 16384,
                "max_batch_rows": 10
            }
        }),
        None,
        None,
    );
    let (status, denied) = post(&server, "/v1/query", &query, None);
    assert_eq!(status, 401);
    assert_eq!(denied["outcome"]["error"]["code"], "unauthenticated");

    let (status, result) = post(&server, "/v1/query", &query, Some((session_id, token)));
    assert_eq!(status, 200, "{result}");
    let result = payload(&result);
    assert_eq!(result["scope"], "instance:socket-test");
    assert_eq!(result["schema_revision"], 1);
    assert_eq!(result["plan"]["exact"], true);
    assert_eq!(result["plan"]["candidates"][0]["selected"], true);
    assert_eq!(result["execution"]["returned_rows"], 1);
    assert_eq!(result["rows"][0]["identity"], "record:document:alpha");
    assert_eq!(result["rows"][0]["values"]["title"]["type"], "string");
    assert_eq!(result["rows"][0]["values"]["title"]["value"], "Alpha");

    let mut wrong_scope = query;
    wrong_scope["payload"]["scope"] = json!("instance:other");
    let (status, wrong) = post(
        &server,
        "/v1/query",
        &wrong_scope,
        Some((session_id, token)),
    );
    assert_eq!(status, 400);
    assert_eq!(wrong["outcome"]["error"]["code"], "invalid_argument");
}

#[test]
fn bounded_changefeed_follow_wakes_on_a_commit_and_times_out_at_the_same_cursor() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("instance");
    seed_query_fixture(&root);
    let server = start(&root);
    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2
            }
        }),
        Some("follow-session"),
        None,
    );
    let (status, created) = post(&server, "/v1/sessions", &create, None);
    assert_eq!(status, 200, "{created}");
    let session_id = payload(&created)["session_id"].as_str().unwrap().to_owned();
    let token = payload(&created)["token"].as_str().unwrap().to_owned();
    let follow = envelope(
        json!({
            "read": {
                "scope": "instance:socket-test",
                "after_cursor": 2,
                "limit": 8
            },
            "wait_timeout_ms": 5_000
        }),
        None,
        None,
    );
    let address = server.address;
    let follow_body = serde_json::to_vec(&follow).unwrap();
    let follow_session = session_id.clone();
    let follow_token = token.clone();
    let waiting = std::thread::spawn(move || {
        let authorization = format!("Bearer {follow_token}");
        http(
            address,
            "POST",
            "/v1/changes/follow",
            &[
                ("Content-Type", "application/json"),
                ("X-RRD-Session", &follow_session),
                ("Authorization", &authorization),
            ],
            &follow_body,
        )
    });
    std::thread::sleep(Duration::from_millis(50));

    let begin = envelope(
        json!({"scope": "data", "timeout_ms": 10_000}),
        Some("follow-begin"),
        None,
    );
    let (status, began) = post(
        &server,
        "/v1/transactions",
        &begin,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{began}");
    let transaction = payload(&began)["transaction_id"].as_str().unwrap();
    let mutations = json!([{
        "mutation": "append_event",
        "kind": "observed",
        "subject": {"kind": "document", "id": "alpha"},
        "properties": {"source": {"type": "string", "value": "follow-test"}}
    }]);
    let typed: Vec<TransactionMutation> = serde_json::from_value(mutations.clone()).unwrap();
    let commit = envelope(
        json!({
            "operation_sha256": transaction_operation_sha256(&typed),
            "mutations": mutations
        }),
        Some("follow-commit"),
        Some(u64::MAX),
    );
    let (status, committed) = post(
        &server,
        &format!("/v1/transactions/{transaction}/commit"),
        &commit,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{committed}");
    assert_eq!(payload(&committed)["last_runtime_cursor"], 3);

    let (status, followed) = waiting.join().unwrap();
    assert_eq!(status, 200, "{followed}");
    assert_eq!(payload(&followed)["timed_out"], false);
    assert_eq!(payload(&followed)["page"]["through_cursor"], 3);
    assert_eq!(payload(&followed)["page"]["changes"][0]["cursor"], 3);
    assert_eq!(
        payload(&followed)["page"]["changes"][0]["mutation"]["mutation"]["mutation"],
        "append_event"
    );

    let timeout = envelope(
        json!({
            "read": {
                "scope": "instance:socket-test",
                "after_cursor": 3,
                "limit": 8
            },
            "wait_timeout_ms": 50
        }),
        None,
        None,
    );
    let (status, timed_out) = post(
        &server,
        "/v1/changes/follow",
        &timeout,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{timed_out}");
    assert_eq!(payload(&timed_out)["timed_out"], true);
    assert_eq!(payload(&timed_out)["page"]["through_cursor"], 3);
    assert!(payload(&timed_out)["page"]["changes"]
        .as_array()
        .unwrap()
        .is_empty());

    let live = envelope(
        json!({
            "scope": "instance:socket-test",
            "query": "FROM record:document AT VALID 100 KNOWN HEAD PROJECT title",
            "parameters": {},
            "after_cursor": 3,
            "budget": {
                "max_scanned_changes": 100,
                "max_rows": 10,
                "max_output_bytes": 4096,
                "max_batch_rows": 10
            },
            "max_delta_rows": 10,
            "wait_timeout_ms": 5_000
        }),
        None,
        None,
    );
    let address = server.address;
    let live_body = serde_json::to_vec(&live).unwrap();
    let live_session = session_id.clone();
    let live_token = token.clone();
    let waiting = std::thread::spawn(move || {
        let authorization = format!("Bearer {live_token}");
        http(
            address,
            "POST",
            "/v1/query/live/poll",
            &[
                ("Content-Type", "application/json"),
                ("X-RRD-Session", &live_session),
                ("Authorization", &authorization),
            ],
            &live_body,
        )
    });
    std::thread::sleep(Duration::from_millis(50));
    let begin = envelope(
        json!({"scope": "data", "timeout_ms": 10_000}),
        Some("live-follow-begin"),
        None,
    );
    let (status, began) = post(
        &server,
        "/v1/transactions",
        &begin,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{began}");
    let transaction = payload(&began)["transaction_id"].as_str().unwrap();
    let mutations = json!([{
        "mutation": "put_record",
        "reference": {"kind": "document", "id": "alpha"},
        "valid_from": 100,
        "properties": {"title": {"type": "string", "value": "Alpha updated"}}
    }]);
    let typed: Vec<TransactionMutation> = serde_json::from_value(mutations.clone()).unwrap();
    let commit = envelope(
        json!({
            "operation_sha256": transaction_operation_sha256(&typed),
            "mutations": mutations
        }),
        Some("live-follow-commit"),
        Some(u64::MAX),
    );
    let (status, committed) = post(
        &server,
        &format!("/v1/transactions/{transaction}/commit"),
        &commit,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{committed}");
    assert_eq!(payload(&committed)["last_runtime_cursor"], 4);
    let (status, followed) = waiting.join().unwrap();
    assert_eq!(status, 200, "{followed}");
    assert_eq!(payload(&followed)["timed_out"], false);
    assert_eq!(payload(&followed)["through_cursor"], 4);
    assert_eq!(payload(&followed)["updated"].as_array().unwrap().len(), 1);
    assert_eq!(
        payload(&followed)["updated"][0]["after"]["values"]["title"]["value"],
        "Alpha updated"
    );

    let live_timeout = envelope(
        json!({
            "scope": "instance:socket-test",
            "query": "FROM record:document AT VALID 100 KNOWN HEAD PROJECT title",
            "parameters": {},
            "after_cursor": 4,
            "budget": {
                "max_scanned_changes": 100,
                "max_rows": 10,
                "max_output_bytes": 4096,
                "max_batch_rows": 10
            },
            "max_delta_rows": 10,
            "wait_timeout_ms": 50
        }),
        None,
        None,
    );
    let (status, timed_out) = post(
        &server,
        "/v1/query/live/poll",
        &live_timeout,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{timed_out}");
    assert_eq!(payload(&timed_out)["timed_out"], true);
    assert_eq!(payload(&timed_out)["through_cursor"], 4);
    assert!(payload(&timed_out)["waited_ms"].as_u64().unwrap() >= 50);
}

#[test]
fn managed_backup_and_restore_are_authenticated_replay_safe_and_path_closed() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("instance");
    seed_query_fixture(&root);
    let server = start(&root);
    let create_session = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2
            }
        }),
        Some("backup-session"),
        None,
    );
    let (status, created) = post(&server, "/v1/sessions", &create_session, None);
    assert_eq!(status, 200, "{created}");
    let session_id = payload(&created)["session_id"].as_str().unwrap().to_owned();
    let token = payload(&created)["token"].as_str().unwrap().to_owned();
    let backup = envelope(
        json!({"label": "before-upgrade", "created_at_unix_ms": 500}),
        Some("backup-create"),
        None,
    );
    let (status, denied) = post(&server, "/v1/backups", &backup, None);
    assert_eq!(status, 401, "{denied}");
    let (status, backed_up) = post(&server, "/v1/backups", &backup, Some((&session_id, &token)));
    assert_eq!(status, 200, "{backed_up}");
    assert_eq!(payload(&backed_up)["idempotent_replay"], false);
    assert_eq!(
        payload(&backed_up)["backup"]["archive"]["runtime_cursor"],
        2
    );
    assert_eq!(payload(&backed_up)["backup"]["object_payloads"], "included");
    assert_eq!(payload(&backed_up)["backup"]["catalogues"], "included");
    assert_eq!(payload(&backed_up)["backup"]["application_complete"], true);
    let backup_id = payload(&backed_up)["backup"]["backup_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, backup_replay) =
        post(&server, "/v1/backups", &backup, Some((&session_id, &token)));
    assert_eq!(status, 200, "{backup_replay}");
    assert_eq!(payload(&backup_replay)["idempotent_replay"], true);
    assert_eq!(
        payload(&backup_replay)["backup"]["backup_sha256"],
        backup_id
    );
    let mut backup_collision = backup.clone();
    backup_collision["payload"]["label"] = json!("different");
    let (status, collision) = post(
        &server,
        "/v1/backups",
        &backup_collision,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 409, "{collision}");

    let list = envelope(json!({"verify_archives": true}), None, None);
    let (status, listed) = post(
        &server,
        "/v1/backups/list",
        &list,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{listed}");
    assert_eq!(payload(&listed)["archives_verified"], true);
    assert_eq!(payload(&listed)["revision"], 1);
    assert_eq!(payload(&listed)["backups"].as_array().unwrap().len(), 1);
    assert_eq!(payload(&listed)["backups"][0]["backup_sha256"], backup_id);

    let restore = envelope(
        json!({
            "backup_sha256": backup_id,
            "restore_id": "restore-a",
            "restored_at_unix_ms": 600
        }),
        Some("restore-create"),
        None,
    );
    let (status, restored) = post(
        &server,
        "/v1/restores",
        &restore,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{restored}");
    assert_eq!(payload(&restored)["restore_id"], "restore-a");
    assert_eq!(payload(&restored)["reopened"], true);
    assert_eq!(payload(&restored)["idempotent_replay"], false);
    let (status, restore_replay) = post(
        &server,
        "/v1/restores",
        &restore,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{restore_replay}");
    assert_eq!(payload(&restore_replay)["idempotent_replay"], true);
    server.stop();

    let restored_root = temporary
        .path()
        .join("rrd-service/socket-test/restores/restore-a");
    assert!(restored_root.join("CURRENT").is_file());
    let restored_engine = PersistentEngine::open(&restored_root).unwrap();
    assert_eq!(restored_engine.sequence().unwrap(), 0);
    assert_eq!(restored_engine.runtime_cursor().unwrap(), 2);
    drop(restored_engine);

    let server = start(&root);
    let (status, restart_backup) =
        post(&server, "/v1/backups", &backup, Some((&session_id, &token)));
    assert_eq!(status, 200, "{restart_backup}");
    assert_eq!(payload(&restart_backup)["idempotent_replay"], true);
    let (status, restart_restore) = post(
        &server,
        "/v1/restores",
        &restore,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{restart_restore}");
    assert_eq!(payload(&restart_restore)["idempotent_replay"], true);
}

#[test]
fn data_transaction_atomically_commits_every_public_model_and_replays_after_restart() {
    let (_temporary, root, server) = start_root();
    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2
            }
        }),
        Some("data-session"),
        None,
    );
    let (status, created) = post(&server, "/v1/sessions", &create, None);
    assert_eq!(status, 200, "{created}");
    let session_id = payload(&created)["session_id"].as_str().unwrap().to_owned();
    let token = payload(&created)["token"].as_str().unwrap().to_owned();
    let ensure_collection = envelope(
        json!({
            "scope": "instance:socket-test",
            "collection_id": "documents",
            "vectors": [{
                "name": "title",
                "field": "title_embedding",
                "kind": "dense",
                "dimensions": 2,
                "metric": "cosine",
                "memory_tier": "cached"
            }]
        }),
        Some("ensure-documents-collection"),
        None,
    );
    let (status, denied) = post(
        &server,
        "/v1/vector/collections/ensure",
        &ensure_collection,
        None,
    );
    assert_eq!(status, 401, "{denied}");
    let (status, ensured) = post(
        &server,
        "/v1/vector/collections/ensure",
        &ensure_collection,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{ensured}");
    assert_eq!(
        payload(&ensured)["collection"]["collection_id"],
        "documents"
    );
    assert_eq!(payload(&ensured)["collection"]["generation"], 1);
    assert_eq!(payload(&ensured)["idempotent_replay"], false);
    // Catalogue configuration is part of the transaction read stamp. Install
    // it before opening the transaction so the commit is bound to the exact
    // vector contract it validates against.
    let begin = envelope(
        json!({"scope": "data", "timeout_ms": 60_000}),
        Some("begin-data"),
        None,
    );
    let (status, began) = post(
        &server,
        "/v1/transactions",
        &begin,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{began}");
    let transaction_id = payload(&began)["transaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let zero_digest = "0".repeat(64);
    let mutations = json!([
        {
            "mutation": "put_schema",
            "registry": {
                "revision": 1,
                "migration": "bootstrap all public data models",
                "records": {
                    "document": {
                        "properties": {"title": {"value_type": "string", "required": true}},
                        "allow_additional_properties": false,
                        "unique_properties": []
                    },
                    "series": {
                        "properties": {},
                        "allow_additional_properties": true,
                        "unique_properties": []
                    }
                },
                "relations": {
                    "linked": {
                        "from": ["document"],
                        "to": ["document"],
                        "properties": {},
                        "allow_additional_properties": true,
                        "unique_pair": true
                    }
                },
                "events": {
                    "observed": {
                        "subject_required": true,
                        "subject_types": ["document"],
                        "properties": {},
                        "allow_additional_properties": true
                    }
                }
            }
        },
        {
            "mutation": "assert_claim",
            "subject": "alpha",
            "predicate": "status",
            "object": "indexed",
            "valid_from": 100,
            "tx_time": 100,
            "producer": "socket-test",
            "confidence": 1.0
        },
        {
            "mutation": "put_record",
            "reference": {"kind": "document", "id": "alpha"},
            "valid_from": 100,
            "properties": {"title": {"type": "string", "value": "Alpha"}}
        },
        {
            "mutation": "put_record",
            "reference": {"kind": "document", "id": "beta"},
            "valid_from": 100,
            "properties": {"title": {"type": "string", "value": "Beta"}}
        },
        {
            "mutation": "put_record",
            "reference": {"kind": "series", "id": "latency"},
            "valid_from": 100,
            "properties": {}
        },
        {
            "mutation": "put_relation",
            "reference": {"kind": "linked", "id": "alpha-beta"},
            "from": {"kind": "document", "id": "alpha"},
            "to": {"kind": "document", "id": "beta"},
            "valid_from": 100,
            "properties": {"reason": {"type": "string", "value": "test"}}
        },
        {
            "mutation": "append_event",
            "kind": "observed",
            "subject": {"kind": "document", "id": "alpha"},
            "properties": {"source": {"type": "string", "value": "socket"}}
        },
        {
            "mutation": "put_vector",
            "reference": {"kind": "embedding", "id": "alpha-title"},
            "subject": {"kind": "document", "id": "alpha"},
            "collection_id": "documents",
            "vector_name": "title",
            "field": "title_embedding",
            "valid_from": 100,
            "value": {"kind": "dense", "values": [0.6, 0.8]},
            "properties": {"tenant": {"type": "string", "value": "alpha"}}
        },
        {
            "mutation": "append_series_sample",
            "reference": {"kind": "sample", "id": "latency-100"},
            "series": {"kind": "series", "id": "latency"},
            "observed_at": 100,
            "value": {"type": "unsigned", "value": 17},
            "properties": {}
        },
        {
            "mutation": "put_geo",
            "reference": {"kind": "location", "id": "alpha-office"},
            "subject": {"kind": "document", "id": "alpha"},
            "field": "office",
            "valid_from": 100,
            "value": {"kind": "point", "point": {"longitude": -73.0, "latitude": 40.0}},
            "properties": {}
        },
        {
            "mutation": "publish_object_reference",
            "reference": {"kind": "object", "id": "alpha-body"},
            "subject": {"kind": "document", "id": "alpha"},
            "sha256": zero_digest,
            "length": 0,
            "media_type": "text/plain",
            "receipt": {
                "backend": "fixture-verified",
                "key": format!("objects/sha256/00/{}", "0".repeat(64))
            },
            "properties": {}
        }
    ]);
    let typed: Vec<TransactionMutation> = serde_json::from_value(mutations.clone()).unwrap();
    let preview = envelope(
        json!({
            "mutations": mutations.clone(),
            "valid_at": u64::MAX,
            "max_scanned_changes": 100
        }),
        Some("prepare-data"),
        None,
    );
    let preview_path = format!("/v1/transactions/{transaction_id}/preview");
    let (status, previewed) = post(
        &server,
        &preview_path,
        &preview,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{previewed}");
    assert_eq!(payload(&previewed)["read_cursor"], 0);
    assert_eq!(payload(&previewed)["idempotent_replay"], false);
    assert_eq!(payload(&previewed)["prospective"]["known_at_cursor"], 11);
    assert_eq!(payload(&previewed)["prospective"]["schema_revision"], 1);
    assert_eq!(
        payload(&previewed)["prospective"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
    let prospective = payload(&previewed)["prospective"].clone();
    let (status, ready) = http(server.address, "GET", "/v1/health/ready", &[], &[]);
    assert_eq!(status, 200, "{ready}");
    assert_eq!(payload(&ready)["runtime_cursor"], 0);

    let mut substituted_preview = preview.clone();
    substituted_preview["payload"]["mutations"][2]["properties"]["title"]["value"] =
        json!("Substituted");
    let (status, conflict) = post(
        &server,
        &preview_path,
        &substituted_preview,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 409, "{conflict}");
    server.stop();

    let server = start(&root);
    let (status, replayed_preview) = post(
        &server,
        &preview_path,
        &preview,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{replayed_preview}");
    assert_eq!(payload(&replayed_preview)["idempotent_replay"], true);
    assert_eq!(payload(&replayed_preview)["prospective"], prospective);
    let (status, ready) = http(server.address, "GET", "/v1/health/ready", &[], &[]);
    assert_eq!(status, 200, "{ready}");
    assert_eq!(payload(&ready)["runtime_cursor"], 0);

    let operation_sha256 = transaction_operation_sha256(&typed);
    let commit = envelope(
        json!({
            "operation_sha256": operation_sha256,
            "mutations": mutations
        }),
        Some("commit-data"),
        Some(u64::MAX),
    );
    let path = format!("/v1/transactions/{transaction_id}/commit");
    let (status, committed) = post(&server, &path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200, "{committed}");
    let receipt = payload(&committed);
    assert_eq!(receipt["mutation_count"], 11);
    assert_eq!(receipt["first_runtime_cursor"], 1);
    assert_eq!(receipt["last_runtime_cursor"], 11);
    assert_eq!(receipt["claim_mutation_count"], 1);
    assert_eq!(receipt["first_claim_sequence"], 1);
    assert_eq!(receipt["last_claim_sequence"], 1);
    assert_eq!(receipt["idempotent_replay"], false);
    assert_eq!(receipt["runtime_commit_sha256"].as_str().unwrap().len(), 64);
    let (status, replayed) = post(
        &server,
        "/v1/vector/collections/ensure",
        &ensure_collection,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{replayed}");
    assert_eq!(payload(&replayed)["idempotent_replay"], true);
    let collision = envelope(
        json!({
            "scope": "instance:socket-test",
            "collection_id": "documents",
            "vectors": [{
                "name": "title",
                "field": "title_embedding",
                "kind": "dense",
                "dimensions": 3,
                "metric": "cosine",
                "memory_tier": "cached"
            }]
        }),
        Some("ensure-documents-collection"),
        None,
    );
    let (status, collision) = post(
        &server,
        "/v1/vector/collections/ensure",
        &collision,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 409, "{collision}");
    let list_collections = envelope(json!({"scope": "instance:socket-test"}), None, None);
    let (status, listed) = post(
        &server,
        "/v1/vector/collections/list",
        &list_collections,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{listed}");
    assert_eq!(payload(&listed)["revision"], 1);
    assert_eq!(
        payload(&listed)["collections"][0]["collection_id"],
        "documents"
    );
    let vector_search = envelope(
        json!({
            "scope": "instance:socket-test",
            "valid_at": 100,
            "collection_id": "documents",
            "vector_name": "title",
            "query": {"kind": "dense", "values": [0.6, 0.8]},
            "filter": {
                "kind": "condition",
                "condition": {
                    "property": "tenant",
                    "operator": {"operator": "equals", "value": {"type": "string", "value": "alpha"}}
                }
            },
            "top_k": 1,
            "max_scanned_changes": 100
        }),
        None,
        None,
    );
    let (status, denied) = post(&server, "/v1/vector/search", &vector_search, None);
    assert_eq!(status, 401, "{denied}");
    let (status, searched) = post(
        &server,
        "/v1/vector/search",
        &vector_search,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{searched}");
    let search = payload(&searched);
    assert_eq!(search["collection_id"], "documents");
    assert_eq!(search["vector_name"], "title");
    assert_eq!(search["known_at_cursor"], 11);
    assert_eq!(search["access_path"], "exact_scan");
    assert_eq!(search["exact"], true);
    assert_eq!(search["hits"][0]["reference"]["id"], "alpha-title");
    assert_eq!(search["hits"][0]["subject"]["id"], "alpha");
    assert_eq!(search["hits"][0]["source_cursor"], 8);
    assert!((search["hits"][0]["score"].as_f64().unwrap() - 1.0).abs() < 1e-9);
    let filtered_out = envelope(
        json!({
            "scope": "instance:socket-test",
            "valid_at": 100,
            "collection_id": "documents",
            "vector_name": "title",
            "query": {"kind": "dense", "values": [0.6, 0.8]},
            "filter": {
                "kind": "condition",
                "condition": {
                    "property": "tenant",
                    "operator": {"operator": "equals", "value": {"type": "string", "value": "other"}}
                }
            },
            "top_k": 1,
            "max_scanned_changes": 100
        }),
        None,
        None,
    );
    let (status, filtered) = post(
        &server,
        "/v1/vector/search",
        &filtered_out,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{filtered}");
    assert!(payload(&filtered)["hits"].as_array().unwrap().is_empty());
    let scroll_points = envelope(
        json!({
            "scope": "instance:socket-test",
            "collection_id": "documents",
            "vector_name": "title",
            "valid_at": 100,
            "limit": 1,
            "max_scanned_changes": 100,
            "filter": {
                "kind": "condition",
                "condition": {
                    "property": "tenant",
                    "operator": {"operator": "equals", "value": {"type": "string", "value": "alpha"}}
                }
            }
        }),
        None,
        None,
    );
    let (status, denied) = post(&server, "/v1/vector/points/scroll", &scroll_points, None);
    assert_eq!(status, 401, "{denied}");
    let (status, scrolled) = post(
        &server,
        "/v1/vector/points/scroll",
        &scroll_points,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{scrolled}");
    let point_page = payload(&scrolled);
    assert_eq!(point_page["known_at_cursor"], 11);
    assert_eq!(point_page["points"][0]["reference"]["id"], "alpha-title");
    assert_eq!(
        point_page["points"][0]["payload"]["tenant"]["value"],
        "alpha"
    );
    assert_eq!(point_page["truncated"], false);
    assert!(point_page.get("next_after").is_none());
    let retrieve_points = envelope(
        json!({
            "scope": "instance:socket-test",
            "collection_id": "documents",
            "vector_name": "title",
            "valid_at": 100,
            "references": [
                {"kind": "embedding", "id": "alpha-title"},
                {"kind": "embedding", "id": "missing-title"}
            ],
            "max_scanned_changes": 100
        }),
        None,
        None,
    );
    let (status, denied) = post(
        &server,
        "/v1/vector/points/retrieve",
        &retrieve_points,
        None,
    );
    assert_eq!(status, 401, "{denied}");
    let (status, retrieved) = post(
        &server,
        "/v1/vector/points/retrieve",
        &retrieve_points,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{retrieved}");
    let point_batch = payload(&retrieved);
    assert_eq!(point_batch["points"][0]["reference"]["id"], "alpha-title");
    assert_eq!(point_batch["missing"][0]["id"], "missing-title");
    let wrong_dimensions = envelope(
        json!({
            "scope": "instance:socket-test",
            "valid_at": 100,
            "collection_id": "documents",
            "vector_name": "title",
            "query": {"kind": "dense", "values": [0.6, 0.8, 0.0]},
            "top_k": 1,
            "max_scanned_changes": 100
        }),
        None,
        None,
    );
    let (status, mismatch) = post(
        &server,
        "/v1/vector/search",
        &wrong_dimensions,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 400, "{mismatch}");
    for (query, identity, expected) in [
        (
            "FROM series:series AT VALID 100 KNOWN HEAD WHERE series_id = \"latency\" PROJECT observed_at, value",
            "series:series:latency:100:latency-100",
            ("value", "unsigned", json!(17)),
        ),
        (
            "FROM series:series AT VALID 100 KNOWN HEAD WHERE observed_at >= 100 AND series_id != \"other\" PROJECT observed_at",
            "series:series:latency:100:latency-100",
            ("observed_at", "unsigned", json!(100)),
        ),
        (
            "FROM geo:location AT VALID 100 KNOWN HEAD WHERE subject_id = \"alpha\" PROJECT geometry_kind, longitude, latitude",
            "geo:location:alpha-office",
            ("longitude", "decimal", json!("-73")),
        ),
        (
            "FROM traverse:linked START document:alpha DIRECTION OUTGOING DEPTH 3 AT VALID 100 KNOWN HEAD PROJECT node_id, depth, path",
            "traversal:linked:document:alpha:1:alpha-beta:beta",
            ("node_id", "string", json!("beta")),
        ),
    ] {
        let request = envelope(
            json!({
                "scope": "instance:socket-test",
                "query": query,
                "parameters": {},
                "budget": {
                    "max_scanned_changes": 100,
                    "max_rows": 10,
                    "max_output_bytes": 4096,
                    "max_batch_rows": 10
                }
            }),
            None,
            None,
        );
        let (status, queried) = post(
            &server,
            "/v1/query",
            &request,
            Some((&session_id, &token)),
        );
        assert_eq!(status, 200, "{queried}");
        let result = payload(&queried);
        assert_eq!(result["known_at_cursor"], 11);
        assert_eq!(result["plan"]["exact"], true);
        assert_eq!(result["rows"][0]["identity"], identity);
        assert_eq!(result["rows"][0]["values"][expected.0]["type"], expected.1);
        assert_eq!(
            result["rows"][0]["values"][expected.0]["value"],
            expected.2
        );
    }
    let live_query = envelope(
        json!({
            "scope": "instance:socket-test",
            "query": "FROM series:series AT VALID 100 KNOWN HEAD WHERE series_id = \"latency\" PROJECT observed_at, value",
            "parameters": {},
            "after_cursor": 0,
            "budget": {
                "max_scanned_changes": 100,
                "max_rows": 10,
                "max_output_bytes": 4096,
                "max_batch_rows": 10
            },
            "max_delta_rows": 10
        }),
        None,
        None,
    );
    let (status, denied) = post(&server, "/v1/query/live/poll", &live_query, None);
    assert_eq!(status, 401, "{denied}");
    let (status, live) = post(
        &server,
        "/v1/query/live/poll",
        &live_query,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{live}");
    let live = payload(&live);
    assert_eq!(live["from_cursor"], 0);
    assert_eq!(live["through_cursor"], 11);
    assert_eq!(
        live["added"][0]["identity"],
        "series:series:latency:100:latency-100"
    );
    assert!(live["updated"].as_array().unwrap().is_empty());
    assert!(live["removed"].as_array().unwrap().is_empty());
    let ensure_index = envelope(
        json!({
            "scope": "instance:socket-test",
            "index_id": "document-title",
            "definition_query": "FROM record:document AT VALID 100 KNOWN HEAD PROJECT title",
            "unique": false,
            "budget": {
                "max_scanned_changes": 100,
                "max_rows": 10,
                "max_output_bytes": 4096,
                "max_batch_rows": 10
            }
        }),
        Some("ensure-document-title"),
        None,
    );
    let (status, denied) = post(&server, "/v1/query/indexes/ensure", &ensure_index, None);
    assert_eq!(status, 401, "{denied}");
    let (status, ensured) = post(
        &server,
        "/v1/query/indexes/ensure",
        &ensure_index,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{ensured}");
    assert_eq!(payload(&ensured)["idempotent_replay"], false);
    assert_eq!(payload(&ensured)["index"]["state"], "ready");
    assert_eq!(payload(&ensured)["index"]["source_cursor"], 4);
    assert_eq!(payload(&ensured)["index"]["artifact_rows"], 2);
    let (status, replayed) = post(
        &server,
        "/v1/query/indexes/ensure",
        &ensure_index,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{replayed}");
    assert_eq!(payload(&replayed)["idempotent_replay"], true);
    let independently_ensured = envelope(
        ensure_index["payload"].clone(),
        Some("ensure-document-title-second"),
        None,
    );
    let (status, independently_ensured) = post(
        &server,
        "/v1/query/indexes/ensure",
        &independently_ensured,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{independently_ensured}");
    assert_eq!(payload(&independently_ensured)["idempotent_replay"], false);
    assert_eq!(payload(&independently_ensured)["index"]["generation"], 1);
    assert_eq!(payload(&independently_ensured)["index"]["source_cursor"], 4);
    let mut collision = ensure_index.clone();
    collision["payload"]["definition_query"] =
        json!("FROM record:document AT VALID 101 KNOWN HEAD PROJECT title");
    let (status, collision) = post(
        &server,
        "/v1/query/indexes/ensure",
        &collision,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 409, "{collision}");
    let list_indexes = envelope(json!({"scope": "instance:socket-test"}), None, None);
    let (status, listed) = post(
        &server,
        "/v1/query/indexes/list",
        &list_indexes,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{listed}");
    assert_eq!(payload(&listed)["indexes"].as_array().unwrap().len(), 1);
    assert_eq!(payload(&listed)["indexes"][0]["index_id"], "document-title");
    let indexed_query = envelope(
        json!({
            "scope": "instance:socket-test",
            "query": "FROM record:document AT VALID 100 KNOWN HEAD WHERE title = \"Alpha\" PROJECT title EXPLAIN CONTRACT",
            "parameters": {},
            "budget": {
                "max_scanned_changes": 100,
                "max_rows": 10,
                "max_output_bytes": 4096,
                "max_batch_rows": 10
            }
        }),
        None,
        None,
    );
    let (status, indexed) = post(
        &server,
        "/v1/query",
        &indexed_query,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{indexed}");
    assert!(payload(&indexed)["plan"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|candidate| candidate["name"] == "index:document-title"
            && candidate["selected"] == true));
    assert_eq!(
        payload(&indexed)["rows"][0]["identity"],
        "record:document:alpha"
    );
    let first_feed = envelope(
        json!({
            "scope": "instance:socket-test",
            "after_cursor": 0,
            "limit": 3
        }),
        None,
        None,
    );
    let (status, denied) = post(&server, "/v1/changes/read", &first_feed, None);
    assert_eq!(status, 401, "{denied}");
    let (status, first_page) = post(
        &server,
        "/v1/changes/read",
        &first_feed,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{first_page}");
    let first_page = payload(&first_page);
    assert_eq!(first_page["requested_after_cursor"], 0);
    assert_eq!(first_page["through_cursor"], 3);
    assert_eq!(first_page["head_cursor"], 11);
    assert_eq!(first_page["has_more"], true);
    assert_eq!(first_page["changes"].as_array().unwrap().len(), 3);
    assert_eq!(first_page["changes"][0]["mutation"]["family"], "data");
    assert_eq!(
        first_page["changes"][0]["mutation"]["mutation"]["mutation"],
        "put_schema"
    );
    assert_eq!(first_page["changes"][1]["mutation"]["family"], "claim");
    assert_eq!(
        first_page["changes"][1]["mutation"]["claim"]["producer"],
        "socket-test"
    );
    assert_eq!(
        first_page["changes"][1]["mutation"]["claim"]["tier"],
        "local"
    );
    let first_page_tail = first_page["changes"][2]["change_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_feed = envelope(
        json!({
            "scope": "instance:socket-test",
            "after_cursor": 3,
            "limit": 8
        }),
        None,
        None,
    );
    let (status, second_page) = post(
        &server,
        "/v1/changes/read",
        &second_feed,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{second_page}");
    let second_page = payload(&second_page);
    assert_eq!(second_page["through_cursor"], 11);
    assert_eq!(second_page["has_more"], false);
    assert_eq!(second_page["changes"].as_array().unwrap().len(), 8);
    assert_eq!(
        second_page["changes"][0]["previous_change_sha256"],
        first_page_tail
    );
    assert_eq!(
        second_page["changes"][7]["mutation"]["mutation"]["mutation"],
        "publish_object_reference"
    );
    server.stop();

    let server = start(&root);
    let (status, replayed) = post(&server, &path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200, "{replayed}");
    assert_eq!(payload(&replayed)["idempotent_replay"], true);
    assert_eq!(
        payload(&replayed)["runtime_commit_sha256"],
        receipt["runtime_commit_sha256"]
    );
    let resumed_feed = envelope(
        json!({
            "scope": "instance:socket-test",
            "after_cursor": 10,
            "limit": 1
        }),
        None,
        None,
    );
    let (status, resumed) = post(
        &server,
        "/v1/changes/read",
        &resumed_feed,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200, "{resumed}");
    assert_eq!(payload(&resumed)["through_cursor"], 11);
    assert_eq!(payload(&resumed)["changes"][0]["cursor"], 11);
    assert_eq!(
        payload(&resumed)["changes"][0]["mutation"]["mutation"]["mutation"],
        "publish_object_reference"
    );
    server.stop();

    let engine = PersistentEngine::open(&root).unwrap();
    let page = engine
        .runtime_changes_since(0, 32, Some(&ScopeId::new("instance:socket-test").unwrap()))
        .unwrap();
    assert_eq!(page.changes.len(), 11);
    assert_eq!(engine.runtime_cursor().unwrap(), 11);
    assert_eq!(engine.sequence().unwrap(), 1);
}

#[test]
fn authenticated_estate_read_returns_the_public_snapshot_only() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("instance");
    {
        let engine = PersistentEngine::open(&root).unwrap();
        let repository =
            rrd_estate::EstateRepository::new(&engine, CanonicalId::new("estate-a").unwrap());
        repository
            .create(&estate_context(10, "create-estate"))
            .unwrap();
        repository
            .set_desired(&rrd_estate::SetDesired {
                context: estate_context(20, "deploy-project-a"),
                instance_id: CanonicalId::new("project-a").unwrap(),
                idempotency_key: "deploy-project-a".into(),
                target: rrd_estate::DesiredTarget {
                    phase: rrd_estate::DesiredPhase::Running,
                    deployment_ref: CanonicalId::new("local-rrd").unwrap(),
                    version: "0.1.0".into(),
                    configuration_sha256: "a".repeat(64),
                },
            })
            .unwrap();
    }
    let server = start(&root);
    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2,
            }
        }),
        Some("estate-session"),
        None,
    );
    let (status, created) = post(&server, "/v1/sessions", &create, None);
    assert_eq!(status, 200);
    let session_id = payload(&created)["session_id"].as_str().unwrap();
    let token = payload(&created)["token"].as_str().unwrap();

    let mut read = envelope(json!({}), None, None);
    read["resource"]["segments"] = json!([
        {"kind": "estate", "id": "estate-a"},
        {"kind": "instance", "id": "socket-test"}
    ]);
    let (status, denied) = post(&server, "/v1/estates/estate-a/read", &read, None);
    assert_eq!(status, 401);
    assert_eq!(denied["outcome"]["error"]["code"], "unauthenticated");

    let (status, response) = post(
        &server,
        "/v1/estates/estate-a/read",
        &read,
        Some((session_id, token)),
    );
    assert_eq!(status, 200, "{response}");
    assert_eq!(payload(&response)["id"], "estate-a");
    assert_eq!(payload(&response)["revision"], 2);
    assert_eq!(payload(&response)["instances"][0]["id"], "project-a");
    assert_eq!(payload(&response)["operations"][0]["state"], "pending");
    assert!(payload(&response).get("idempotency").is_none());
    assert_eq!(payload(&response)["idempotency_binding_count"], 1);

    let (status, mismatch) = post(
        &server,
        "/v1/estates/other-estate/read",
        &read,
        Some((session_id, token)),
    );
    assert_eq!(status, 412);
    assert_eq!(mismatch["outcome"]["error"]["code"], "failed_precondition");
}

#[test]
fn server_denies_remote_bind_before_opening_a_listener() {
    let temporary = tempfile::tempdir().unwrap();
    let engine = RrdEngine::open(
        &temporary.path().join("instance"),
        CanonicalId::new("socket-test").unwrap(),
        [7; 32],
    )
    .unwrap();
    let result = RrdHttpServer::bind(engine, "0.0.0.0:0".parse().unwrap());
    assert!(matches!(result, Err(HttpError::RemoteBindDenied(_))));
}

#[test]
fn binary_refuses_remote_bind() {
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    std::fs::create_dir(&project).unwrap();
    InstanceManifest::ensure_dedicated(&project).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rrd-server"))
        .args(["--root", project.to_str().unwrap(), "--bind", "0.0.0.0:0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("denied without authenticated TLS"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn standalone_daemon_process_passes_the_shared_corpus_and_exclusively_owns_its_root() {
    let _fixture = acquire_server_fixture();
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    std::fs::create_dir(&project).unwrap();
    InstanceManifest::ensure_dedicated_as(&project, "socket-test").unwrap();
    let root = project.join(".rrflow/rrd");

    let ready = temporary.path().join("RRD.READY");
    let shutdown_request = temporary.path().join("SHUTDOWN.REQUEST");
    let shutdown_complete = temporary.path().join("SHUTDOWN.COMPLETE");
    let mut child = Command::new(env!("CARGO_BIN_EXE_rrd-server"))
        .args([
            "--root",
            project.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
            "--ready-file",
            ready.to_str().unwrap(),
            "--shutdown-request-file",
            shutdown_request.to_str().unwrap(),
            "--shutdown-complete-file",
            shutdown_complete.to_str().unwrap(),
        ])
        .spawn()
        .unwrap();
    let deadline = Instant::now() + INTEGRATION_IO_TIMEOUT;
    while !ready.is_file() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("rrd-server child exited before readiness: {status}");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("rrd-server child did not publish readiness");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let readiness: Value = serde_json::from_slice(&std::fs::read(&ready).unwrap()).unwrap();
    let address = readiness["url"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap()
        .parse()
        .unwrap();
    let process = RunningServerProcess {
        address,
        child: Some(child),
        shutdown_request,
        shutdown_complete,
    };

    assert!(PersistentEngine::open(&root).is_err());
    let (status, capabilities) = http(process.address, "GET", "/v1/capabilities", &[], &[]);
    assert_eq!(status, 200);
    assert_eq!(payload(&capabilities)["deployment_mode"], "local_daemon");

    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 1
            }
        }),
        Some("deployment-process-session"),
        None,
    );
    let create_bytes = serde_json::to_vec(&create).unwrap();
    let (status, created) = http(
        process.address,
        "POST",
        "/v1/sessions",
        &[("Content-Type", "application/json")],
        &create_bytes,
    );
    assert_eq!(status, 200, "{created}");
    let session = payload(&created)["session_id"].as_str().unwrap();
    let token = payload(&created)["token"].as_str().unwrap();
    let authorization = format!("Bearer {token}");
    let begin = envelope(
        json!({"scope": "data", "timeout_ms": 10_000}),
        Some("deployment-process-begin"),
        None,
    );
    let begin_bytes = serde_json::to_vec(&begin).unwrap();
    let (status, began) = http(
        process.address,
        "POST",
        "/v1/transactions",
        &[
            ("Content-Type", "application/json"),
            ("X-RRD-Session", session),
            ("Authorization", &authorization),
        ],
        &begin_bytes,
    );
    assert_eq!(status, 200, "{began}");
    let transaction = payload(&began)["transaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mutations = deployment_mutations();
    let operation_sha256 = transaction_operation_sha256(&mutations);
    let commit = envelope(
        json!({
            "operation_sha256": operation_sha256,
            "mutations": mutations
        }),
        Some("deployment-process-commit"),
        Some(u64::MAX),
    );
    let commit_bytes = serde_json::to_vec(&commit).unwrap();
    let (status, committed) = http(
        process.address,
        "POST",
        &format!("/v1/transactions/{transaction}/commit"),
        &[
            ("Content-Type", "application/json"),
            ("X-RRD-Session", session),
            ("Authorization", &authorization),
        ],
        &commit_bytes,
    );
    assert_eq!(status, 200, "{committed}");
    assert_eq!(payload(&committed)["first_runtime_cursor"], 1);
    assert_eq!(payload(&committed)["last_runtime_cursor"], 3);
    assert_eq!(payload(&committed)["mutation_count"], 3);

    let corpus = deployment_corpus();
    let query = envelope(
        json!({
            "scope": "instance:socket-test",
            "query": corpus.query.rrflowql,
            "parameters": {},
            "budget": {
                "max_scanned_changes": 100,
                "max_rows": 10,
                "max_output_bytes": 16384,
                "max_batch_rows": 10
            }
        }),
        None,
        None,
    );
    let query_bytes = serde_json::to_vec(&query).unwrap();
    let (status, result) = http(
        process.address,
        "POST",
        "/v1/query",
        &[
            ("Content-Type", "application/json"),
            ("X-RRD-Session", session),
            ("Authorization", &authorization),
        ],
        &query_bytes,
    );
    assert_eq!(status, 200, "{result}");
    let actual = payload(&result)["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            row["identity"]
                .as_str()
                .unwrap()
                .strip_prefix("record:document:")
                .unwrap()
        })
        .collect::<Vec<_>>();
    let expected = corpus
        .expected_ids
        .iter()
        .map(CanonicalId::as_str)
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_eq!(
        payload(&result)["rows"][0]["values"]["body"]["value"],
        corpus.documents[0].text
    );

    process.stop();
    let reopened = PersistentEngine::open(&root).unwrap();
    assert_eq!(reopened.runtime_cursor().unwrap(), 3);
}

#[test]
fn token_key_is_stable_exact_and_private() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("RRD.SERVER.SECRET");
    let first = load_or_create_token_key(&path).unwrap();
    let second = load_or_create_token_key(&path).unwrap();
    assert_eq!(first, second);
    assert_ne!(first, [0; 32]);
    assert_eq!(std::fs::read(&path).unwrap().len(), 32);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o077,
            0
        );
    }
}

#[test]
fn real_socket_exercises_lifecycle_commit_and_restart_replay() {
    let (_temporary, root, server) = start_root();
    let (status, live) = http(server.address, "GET", "/v1/health/live", &[], &[]);
    assert_eq!(status, 200);
    assert_eq!(live["outcome"]["status"], "ok");
    let (status, capabilities) = http(server.address, "GET", "/v1/capabilities", &[], &[]);
    assert_eq!(status, 200);
    assert_eq!(payload(&capabilities)["deployment_mode"], "local_daemon");
    assert!(payload(&capabilities)["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|capability| {
            capability["name"] == "remote-listen" && capability["status"] == "unavailable"
        }));
    let (status, catalogue) = http(server.address, "GET", "/v1/schema/endpoints", &[], &[]);
    assert_eq!(status, 200, "{catalogue}");
    assert_eq!(payload(&catalogue)["protocol_version"], 1);
    assert_eq!(
        payload(&catalogue)["endpoints"].as_array().unwrap().len(),
        34
    );
    assert_eq!(
        payload(&catalogue)["websocket_endpoints"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let (status, openapi) = http(server.address, "GET", "/v1/schema/openapi", &[], &[]);
    assert_eq!(status, 200, "{openapi}");
    assert_eq!(payload(&openapi)["openapi"], "3.1.0");
    assert_eq!(payload(&openapi)["x-rrd-endpoint-count"], 34);
    assert_eq!(payload(&openapi)["x-rrd-websocket-endpoint-count"], 1);
    assert!(
        payload(&openapi)["paths"]["/v1/query"]["post"]["requestBody"]["content"]
            ["application/json"]["schema"]
            .is_object()
    );
    assert!(
        payload(&openapi)["paths"]["/v1/query/live/poll"]["post"]["requestBody"]["content"]
            ["application/json"]["schema"]
            .is_object()
    );
    assert!(
        payload(&openapi)["paths"]["/v1/query/indexes/ensure"]["post"]["requestBody"]["content"]
            ["application/json"]["schema"]
            .is_object()
    );

    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2,
            }
        }),
        Some("create-key"),
        None,
    );
    let (status, created) = post(&server, "/v1/sessions", &create, None);
    assert_eq!(status, 200);
    let session_id = payload(&created)["session_id"].as_str().unwrap().to_owned();
    let token = payload(&created)["token"].as_str().unwrap().to_owned();
    let (status, create_replay) = post(&server, "/v1/sessions", &create, None);
    assert_eq!(status, 200);
    assert_eq!(payload(&create_replay), payload(&created));

    let begin = envelope(
        json!({"scope": "claims", "timeout_ms": 60_000}),
        Some("begin-key"),
        None,
    );
    let (status, began) = post(
        &server,
        "/v1/transactions",
        &begin,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200);
    let transaction_id = payload(&began)["transaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mutations = json!([{
        "mutation": "assert_claim",
        "subject": "document-1",
        "predicate": "contains",
        "object": "socket verified",
        "valid_from": 1,
        "tx_time": 1,
        "producer": "socket-test",
        "confidence": 0.95,
    }]);
    let preview = envelope(
        json!({"mutations": mutations.clone()}),
        Some("prepare-claims"),
        None,
    );
    let (status, previewed) = post(
        &server,
        &format!("/v1/transactions/{transaction_id}/preview"),
        &preview,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 200);
    assert_eq!(payload(&previewed)["mutations"], mutations);

    let typed_mutations: Vec<TransactionMutation> =
        serde_json::from_value(mutations.clone()).unwrap();
    let operation_sha256 = transaction_operation_sha256(&typed_mutations);
    let commit = envelope(
        json!({
            "operation_sha256": operation_sha256,
            "mutations": mutations,
        }),
        Some("commit-key"),
        Some(u64::MAX),
    );
    let commit_path = format!("/v1/transactions/{transaction_id}/commit");
    let mut missing_deadline = commit.clone();
    missing_deadline["context"]["deadline_unix_ms"] = Value::Null;
    let (status, denied) = post(
        &server,
        &commit_path,
        &missing_deadline,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 400);
    assert_eq!(denied["outcome"]["error"]["code"], "invalid_argument");
    let (status, committed) = post(&server, &commit_path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200, "{committed}");
    assert_eq!(payload(&committed)["idempotent_replay"], false);
    let (status, replay) = post(&server, &commit_path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200);
    assert_eq!(payload(&replay)["idempotent_replay"], true);
    server.stop();

    let server = start(&root);
    let (status, ready) = http(server.address, "GET", "/v1/health/ready", &[], &[]);
    assert_eq!(status, 200);
    assert_eq!(payload(&ready)["claim_sequence"], 1);
    let (status, restart_replay) =
        post(&server, &commit_path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200);
    assert_eq!(payload(&restart_replay)["idempotent_replay"], true);

    let mut collision = commit.clone();
    collision["context"]["idempotency_key"] = json!("different-key");
    let (status, conflict) = post(
        &server,
        &commit_path,
        &collision,
        Some((&session_id, &token)),
    );
    assert_eq!(status, 409);
    assert_eq!(conflict["outcome"]["error"]["code"], "conflict");
}

#[test]
fn malformed_oversized_deadline_and_resource_fail_before_mutation() {
    let (_temporary, _root, server) = start_root();
    let (status, malformed) = http(
        server.address,
        "POST",
        "/v1/sessions",
        &[("Content-Type", "application/json")],
        b"{",
    );
    assert_eq!(status, 400);
    assert_eq!(malformed["outcome"]["error"]["code"], "invalid_argument");

    let oversized = vec![b' '; RRD_MAX_BODY_BYTES + 1];
    let (status, exhausted) = http(
        server.address,
        "POST",
        "/v1/sessions",
        &[("Content-Type", "application/json")],
        &oversized,
    );
    assert_eq!(status, 429);
    assert_eq!(exhausted["outcome"]["error"]["code"], "resource_exhausted");

    let expired = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2,
            }
        }),
        Some("expired-key"),
        Some(1),
    );
    let (status, deadline) = post(&server, "/v1/sessions", &expired, None);
    assert_eq!(status, 504);
    assert_eq!(deadline["outcome"]["error"]["code"], "deadline_exceeded");

    let mut wrong_resource = expired;
    wrong_resource["context"]["deadline_unix_ms"] = Value::Null;
    wrong_resource["resource"]["segments"][0]["id"] = json!("other-instance");
    let (status, denied) = post(&server, "/v1/sessions", &wrong_resource, None);
    assert_eq!(status, 412);
    assert_eq!(denied["outcome"]["error"]["code"], "failed_precondition");

    let (status, ready) = http(server.address, "GET", "/v1/health/ready", &[], &[]);
    assert_eq!(status, 200);
    assert_eq!(payload(&ready)["claim_sequence"], 0);
}

#[test]
fn close_and_abort_paths_require_matching_session_identity() {
    let (_temporary, _root, server) = start_root();
    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2,
            }
        }),
        Some("create-key"),
        None,
    );
    let (_, created) = post(&server, "/v1/sessions", &create, None);
    let session_id = payload(&created)["session_id"].as_str().unwrap();
    let token = payload(&created)["token"].as_str().unwrap();
    let close = envelope(json!({}), Some("close-key"), None);
    let (status, denied) = delete(
        &server,
        "/v1/sessions/different-session",
        &close,
        (session_id, token),
    );
    assert_eq!(status, 403);
    assert_eq!(denied["outcome"]["error"]["code"], "permission_denied");
    let (status, closed) = delete(
        &server,
        &format!("/v1/sessions/{session_id}"),
        &close,
        (session_id, token),
    );
    assert_eq!(status, 200);
    assert_eq!(payload(&closed)["state"], "closed");
    let (status, replay) = delete(
        &server,
        &format!("/v1/sessions/{session_id}"),
        &close,
        (session_id, token),
    );
    assert_eq!(status, 200);
    assert_eq!(payload(&replay)["idempotent_replay"], true);
}

#[test]
fn public_correlation_ids_remain_header_safe() {
    assert!(CorrelationId::new("session:test_1-token").is_ok());
}

#[test]
fn socket_rotation_quota_abort_and_idle_expiry_are_enforced() {
    let (_temporary, _root, server) = start_root();
    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 1_000,
                "absolute_timeout_ms": 5_000,
                "max_open_transactions": 1,
            }
        }),
        Some("create-key"),
        None,
    );
    let (_, created) = post(&server, "/v1/sessions", &create, None);
    let session_id = payload(&created)["session_id"].as_str().unwrap().to_owned();
    let original_token = payload(&created)["token"].as_str().unwrap().to_owned();
    let renew = envelope(json!({}), Some("renew-key"), None);
    let renewal_path = format!("/v1/sessions/{session_id}/renew");
    let (status, renewed) = post(
        &server,
        &renewal_path,
        &renew,
        Some((&session_id, &original_token)),
    );
    assert_eq!(status, 200);
    let renewed_token = payload(&renewed)["token"].as_str().unwrap().to_owned();
    assert_ne!(renewed_token, original_token);
    let (status, renewal_replay) = post(
        &server,
        &renewal_path,
        &renew,
        Some((&session_id, &original_token)),
    );
    assert_eq!(status, 200);
    assert_eq!(payload(&renewal_replay), payload(&renewed));

    let begin = envelope(
        json!({"scope": "claims", "timeout_ms": 5_000}),
        Some("begin-key"),
        None,
    );
    let (status, old_token_denied) = post(
        &server,
        "/v1/transactions",
        &begin,
        Some((&session_id, &original_token)),
    );
    assert_eq!(status, 401);
    assert_eq!(
        old_token_denied["outcome"]["error"]["code"],
        "unauthenticated"
    );
    let (status, began) = post(
        &server,
        "/v1/transactions",
        &begin,
        Some((&session_id, &renewed_token)),
    );
    assert_eq!(status, 200);
    let transaction_id = payload(&began)["transaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut over_quota = begin.clone();
    over_quota["context"]["idempotency_key"] = json!("begin-over-quota");
    let (status, quota) = post(
        &server,
        "/v1/transactions",
        &over_quota,
        Some((&session_id, &renewed_token)),
    );
    assert_eq!(status, 429);
    assert_eq!(quota["outcome"]["error"]["code"], "resource_exhausted");

    let abort = envelope(json!({}), Some("abort-key"), None);
    let abort_path = format!("/v1/transactions/{transaction_id}");
    let (status, aborted) = delete(&server, &abort_path, &abort, (&session_id, &renewed_token));
    assert_eq!(status, 200);
    assert_eq!(payload(&aborted)["state"], "aborted");
    let (status, abort_replay) =
        delete(&server, &abort_path, &abort, (&session_id, &renewed_token));
    assert_eq!(status, 200);
    assert_eq!(payload(&abort_replay), payload(&aborted));

    std::thread::sleep(Duration::from_millis(1_100));
    let mut after_expiry = begin;
    after_expiry["context"]["idempotency_key"] = json!("begin-after-expiry");
    let (status, expired) = post(
        &server,
        "/v1/transactions",
        &after_expiry,
        Some((&session_id, &renewed_token)),
    );
    assert_eq!(status, 412);
    assert_eq!(expired["outcome"]["error"]["code"], "failed_precondition");
}

#[test]
fn concurrent_same_commit_is_single_acceptance_and_retry_converges() {
    let (_temporary, _root, server) = start_root();
    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2,
            }
        }),
        Some("create-key"),
        None,
    );
    let (_, created) = post(&server, "/v1/sessions", &create, None);
    let session_id = payload(&created)["session_id"].as_str().unwrap().to_owned();
    let token = payload(&created)["token"].as_str().unwrap().to_owned();
    let begin = envelope(
        json!({"scope": "claims", "timeout_ms": 60_000}),
        Some("begin-key"),
        None,
    );
    let (_, began) = post(
        &server,
        "/v1/transactions",
        &begin,
        Some((&session_id, &token)),
    );
    let transaction_id = payload(&began)["transaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mutations = json!([{
        "mutation": "assert_claim",
        "subject": "document-1",
        "predicate": "contains",
        "object": "concurrent",
        "valid_from": 1,
        "tx_time": 1,
        "producer": "socket-test"
    }]);
    let typed: Vec<TransactionMutation> = serde_json::from_value(mutations.clone()).unwrap();
    let commit = envelope(
        json!({
            "operation_sha256": transaction_operation_sha256(&typed),
            "mutations": mutations,
        }),
        Some("commit-key"),
        Some(u64::MAX),
    );
    let path = format!("/v1/transactions/{transaction_id}/commit");
    let address = server.address;
    let mut workers = Vec::new();
    for _ in 0..2 {
        let body = serde_json::to_vec(&commit).unwrap();
        let path = path.clone();
        let session_id = session_id.clone();
        let authorization = format!("Bearer {token}");
        workers.push(std::thread::spawn(move || {
            http(
                address,
                "POST",
                &path,
                &[
                    ("Content-Type", "application/json"),
                    ("X-RRD-Session", &session_id),
                    ("Authorization", &authorization),
                ],
                &body,
            )
        }));
    }
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert!(results.iter().any(|(status, _)| *status == 200));
    assert!(results
        .iter()
        .all(|(status, _)| matches!(*status, 200 | 409)));

    let (status, converged) = post(&server, &path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200);
    assert_eq!(payload(&converged)["idempotent_replay"], true);
    let (status, ready) = http(server.address, "GET", "/v1/health/ready", &[], &[]);
    assert_eq!(status, 200);
    assert_eq!(payload(&ready)["claim_sequence"], 1);
}

#[test]
fn cancelled_data_commit_restarts_and_converges_on_durable_retry() {
    let (_temporary, root, server) = start_root();
    let create = envelope(
        json!({
            "limits": {
                "idle_timeout_ms": 60_000,
                "absolute_timeout_ms": 300_000,
                "max_open_transactions": 2,
            }
        }),
        Some("create-key"),
        None,
    );
    let (_, created) = post(&server, "/v1/sessions", &create, None);
    let session_id = payload(&created)["session_id"].as_str().unwrap().to_owned();
    let token = payload(&created)["token"].as_str().unwrap().to_owned();
    let begin = envelope(
        json!({"scope": "data", "timeout_ms": 60_000}),
        Some("begin-key"),
        None,
    );
    let (_, began) = post(
        &server,
        "/v1/transactions",
        &begin,
        Some((&session_id, &token)),
    );
    let transaction_id = payload(&began)["transaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let typed = deployment_mutations();
    let mutations = serde_json::to_value(&typed).unwrap();
    let commit = envelope(
        json!({
            "operation_sha256": transaction_operation_sha256(&typed),
            "mutations": mutations,
        }),
        Some("commit-key"),
        Some(u64::MAX),
    );
    let commit_path = format!("/v1/transactions/{transaction_id}/commit");
    let body = serde_json::to_vec(&commit).unwrap();
    let authorization = format!("Bearer {token}");
    let mut stream = TcpStream::connect(server.address).unwrap();
    write!(
        stream,
        "POST {commit_path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nX-RRD-Session: {session_id}\r\nAuthorization: {authorization}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        server.address,
        body.len()
    )
    .unwrap();
    stream.write_all(&body).unwrap();
    stream.flush().unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    drop(stream);
    server.stop();

    let server = start(&root);
    let (status, first_retry) = post(&server, &commit_path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200);
    assert!(payload(&first_retry)["idempotent_replay"].is_boolean());
    let (status, converged) = post(&server, &commit_path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200);
    assert_eq!(payload(&converged)["idempotent_replay"], true);
    let (_, ready) = http(server.address, "GET", "/v1/health/ready", &[], &[]);
    assert_eq!(payload(&ready)["runtime_cursor"], 3);
    assert_eq!(payload(&ready)["claim_sequence"], 0);
}
