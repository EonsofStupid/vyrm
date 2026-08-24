use rrd_contract::{CanonicalId, CorrelationId, TransactionMutation, transaction_operation_sha256};
use rrd_security::{
    Action as SecurityAction, Principal, PrincipalKind, ResourceGrant, SECURITY_FORMAT,
    SecurityRepository, SecurityState,
};
use rrd_server::{HttpError, RRD_MAX_BODY_BYTES, RrdHttpServer, load_or_create_token_key};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::JoinHandle;
use std::time::Duration;
use vyrm_core::{
    RuntimeCommit, RuntimeEventSchema, RuntimeMutation, RuntimeProperties, RuntimePropertySchema,
    RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType,
    RuntimeValue, RuntimeValueType, ScopeId,
};
use vyrm_store::Engine;
use vyrm_store::PersistentEngine;

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
    let engine = PersistentEngine::open(root).unwrap();
    let token_key = load_or_create_token_key(&root.join("RRD.SERVER.SECRET")).unwrap();
    let server = RrdHttpServer::bind(
        engine,
        CanonicalId::new("socket-test").unwrap(),
        token_key,
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
    let address = server.local_addr();
    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(server.serve_until(async move {
                let _ = receiver.await;
            }))
    });
    RunningServer {
        address,
        shutdown: Some(shutdown),
        thread: Some(thread),
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
        .set_read_timeout(Some(Duration::from_secs(5)))
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

#[test]
fn initialized_security_authority_binds_sessions_and_denies_ungranted_routes() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("instance");
    seed_query_fixture(&root);
    let engine = PersistentEngine::open(&root).unwrap();
    let instance = CanonicalId::new("socket-test").unwrap();
    let resource = rrd_contract::ResourcePath {
        segments: vec![
            rrd_contract::ResourceId::new(rrd_contract::ResourceKind::Instance, "socket-test")
                .unwrap(),
        ],
    };
    let principal = Principal {
        id: CanonicalId::new("connectome-local").unwrap(),
        kind: PrincipalKind::User,
        credential_sha256: vyrm_core::digest::sha256_hex(b"local-api-key"),
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        grants: vec![
            ResourceGrant {
                action: SecurityAction::SessionCreate,
                resource_prefix: resource.clone(),
            },
            ResourceGrant {
                action: SecurityAction::QueryExecute,
                resource_prefix: resource,
            },
            ResourceGrant {
                action: SecurityAction::AuditRead,
                resource_prefix: rrd_contract::ResourcePath {
                    segments: vec![
                        rrd_contract::ResourceId::new(
                            rrd_contract::ResourceKind::Instance,
                            "socket-test",
                        )
                        .unwrap(),
                    ],
                },
            },
        ],
    };
    SecurityRepository::new(&engine, instance)
        .initialize(
            SecurityState {
                format_version: SECURITY_FORMAT,
                revision: 1,
                principals: BTreeMap::from([(principal.id.clone(), principal)]),
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
        None,
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
    invalid_query["payload"]["query"] = json!("NOT VYRMQL");
    let (status, failed) = post(&server, "/v1/query", &invalid_query, Some((session, token)));
    assert_eq!(status, 400, "{failed}");

    let audit = envelope(json!({"after_sequence": 0, "limit": 32}), None, None);
    let (status, audited) = post(&server, "/v1/audit/read", &audit, Some((session, token)));
    assert_eq!(status, 200, "{audited}");
    let records = payload(&audited)["records"].as_array().unwrap();
    assert_eq!(records.len(), 13, "{audited}");
    assert_eq!(records[0]["action"], "service_inspect");
    assert_eq!(records[0]["decision"], "allowed");
    assert_eq!(records[1]["action"], "unknown_request");
    assert_eq!(records[1]["decision"], "failed");
    assert_eq!(
        records
            .iter()
            .filter(|record| {
                record["action"] == "session_create" && record["decision"] == "denied"
            })
            .count(),
        2
    );
    assert!(
        records.iter().any(|record| {
            record["action"] == "session_create" && record["phase"] == "authorized"
        })
    );
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
    assert!(
        records
            .iter()
            .any(|record| { record["action"] == "audit_read" && record["phase"] == "authorized" })
    );
    let encoded = serde_json::to_string(records).unwrap();
    assert!(!encoded.contains("local-api-key"));
    assert!(!encoded.contains("wrong"));
    server.stop();
    let reopened = PersistentEngine::open(&root).unwrap();
    assert_eq!(reopened.runtime_cursor().unwrap(), 2);
    let journal = reopened.control_journal_since(0, 64).unwrap();
    let encoded = serde_json::to_string(&journal).unwrap();
    assert!(!encoded.contains("local-api-key"));
    assert!(!encoded.contains("ApiKey"));
}

#[test]
fn authenticated_query_exposes_exact_vyrmql_vyrmmx_contract() {
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
            "wait_timeout_ms": 1_000
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
    assert!(
        payload(&timed_out)["page"]["changes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
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
    assert_eq!(
        payload(&backed_up)["backup"]["object_payloads"],
        "referenced_only"
    );
    assert_eq!(payload(&backed_up)["backup"]["application_complete"], false);
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
            "field": "title_embedding",
            "valid_from": 100,
            "value": {"kind": "dense", "values": [0.6, 0.8]},
            "properties": {}
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
    let vector_search = envelope(
        json!({
            "scope": "instance:socket-test",
            "valid_at": 100,
            "field": "title_embedding",
            "query": {"kind": "dense", "values": [0.6, 0.8]},
            "metric": "cosine",
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
    assert_eq!(search["known_at_cursor"], 11);
    assert_eq!(search["access_path"], "exact_scan");
    assert_eq!(search["exact"], true);
    assert_eq!(search["hits"][0]["reference"]["id"], "alpha-title");
    assert_eq!(search["hits"][0]["subject"]["id"], "alpha");
    assert_eq!(search["hits"][0]["source_cursor"], 8);
    assert!((search["hits"][0]["score"].as_f64().unwrap() - 1.0).abs() < 1e-9);
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
    let engine = PersistentEngine::open(&temporary.path().join("instance")).unwrap();
    let result = RrdHttpServer::bind(
        engine,
        CanonicalId::new("socket-test").unwrap(),
        [7; 32],
        "0.0.0.0:0".parse().unwrap(),
    );
    assert!(matches!(result, Err(HttpError::RemoteBindDenied(_))));
}

#[test]
fn binary_refuses_remote_bind() {
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("instance");
    let output = Command::new(env!("CARGO_BIN_EXE_rrd-server"))
        .args([
            "--db",
            database.to_str().unwrap(),
            "--instance",
            "socket-test",
            "--bind",
            "0.0.0.0:0",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("denied before F4 security"),
        "unexpected stderr: {stderr}"
    );
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
    assert_eq!(payload(&capabilities)["deployment_mode"], "local_server");
    assert!(
        payload(&capabilities)["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| {
                capability["name"] == "remote-listen" && capability["status"] == "unavailable"
            })
    );
    let (status, catalogue) = http(server.address, "GET", "/v1/schema/endpoints", &[], &[]);
    assert_eq!(status, 200, "{catalogue}");
    assert_eq!(payload(&catalogue)["protocol_version"], 1);
    assert_eq!(
        payload(&catalogue)["endpoints"].as_array().unwrap().len(),
        21
    );
    let (status, openapi) = http(server.address, "GET", "/v1/schema/openapi", &[], &[]);
    assert_eq!(status, 200, "{openapi}");
    assert_eq!(payload(&openapi)["openapi"], "3.1.0");
    assert_eq!(payload(&openapi)["x-rrd-endpoint-count"], 21);
    assert!(payload(&openapi)["paths"]["/v1/query"]["post"]["requestBody"]
        ["content"]["application/json"]["schema"]
        .is_object());

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
    let preview = envelope(json!({"mutations": mutations.clone()}), None, None);
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
    assert!(
        results
            .iter()
            .all(|(status, _)| matches!(*status, 200 | 409))
    );

    let (status, converged) = post(&server, &path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200);
    assert_eq!(payload(&converged)["idempotent_replay"], true);
    let (status, ready) = http(server.address, "GET", "/v1/health/ready", &[], &[]);
    assert_eq!(status, 200);
    assert_eq!(payload(&ready)["claim_sequence"], 1);
}

#[test]
fn disconnect_after_commit_send_converges_on_durable_retry() {
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
        "object": "disconnect",
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
    std::thread::sleep(Duration::from_millis(20));
    drop(stream);

    let (status, first_retry) = post(&server, &commit_path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200);
    assert!(payload(&first_retry)["idempotent_replay"].is_boolean());
    let (status, converged) = post(&server, &commit_path, &commit, Some((&session_id, &token)));
    assert_eq!(status, 200);
    assert_eq!(payload(&converged)["idempotent_replay"], true);
    let (_, ready) = http(server.address, "GET", "/v1/health/ready", &[], &[]);
    assert_eq!(payload(&ready)["claim_sequence"], 1);
}
