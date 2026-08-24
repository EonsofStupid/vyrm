use rrd_client::{ClientConfig, Error, RequestOptions, RrdClient, is_unauthenticated};
use rrd_contract::{
    AbortTransaction, BeginTransaction, CanonicalId, CreateSession, ExecuteQuery,
    PreviewTransaction, QueryBudget, ReadAudit, ReadChangefeed, ResourceId, ResourceKind,
    ResourcePath, SessionLimits, TransactionMutation,
};
use rrd_security::{
    Action, Principal, PrincipalKind, ResourceGrant, SECURITY_FORMAT, SecurityRepository,
    SecurityState,
};
use rrd_server::{RrdHttpServer, load_or_create_token_key};
use std::collections::BTreeMap;
use std::time::Duration;
use vyrm_core::{
    RuntimeCommit, RuntimeProperties, RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema,
    RuntimeRef, RuntimeSchemaRegistry, RuntimeType, RuntimeValue, RuntimeValueType, ScopeId,
    digest,
};
use vyrm_store::{Engine, PersistentEngine};

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
                vyrm_core::RuntimeMutation::Schema { registry },
                vyrm_core::RuntimeMutation::Record {
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
    let root = temporary.path().join("instance");
    let engine = PersistentEngine::open(&root).unwrap();
    seed(&engine);
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
        ]
        .into_iter()
        .map(|action| ResourceGrant {
            action,
            resource_prefix: resource.clone(),
        })
        .collect(),
    };
    SecurityRepository::new(&engine, instance.clone())
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
    let token_key = load_or_create_token_key(&root.join("RRD.SERVER.SECRET")).unwrap();
    let server = RrdHttpServer::bind(
        engine,
        instance.clone(),
        token_key,
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
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
    let catalogue = client.endpoint_catalogue().await.unwrap();
    assert_eq!(catalogue.endpoints.len(), 26);
    let openapi = client.openapi_document().await.unwrap();
    assert_eq!(openapi["x-rrd-endpoint-count"], 26);

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
                limit: 8,
            },
            RequestOptions::read("request-changes", "operation-changes").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(changes.through_cursor, 2);
    assert_eq!(changes.changes.len(), 2);

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
