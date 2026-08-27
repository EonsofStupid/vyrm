use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose,
};
use rrd_client::{is_unauthenticated, ClientConfig, Error, RequestOptions, RrdClient};
use rrd_contract::{
    AbortTransaction, BeginTransaction, CanonicalId, CreateSession, ExecuteQuery,
    PreviewTransaction, QueryBudget, ReadAudit, ReadChangefeed, ReadDiagnosticSnapshot, ResourceId,
    ResourceKind, ResourcePath, SessionLimits, TransactionMutation,
};
use rrd_core::{
    digest, RuntimeCommit, RuntimeProperties, RuntimePropertySchema, RuntimeRecord,
    RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType, RuntimeValue,
    RuntimeValueType, ScopeId,
};
use rrd_engine::{load_or_create_token_key, InstanceBinding, InstanceManifest, RrdEngine};
use rrd_security::{
    Action, Principal, PrincipalKind, ResourceGrant, SecurityRepository, SecurityState,
    SECURITY_FORMAT,
};
use rrd_server::RrdHttpServer;
use rrd_server::RrdMutualTlsServerConfig;
use rrd_store::{Engine, PersistentEngine};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ClientConfig as RustlsClientConfig, RootCertStore};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn test_ca() -> (Certificate, Issuer<'static, KeyPair>) {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
    ];
    let key = KeyPair::generate().unwrap();
    let certificate = params.self_signed(&key).unwrap();
    (certificate, Issuer::new(params, key))
}

fn test_identity(
    issuer: &Issuer<'static, KeyPair>,
    dns_names: Vec<String>,
    usage: ExtendedKeyUsagePurpose,
) -> (
    Vec<rustls::pki_types::CertificateDer<'static>>,
    PrivateKeyDer<'static>,
) {
    let mut params = CertificateParams::new(dns_names).unwrap();
    params.extended_key_usages = vec![usage];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    let key = KeyPair::generate().unwrap();
    let certificate = params.signed_by(&key, issuer).unwrap();
    (
        vec![certificate.der().clone()],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
    )
}

fn roots(ca: &Certificate) -> RootCertStore {
    let mut roots = RootCertStore::empty();
    roots.add(ca.der().clone()).unwrap();
    roots
}

fn instance_resource() -> ResourcePath {
    ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, "sdk-test").unwrap()],
    }
}

