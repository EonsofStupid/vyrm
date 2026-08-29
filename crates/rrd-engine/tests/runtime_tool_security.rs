use rrd_contract::{CanonicalId, ResourceId, ResourceKind, ResourcePath, SecurityAction};
use rrd_core::digest;
use rrd_engine::{RrdEngine, ServiceError};
use rrd_security::{Principal, PrincipalKind, ResourceGrant, SecurityRepository, SecurityState};
use rrd_store::PersistentEngine;

#[test]
fn initialized_security_denies_governed_runtime_tools_but_keeps_public_status_public() {
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("rrd");
    let instance = CanonicalId::new("runtime-security").unwrap();
    let storage = PersistentEngine::open(&database).unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance.as_str()).unwrap()],
    };
    let principal = Principal {
        id: CanonicalId::new("runtime-agent").unwrap(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(b"runtime-secret"),
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: Default::default(),
        grants: vec![ResourceGrant {
            action: SecurityAction::QueryExecute,
            resource_prefix: resource,
            data_policy: None,
        }],
    };
    SecurityRepository::new(&storage, instance.clone())
        .initialize(
            SecurityState {
                format_version: rrd_security::SECURITY_FORMAT,
                revision: 1,
                principals: [(principal.id.clone(), principal)].into_iter().collect(),
                roles: Default::default(),
                identity_bindings: Default::default(),
                jwt_issuers: Default::default(),
            },
            1,
            "test:security",
            "request-security",
            "operation-security",
        )
        .unwrap();
    drop(storage);

    let engine = RrdEngine::open(&database, instance, [7; 32]).unwrap();
    let denied = engine.call_runtime_tool(
        temporary.path(),
        "rrflow_query",
        &serde_json::json!({"ql":"FROM event:any"}),
        2,
    );
    assert!(matches!(denied, Err(ServiceError::Unauthenticated)));
    let denied_commit = engine.call_runtime_tool(
        temporary.path(),
        "rrflow_data_commit",
        &serde_json::json!({
            "idempotency_key":"security-denied-commit",
            "mutations":[{
                "mutation":"assert_claim",
                "subject":"security",
                "predicate":"status",
                "object":"must-not-commit",
                "valid_from":2,
                "tx_time":2,
                "producer":"runtime-test"
            }]
        }),
        2,
    );
    assert!(matches!(denied_commit, Err(ServiceError::Unauthenticated)));
    let denied_index_list = engine.call_runtime_tool(
        temporary.path(),
        "rrflow_query_index_list",
        &serde_json::json!({"scope":"instance:runtime-security"}),
        2,
    );
    assert!(matches!(
        denied_index_list,
        Err(ServiceError::Unauthenticated)
    ));
    let denied_live = engine.call_runtime_tool(
        temporary.path(),
        "rrflow_live_query_poll",
        &serde_json::json!({
            "scope":"instance:runtime-security",
            "query":"FROM record:document AT VALID 1 KNOWN HEAD PROJECT id",
            "after_cursor":0,
            "max_delta_rows":10
        }),
        2,
    );
    assert!(matches!(denied_live, Err(ServiceError::Unauthenticated)));
    for (name, arguments) in [
        (
            "rrflow_changefeed_read",
            serde_json::json!({
                "scope":"instance:runtime-security",
                "after_cursor":0,
                "limit":10
            }),
        ),
        (
            "rrflow_changefeed_follow",
            serde_json::json!({
                "read":{
                    "scope":"instance:runtime-security",
                    "after_cursor":0,
                    "limit":10
                },
                "wait_timeout_ms":1
            }),
        ),
    ] {
        assert!(matches!(
            engine.call_runtime_tool(temporary.path(), name, &arguments, 2),
            Err(ServiceError::Unauthenticated)
        ));
    }
    for (name, arguments) in [
        (
            "rrflow_vector_collection_ensure",
            serde_json::json!({
                "idempotency_key":"security-vector-ensure",
                "scope":"instance:runtime-security",
                "collection_id":"documents",
                "vectors":[{
                    "name":"title",
                    "field":"title-embedding",
                    "kind":"dense",
                    "dimensions":2,
                    "metric":"cosine",
                    "memory_tier":"cached"
                }]
            }),
        ),
        (
            "rrflow_vector_collection_list",
            serde_json::json!({"scope":"instance:runtime-security"}),
        ),
        (
            "rrflow_vector_points_retrieve",
            serde_json::json!({
                "scope":"instance:runtime-security",
                "collection_id":"documents",
                "vector_name":"title",
                "valid_at":1,
                "references":[{"kind":"embedding","id":"one"}],
                "max_scanned_changes":100
            }),
        ),
        (
            "rrflow_vector_points_scroll",
            serde_json::json!({
                "scope":"instance:runtime-security",
                "collection_id":"documents",
                "vector_name":"title",
                "valid_at":1,
                "limit":10,
                "max_scanned_changes":100
            }),
        ),
        (
            "rrflow_vector_search",
            serde_json::json!({
                "scope":"instance:runtime-security",
                "valid_at":1,
                "collection_id":"documents",
                "vector_name":"title",
                "query":{"kind":"dense","values":[1.0,0.0]},
                "top_k":10,
                "max_scanned_changes":100
            }),
        ),
        (
            "rrflow_backup_create",
            serde_json::json!({
                "idempotency_key":"security-backup-create",
                "label":"security-backup",
                "created_at_unix_ms":2
            }),
        ),
        (
            "rrflow_backup_list",
            serde_json::json!({"verify_archives":false}),
        ),
        (
            "rrflow_restore",
            serde_json::json!({
                "idempotency_key":"security-restore",
                "confirmation":"restore_to_new_root",
                "backup_sha256":"11".repeat(32),
                "restore_id":"security-restore",
                "restored_at_unix_ms":2
            }),
        ),
        (
            "rrflow_estate_read",
            serde_json::json!({"estate_id":"security-estate"}),
        ),
        (
            "rrflow_audit_read",
            serde_json::json!({"after_sequence":0,"limit":10}),
        ),
    ] {
        assert!(matches!(
            engine.call_runtime_tool(temporary.path(), name, &arguments, 2),
            Err(ServiceError::Unauthenticated)
        ));
    }

    let public = engine
        .call_runtime_tool(
            temporary.path(),
            "rrflow_service_status",
            &serde_json::json!({"at":2}),
            2,
        )
        .unwrap();
    let status: serde_json::Value = serde_json::from_str(&public.text).unwrap();
    assert_eq!(status["security_enforced"], true);
    assert_eq!(status["instance_id"], "runtime-security");
}
