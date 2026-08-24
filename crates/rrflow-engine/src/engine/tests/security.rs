use super::*;

#[test]
fn embedded_engine_cannot_bypass_policy_grants() {
    let (_root, engine) = isolated_engine();
    let principal_id = CanonicalId::new("backup-denied").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance().as_str()).unwrap()],
    };
    let principal = Principal {
        id: principal_id.clone(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(b"session-only-key"),
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        grants: vec![ResourceGrant {
            action: SecurityAction::SessionCreate,
            resource_prefix: resource,
        }],
    };
    SecurityRepository::new(&engine.storage, instance())
        .initialize(
            SecurityState {
                format_version: rrd_security::SECURITY_FORMAT,
                revision: 1,
                principals: [(principal_id.clone(), principal)].into_iter().collect(),
            },
            1,
            "security-test",
            "request-security",
            "operation-security",
        )
        .unwrap();

    let lease = engine
        .create_authenticated_session(
            &principal_id,
            b"session-only-key",
            &session_request(5_000, 2),
            &id("create-key"),
            1_000,
            "request-create",
            "operation-create",
        )
        .unwrap();
    let before = engine.storage.control_journal_since(0, 64).unwrap();

    let denied = engine.create_instance_backup(
        &lease.session_id,
        &lease.token,
        &id("backup-key"),
        &rrd_contract::CreateInstanceBackup {
            label: "must-not-exist".into(),
            created_at_unix_ms: 1_100,
        },
        1_100,
        "request-backup",
        "operation-backup",
    );

    assert!(matches!(denied, Err(ServiceError::PermissionDenied)));
    let after = engine.storage.control_journal_since(0, 64).unwrap();
    assert_eq!(
        after, before,
        "a denied embedded call must not prepare a backup"
    );
}