fn seed(engine: &PersistentEngine) {
    let mut registry = RuntimeSchemaRegistry::empty(1, "Rust SDK fixture");
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
    engine
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new("instance:sdk-test").unwrap(),
            at: 100,
            actor: "rrd-client-fixture".into(),
            expected_cursor: 0,
            mutations: vec![
                rrd_core::RuntimeMutation::Schema { registry },
                rrd_core::RuntimeMutation::Record {
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

#[tokio::test]
async fn rust_client_negotiates_authenticates_queries_and_reads_audit() {
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    InstanceManifest::ensure_dedicated_as(&project, "sdk-test").unwrap();
    let binding = InstanceBinding::discover(&project).unwrap();
    let root = binding.expected_store();
    let storage = PersistentEngine::open(&root).unwrap();
    seed(&storage);
    let runtime_head = storage.runtime_cursor().unwrap();
    storage
        .open_runtime_snapshot(
            &ScopeId::new("instance:sdk-test").unwrap(),
            "rust-sdk-fixture",
            u64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            )
            .unwrap(),
            3_600_000,
        )
        .unwrap();
    let instance = CanonicalId::new("sdk-test").unwrap();
    let resource = instance_resource();
    let principal = Principal {
        id: CanonicalId::new("rust-sdk").unwrap(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(b"sdk-api-key"),
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        grants: [
            Action::SessionCreate,
            Action::QueryExecute,
            Action::AuditRead,
            Action::TransactionBegin,
            Action::TransactionPreview,
            Action::TransactionAbort,
            Action::ChangefeedRead,
            Action::RuntimeToolCatalogueRead,
            Action::ServiceInspect,
            Action::DiagnosticsRead,
        ]
        .into_iter()
        .map(|action| ResourceGrant {
            action,
            resource_prefix: resource.clone(),
        })
        .collect(),
    };
    SecurityRepository::new(&storage, instance.clone())
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
    drop(storage);
    let token_key = load_or_create_token_key(&root.join("RRD.SERVER.SECRET")).unwrap();
    let engine =
        RrdEngine::open_bound_with_token_key(&binding, instance.clone(), token_key, 2).unwrap();
    let authority = binding.authority_binding().unwrap();
    let server =
        RrdHttpServer::bind_project(engine, authority, "127.0.0.1:0".parse().unwrap()).unwrap();
    let address = server.local_addr();
    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(server.serve_until(async move {
        let _ = receiver.await;
    }));

    let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_address = proxy_listener.local_addr().unwrap();
    let proxy = tokio::spawn(async move {
        let (first, _) = proxy_listener.accept().await.unwrap();
        drop(first);
        let (mut downstream, _) = proxy_listener.accept().await.unwrap();
        let mut upstream = tokio::net::TcpStream::connect(address).await.unwrap();
        tokio::io::copy_bidirectional(&mut downstream, &mut upstream)
            .await
            .unwrap();
    });

    let client = RrdClient::connect_local(
        proxy_address,
        instance,
        ClientConfig {
            request_timeout: Duration::from_secs(2),
            max_attempts: 2,
        },
    )
    .unwrap();
    let capabilities = client.capabilities().await.unwrap();
    assert_eq!(capabilities.protocol_version, 1);
    assert_eq!(
        capabilities.product_capabilities,
        rrd_engine::product_capability_catalogue()
    );
    let catalogue = client.endpoint_catalogue().await.unwrap();
    assert_eq!(catalogue.endpoints.len(), 31);
    let openapi = client.openapi_document().await.unwrap();
    assert_eq!(openapi["x-rrd-endpoint-count"], 31);

    let session_request = CreateSession {
        limits: SessionLimits {
            idle_timeout_ms: 60_000,
            absolute_timeout_ms: 300_000,
            max_open_transactions: 2,
        },
    };
    let wrong = client
        .create_session(
            CanonicalId::new("rust-sdk").unwrap(),
            "wrong",
            session_request.clone(),
            RequestOptions::mutation("request-wrong", "operation-wrong", "session-wrong").unwrap(),
        )
        .await
        .unwrap_err();
    assert!(is_unauthenticated(&wrong));

    let session = client
        .create_session(
            CanonicalId::new("rust-sdk").unwrap(),
            "sdk-api-key",
            session_request,
            RequestOptions::mutation("request-session", "operation-session", "session-key")
                .unwrap(),
        )
        .await
        .unwrap();
    let runtime_catalogue = client
        .runtime_tool_catalogue(
            &session,
            RequestOptions::read("request-runtime-list", "operation-runtime-list").unwrap(),
        )
        .await
        .unwrap();
    let service_status = client
        .invoke_runtime_tool(
            &session,
            &runtime_catalogue,
            &CanonicalId::new("rrflow_service_status").unwrap(),
            json!({}),
            RequestOptions::read("request-runtime-invoke", "operation-runtime-invoke").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(service_status.tool.as_str(), "rrflow_service_status");
    assert!(service_status.content.contains("sdk-test"));
    let denied = client
        .invoke_runtime_tool(
            &session,
            &runtime_catalogue,
            &CanonicalId::new("rrflow_context").unwrap(),
            json!({}),
            RequestOptions::read("request-runtime-denied", "operation-runtime-denied").unwrap(),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        denied,
        Error::Api {
            error: rrd_contract::ErrorBody {
                code: rrd_contract::ErrorCode::PermissionDenied,
                ..
            },
            ..
        }
    ));
    let query_request = ExecuteQuery {
        scope: "instance:sdk-test".into(),
        query: "FROM record:document AT VALID 100 KNOWN HEAD PROJECT id, title EXPLAIN CONTRACT"
            .into(),
        parameters: BTreeMap::new(),
        budget: QueryBudget {
            max_scanned_changes: 100,
            max_rows: 10,
            max_output_bytes: 4_096,
            max_batch_rows: 10,
        },
    };
    let query = client
        .execute_query(
            &session,
            query_request.clone(),
            RequestOptions::read("request-query", "operation-query").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(query.rows.len(), 1);
    assert_eq!(query.rows[0].identity, "record:document:alpha");
    let expired = client
        .execute_query(
            &session,
            query_request,
            RequestOptions::new("request-expired", "operation-expired", None, Some(1)).unwrap(),
        )
        .await
        .unwrap_err();
    assert!(matches!(expired, Error::Timeout));

    let transaction = client
        .begin_transaction(
            &session,
            BeginTransaction {
                scope: CanonicalId::new("claims").unwrap(),
                timeout_ms: 60_000,
            },
            RequestOptions::mutation("request-begin", "operation-begin", "transaction-begin")
                .unwrap(),
        )
        .await
        .unwrap();
    let preview = client
        .preview_transaction(
            &session,
            &transaction.transaction_id,
            PreviewTransaction {
                mutations: vec![TransactionMutation::AssertClaim {
                    subject: CanonicalId::new("document-alpha").unwrap(),
                    predicate: CanonicalId::new("contains").unwrap(),
                    object: "preview-only".into(),
                    valid_from: 100,
                    tx_time: 100,
                    producer: CanonicalId::new("rust-sdk").unwrap(),
                    confidence: Some(0.9),
                }],
            },
            RequestOptions::read("request-preview", "operation-preview").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preview.transaction_id, transaction.transaction_id);
    let aborted = client
        .abort_transaction(
            &session,
            &transaction.transaction_id,
            AbortTransaction {},
            RequestOptions::mutation("request-abort", "operation-abort", "transaction-abort")
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(aborted.state, rrd_contract::TransactionState::Aborted);

    let changes = client
        .read_changefeed(
            &session,
            ReadChangefeed {
                scope: "instance:sdk-test".into(),
                after_cursor: 0,
                limit: 64,
            },
            RequestOptions::read("request-changes", "operation-changes").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(changes.through_cursor, runtime_head);
    assert_eq!(
        changes.changes.len(),
        usize::try_from(runtime_head).unwrap()
    );

    let diagnostics = client
        .read_diagnostic_snapshot(
            &session,
            ReadDiagnosticSnapshot {
                scope: "instance:sdk-test".into(),
                graph_valid_at_unix_ms: 1_100,
                graph_known_at_cursor: Some(2),
                graph_compare_cursor: 0,
                runtime_max_scanned_changes: 1_024,
                changes_after_cursor: 0,
                change_limit: 64,
                audit_after_sequence: 0,
                audit_limit: 32,
            },
            RequestOptions::read("request-diagnostics", "operation-diagnostics").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(diagnostics.instance.id.as_str(), "sdk-test");
    assert_eq!(diagnostics.read.runtime_cursor, runtime_head);
    assert!(diagnostics.read.control_sequence > 0);
    assert_eq!(diagnostics.read.runtime_manifest_sha256.len(), 64);
    assert_eq!(diagnostics.changes.head_cursor, runtime_head);
    assert_eq!(diagnostics.schema.as_ref().unwrap().revision, 1);
    assert!(diagnostics.models.models.iter().any(|model| {
        model.id.as_str() == "document"
            && model.kind == rrd_contract::DiagnosticModelKind::Record
            && model.property_count == 1
            && model.required_property_count == 1
    }));
    assert_eq!(diagnostics.graph.valid_at_unix_ms, 1_100);
    assert_eq!(diagnostics.graph.known_at_cursor, 2);
    assert_eq!(diagnostics.graph.records.len(), 1);
    assert_eq!(
        diagnostics.graph.records[0].reference.kind.as_str(),
        "document"
    );
    assert_eq!(diagnostics.graph_difference.from_cursor, 0);
    assert_eq!(diagnostics.graph_difference.to_cursor, 2);
    assert_eq!(diagnostics.graph_difference.added_records.len(), 1);
    assert_eq!(diagnostics.retention.leases.len(), 1);
    assert_eq!(diagnostics.retention.pins.len(), 1);
    assert_eq!(
        diagnostics.retention.oldest_retained_cursor,
        Some(diagnostics.read.runtime_cursor)
    );
    assert_eq!(diagnostics.retention.leases[0].owner, "rust-sdk-fixture");
    assert_eq!(diagnostics.vector_artifacts.revision, 0);
    assert!(diagnostics.vector_artifacts.artifacts.is_empty());
    let impossible_graph_cursor = client
        .read_diagnostic_snapshot(
            &session,
            ReadDiagnosticSnapshot {
                scope: "instance:sdk-test".into(),
                graph_valid_at_unix_ms: 1_100,
                graph_known_at_cursor: Some(runtime_head.saturating_add(1)),
                graph_compare_cursor: 0,
                runtime_max_scanned_changes: 1_024,
                changes_after_cursor: 0,
                change_limit: 64,
                audit_after_sequence: 0,
                audit_limit: 32,
            },
            RequestOptions::read("request-diagnostics-future", "operation-diagnostics-future")
                .unwrap(),
        )
        .await;
    assert!(impossible_graph_cursor.is_err());
    assert_eq!(diagnostics.runtime_tools, runtime_catalogue);
    assert!(diagnostics
        .sections
        .iter()
        .any(|section| section.id.as_str() == "vector-collections"));

    let audit = client
        .read_audit(
            &session,
            ReadAudit {
                after_sequence: 0,
                limit: 32,
            },
            RequestOptions::read("request-audit", "operation-audit").unwrap(),
        )
        .await
        .unwrap();
    assert!(audit.records.iter().any(|record| {
        record.action == rrd_contract::SecurityAction::QueryExecute
            && record.phase == rrd_contract::AuditPhase::Completed
    }));
    assert!(audit.records.iter().any(|record| {
        record.action == rrd_contract::SecurityAction::MemoryContextRead
            && record.principal_id.as_ref().map(CanonicalId::as_str) == Some("rust-sdk")
            && record.decision == rrd_contract::AuditDecision::Denied
    }));

    assert!(matches!(
        RrdClient::connect_local(
            "192.0.2.1:9477".parse().unwrap(),
            CanonicalId::new("sdk-test").unwrap(),
            ClientConfig::default(),
        ),
        Err(Error::Contract(_))
    ));
    drop(client);
    proxy.await.unwrap();
    shutdown.send(()).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn remote_transport_requires_mutual_tls_and_exact_server_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("mtls-instance");
    let storage = PersistentEngine::open(&root).unwrap();
    let instance = CanonicalId::new("mtls-sdk-test").unwrap();
    let principal = Principal {
        id: CanonicalId::new("mtls-client").unwrap(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(b"mtls-api-key"),
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        grants: vec![ResourceGrant {
            action: Action::SessionCreate,
            resource_prefix: ResourcePath {
                segments: vec![ResourceId::new(ResourceKind::Instance, instance.as_str()).unwrap()],
            },
        }],
    };
    SecurityRepository::new(&storage, instance.clone())
        .initialize(
            SecurityState {
                format_version: SECURITY_FORMAT,
                revision: 1,
                principals: BTreeMap::from([(principal.id.clone(), principal)]),
            },
            1,
            "bootstrap",
            "request-bootstrap-mtls",
            "operation-bootstrap-mtls",
        )
        .unwrap();
    drop(storage);

    let (ca, issuer) = test_ca();
    let (server_chain, server_key) = test_identity(
        &issuer,
        vec!["localhost".into()],
        ExtendedKeyUsagePurpose::ServerAuth,
    );
    let (client_chain, client_key) =
        test_identity(&issuer, Vec::new(), ExtendedKeyUsagePurpose::ClientAuth);
    let server_tls = RrdMutualTlsServerConfig::new(server_chain, server_key, roots(&ca)).unwrap();
    let token_key = load_or_create_token_key(&root.join("RRD.SERVER.SECRET")).unwrap();
    let engine = RrdEngine::open(&root, instance.clone(), token_key).unwrap();
    let server =
        RrdHttpServer::bind_mtls(engine, "127.0.0.1:0".parse().unwrap(), server_tls).unwrap();
    let endpoint = format!("https://localhost:{}", server.local_addr().port());
    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(server.serve_until(async move {
        let _ = receiver.await;
    }));

    let anonymous_tls = RustlsClientConfig::builder()
        .with_root_certificates(roots(&ca))
        .with_no_client_auth();
    let anonymous = RrdClient::connect_mtls(
        endpoint.clone(),
        instance.clone(),
        anonymous_tls,
        ClientConfig::default(),
    )
    .unwrap();
    assert!(matches!(
        anonymous.capabilities().await,
        Err(Error::Transport(_))
    ));

    let (wrong_name_chain, wrong_name_key) =
        test_identity(&issuer, Vec::new(), ExtendedKeyUsagePurpose::ClientAuth);
    let wrong_name_tls = RustlsClientConfig::builder()
        .with_root_certificates(roots(&ca))
        .with_client_auth_cert(wrong_name_chain, wrong_name_key)
        .unwrap();
    let wrong_identity = RrdClient::connect_mtls(
        format!("https://127.0.0.1:{}", server_address_port(&endpoint)),
        instance.clone(),
        wrong_name_tls,
        ClientConfig::default(),
    )
    .unwrap();
    assert!(matches!(
        wrong_identity.capabilities().await,
        Err(Error::Transport(_))
    ));

    let client_tls = RustlsClientConfig::builder()
        .with_root_certificates(roots(&ca))
        .with_client_auth_cert(client_chain, client_key)
        .unwrap();
    let client =
        RrdClient::connect_mtls(endpoint, instance, client_tls, ClientConfig::default()).unwrap();
    let capabilities = client.capabilities().await.unwrap();
    assert_eq!(capabilities.protocol_version, 1);
    assert_eq!(
        capabilities
            .capabilities
            .iter()
            .find(|capability| capability.name.as_str() == "remote-listen")
            .unwrap()
            .status,
        rrd_contract::CapabilityStatus::Experimental
    );

    drop(client);
    drop(anonymous);
    shutdown.send(()).unwrap();
    task.await.unwrap().unwrap();
}

fn server_address_port(endpoint: &str) -> u16 {
    endpoint
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .expect("test TLS endpoint carries a port")
}
