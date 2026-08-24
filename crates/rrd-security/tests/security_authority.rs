use rrd_contract::{CanonicalId, ResourceId, ResourceKind, ResourcePath};
use rrd_security::{
    Action, AuditDecision, AuditRecord, Error, Principal, PrincipalKind, ResourceGrant,
    SECURITY_FORMAT, SecurityRepository, SecurityState,
};
use std::collections::BTreeMap;
use vyrm_core::digest;
use vyrm_store::{Engine, NativeEngine};

fn path(instance: &str) -> ResourcePath {
    ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance).unwrap()],
    }
}

fn state(secret: &[u8]) -> SecurityState {
    let principal = Principal {
        id: CanonicalId::new("connectome-local").unwrap(),
        kind: PrincipalKind::User,
        credential_sha256: digest::sha256_hex(secret),
        not_before_unix_ms: 1_000,
        expires_at_unix_ms: 10_000,
        disabled: false,
        grants: vec![
            ResourceGrant {
                action: Action::QueryExecute,
                resource_prefix: path("alpha"),
            },
            ResourceGrant {
                action: Action::AuditRead,
                resource_prefix: path("alpha"),
            },
        ],
    };
    SecurityState {
        format_version: SECURITY_FORMAT,
        revision: 1,
        principals: BTreeMap::from([(principal.id.clone(), principal)]),
    }
}

#[test]
fn policy_is_persistent_exact_scope_and_deny_by_default() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("security");
    let engine = NativeEngine::open(&root).unwrap();
    let repository = SecurityRepository::new(&engine, CanonicalId::new("alpha").unwrap());
    repository
        .initialize(
            state(b"correct horse battery staple"),
            1_000,
            "bootstrap",
            "request-bootstrap",
            "operation-bootstrap",
        )
        .unwrap();

    let authorization = repository
        .authenticate_and_authorize(
            &CanonicalId::new("connectome-local").unwrap(),
            b"correct horse battery staple",
            Action::QueryExecute,
            &path("alpha"),
            2_000,
        )
        .unwrap();
    assert_eq!(authorization.policy_revision, 1);
    assert!(matches!(
        repository.authenticate_and_authorize(
            &CanonicalId::new("connectome-local").unwrap(),
            b"wrong",
            Action::QueryExecute,
            &path("alpha"),
            2_000,
        ),
        Err(Error::Unauthenticated)
    ));
    assert!(matches!(
        repository.authenticate_and_authorize(
            &CanonicalId::new("connectome-local").unwrap(),
            b"correct horse battery staple",
            Action::BackupCreate,
            &path("alpha"),
            2_000,
        ),
        Err(Error::PermissionDenied)
    ));
    drop(engine);

    let engine = NativeEngine::open(&root).unwrap();
    let repository = SecurityRepository::new(&engine, CanonicalId::new("alpha").unwrap());
    assert_eq!(repository.load().unwrap().unwrap().revision, 1);
    assert!(
        repository
            .authenticate_and_authorize(
                &CanonicalId::new("connectome-local").unwrap(),
                b"correct horse battery staple",
                Action::QueryExecute,
                &path("other"),
                2_000,
            )
            .is_err()
    );
}

#[test]
fn audit_is_redacted_idempotent_authenticated_and_replayable() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("security");
    let engine = NativeEngine::open(&root).unwrap();
    let repository = SecurityRepository::new(&engine, CanonicalId::new("alpha").unwrap());
    repository
        .initialize(
            state(b"do-not-journal-this-secret"),
            1_000,
            "bootstrap",
            "request-bootstrap",
            "operation-bootstrap",
        )
        .unwrap();
    let record = AuditRecord {
        audit_id: CanonicalId::new("audit-query-1").unwrap(),
        at_unix_ms: 2_000,
        principal_id: Some(CanonicalId::new("connectome-local").unwrap()),
        action: Action::QueryExecute,
        resource: path("alpha"),
        request_id: "request-query".into(),
        operation_id: "operation-query".into(),
        decision: AuditDecision::Allowed,
        status_code: 200,
        request_sha256: digest::sha256_hex(b"redacted request"),
        response_sha256: digest::sha256_hex(b"redacted response"),
    };
    repository.append_audit(&record).unwrap();
    repository.append_audit(&record).unwrap();
    let mut collision = record.clone();
    collision.status_code = 500;
    assert!(matches!(
        repository.append_audit(&collision),
        Err(Error::IdempotencyConflict)
    ));
    drop(engine);

    let engine = NativeEngine::open(&root).unwrap();
    let repository = SecurityRepository::new(&engine, CanonicalId::new("alpha").unwrap());
    let records = repository.audit_since(0, 10).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].1, record);
    let journal = engine.control_journal_since(0, 10).unwrap();
    let encoded = serde_json::to_string(&journal).unwrap();
    assert!(!encoded.contains("do-not-journal-this-secret"));
    assert!(journal.iter().all(|entry| entry.verify()));
}
