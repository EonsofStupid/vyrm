use rrd_contract::{
    runtime_tool_arguments_sha256, AuditDecision, AuditPhase, CanonicalId, CorrelationId,
    CreateSession, RequestContext, ResourceId, ResourceKind, ResourcePath, RuntimeToolInvocation,
    SecurityAction, SessionLimits, RUNTIME_TOOL_CATALOGUE_VERSION,
};
use rrd_core::digest;
use rrd_engine::{Invocation, InvocationCredential, RrdEngine, ServiceError};
use rrd_security::{Principal, PrincipalKind, ResourceGrant, SecurityRepository, SecurityState};
use rrd_store::PersistentEngine;
use serde_json::{json, Value};

fn id(value: &str) -> CanonicalId {
    CanonicalId::new(value).unwrap()
}

fn resource(instance: &CanonicalId) -> ResourcePath {
    ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance.as_str()).unwrap()],
    }
}

fn tool_request(name: &str, arguments: Value) -> RuntimeToolInvocation {
    RuntimeToolInvocation {
        catalogue_version: RUNTIME_TOOL_CATALOGUE_VERSION,
        tool: id(name),
        arguments_sha256: runtime_tool_arguments_sha256(&arguments).unwrap(),
        arguments,
    }
}

fn invocation(
    instance: &CanonicalId,
    request: &RuntimeToolInvocation,
    operation: &str,
    at: u64,
) -> Invocation {
    Invocation {
        context: RequestContext {
            request_id: CorrelationId::new(format!("request-{operation}")).unwrap(),
            operation_id: CorrelationId::new(format!("operation-{operation}")).unwrap(),
            idempotency_key: None,
            deadline_unix_ms: Some(at + 10_000),
        },
        resource: resource(instance),
        observed_at_unix_ms: at,
        attempt: 1,
        request_sha256: digest::sha256_hex(&serde_json::to_vec(request).unwrap()),
    }
}

#[test]
fn runtime_dispatch_preserves_api_key_and_session_principals_through_nested_execution_and_audit() {
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("rrd");
    let project = temporary.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let instance = id("caller-runtime");
    let principal_id = id("runtime-agent");
    let credential = b"runtime-secret";
    let instance_resource = resource(&instance);
    let principal = Principal {
        id: principal_id.clone(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(credential),
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        grants: [
            SecurityAction::SessionCreate,
            SecurityAction::QueryIndexList,
            SecurityAction::MemoryContextRead,
        ]
        .into_iter()
        .map(|action| ResourceGrant {
            action,
            resource_prefix: instance_resource.clone(),
        })
        .collect(),
    };
    let storage = PersistentEngine::open(&database).unwrap();
    SecurityRepository::new(&storage, instance.clone())
        .initialize(
            SecurityState {
                format_version: rrd_security::SECURITY_FORMAT,
                revision: 1,
                principals: [(principal_id.clone(), principal)].into_iter().collect(),
            },
            1,
            "test:runtime-caller",
            "request-security-init",
            "operation-security-init",
        )
        .unwrap();
    drop(storage);

    let engine = RrdEngine::open(&database, instance.clone(), [19; 32]).unwrap();
    let query = tool_request(
        "rrflow_query_index_list",
        json!({"scope":"instance:caller-runtime"}),
    );
    let query_result = engine
        .invoke_runtime_tool(
            &project,
            &query,
            invocation(&instance, &query, "query", 10),
            InvocationCredential::ApiKey {
                principal_id: &principal_id,
                credential,
            },
        )
        .unwrap();
    query_result.validate().unwrap();

    let lease = engine
        .create_authenticated_session(
            &principal_id,
            credential,
            &CreateSession {
                limits: SessionLimits {
                    idle_timeout_ms: 60_000,
                    absolute_timeout_ms: 60_000,
                    max_open_transactions: 1,
                },
            },
            &CorrelationId::new("caller-session-key").unwrap(),
            20,
            "request-session",
            "operation-session",
        )
        .unwrap();
    let context = tool_request("rrflow_context", json!({"at":21,"max_subjects":1}));
    let context_result = engine
        .invoke_runtime_tool(
            &project,
            &context,
            invocation(&instance, &context, "context", 21),
            InvocationCredential::Session {
                session_id: &lease.session_id,
                token: &lease.token,
            },
        )
        .unwrap();
    context_result.validate().unwrap();

    let denied = tool_request(
        "rrflow_vector_collection_list",
        json!({"scope":"instance:caller-runtime"}),
    );
    assert!(matches!(
        engine.invoke_runtime_tool(
            &project,
            &denied,
            invocation(&instance, &denied, "vector-denied", 22),
            InvocationCredential::Session {
                session_id: &lease.session_id,
                token: &lease.token,
            },
        ),
        Err(ServiceError::PermissionDenied)
    ));
    drop(engine);

    let storage = PersistentEngine::open(&database).unwrap();
    let page = SecurityRepository::new(&storage, instance)
        .audit_since(0, 100)
        .unwrap();
    for action in [
        SecurityAction::QueryIndexList,
        SecurityAction::MemoryContextRead,
    ] {
        let records = page
            .records
            .iter()
            .filter(|(_, record)| record.action == action)
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        assert_eq!(
            records.len(),
            2,
            "expected authorize and completion for {action:?}"
        );
        assert_eq!(records[0].phase, AuditPhase::Authorized);
        assert_eq!(records[1].phase, AuditPhase::Completed);
        assert_eq!(records[1].decision, AuditDecision::Allowed);
        assert!(records
            .iter()
            .all(|record| record.principal_id.as_ref() == Some(&principal_id)));
    }
    assert!(page.records.iter().any(|(_, record)| {
        record.action == SecurityAction::VectorCollectionList
            && record.phase == AuditPhase::Completed
            && record.decision == AuditDecision::Denied
            && record.principal_id.as_ref() == Some(&principal_id)
    }));
}

#[test]
fn runtime_invocation_rejects_payload_digest_drift_before_execution() {
    let temporary = tempfile::tempdir().unwrap();
    let instance = id("digest-runtime");
    let engine =
        RrdEngine::open(&temporary.path().join("rrd"), instance.clone(), [23; 32]).unwrap();
    let mut request = tool_request("rrflow_service_status", json!({"at":10}));
    let invocation = invocation(&instance, &request, "status", 10);
    request.arguments["at"] = json!(11);

    assert!(matches!(
        engine.invoke_runtime_tool(
            temporary.path(),
            &request,
            invocation,
            InvocationCredential::Anonymous,
        ),
        Err(ServiceError::Contract(_))
    ));
}
