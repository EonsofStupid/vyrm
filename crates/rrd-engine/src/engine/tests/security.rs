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

#[test]
fn diagnostic_snapshot_requires_its_exact_grant() {
    let (_root, engine) = isolated_engine();
    let principal_id = CanonicalId::new("diagnostic-denied").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance().as_str()).unwrap()],
    };
    SecurityRepository::new(&engine.storage, instance())
        .initialize(
            SecurityState {
                format_version: rrd_security::SECURITY_FORMAT,
                revision: 1,
                principals: [(
                    principal_id.clone(),
                    Principal {
                        id: principal_id.clone(),
                        kind: PrincipalKind::Service,
                        credential_sha256: digest::sha256_hex(b"diagnostic-key"),
                        not_before_unix_ms: 1,
                        expires_at_unix_ms: u64::MAX,
                        disabled: false,
                        grants: vec![ResourceGrant {
                            action: SecurityAction::SessionCreate,
                            resource_prefix: resource,
                        }],
                    },
                )]
                .into_iter()
                .collect(),
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
            b"diagnostic-key",
            &session_request(5_000, 2),
            &id("diagnostic-session"),
            1_000,
            "request-diagnostic-session",
            "operation-diagnostic-session",
        )
        .unwrap();

    let result = engine.read_diagnostic_snapshot(
        &lease.session_id,
        &lease.token,
        &rrd_contract::ReadDiagnosticSnapshot {
            scope: "instance:test-instance".into(),
            graph_valid_at_unix_ms: 1_100,
            graph_known_at_cursor: None,
            graph_compare_cursor: 0,
            runtime_max_scanned_changes: 1_024,
            changes_after_cursor: 0,
            change_limit: 8,
            audit_after_sequence: 0,
            audit_limit: 8,
        },
        1_100,
        "request-diagnostic-read",
        "operation-diagnostic-read",
    );

    assert!(matches!(result, Err(ServiceError::PermissionDenied)));
}

#[test]
fn engine_invocation_owns_authorization_and_completion_audit() {
    let (_root, engine) = isolated_engine();
    let principal_id = CanonicalId::new("query-reader").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance().as_str()).unwrap()],
    };
    let principal = Principal {
        id: principal_id.clone(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(b"query-key"),
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
                resource_prefix: resource.clone(),
            },
        ],
    };
    let repository = SecurityRepository::new(&engine.storage, instance());
    repository
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
            b"query-key",
            &session_request(5_000, 2),
            &id("create-query-session"),
            1_000,
            "request-create-query-session",
            "operation-create-query-session",
        )
        .unwrap();
    let invocation = Invocation {
        context: RequestContext {
            request_id: id("request-query"),
            operation_id: id("operation-query"),
            idempotency_key: None,
            deadline_unix_ms: Some(2_000),
        },
        resource,
        observed_at_unix_ms: 1_100,
        attempt: 1,
        request_sha256: digest::sha256_hex(b"query-request"),
    };
    let authorized = engine
        .begin_invocation(
            invocation,
            RrdOperation::QueryExecute,
            InvocationCredential::Session {
                session_id: &lease.session_id,
                token: &lease.token,
            },
        )
        .unwrap();
    engine
        .complete_invocation(
            &authorized,
            InvocationCompletion {
                decision: AuditDecision::Allowed,
                status_code: 200,
                response_sha256: digest::sha256_hex(b"query-response"),
            },
        )
        .unwrap();

    let page = repository.audit_since(0, 16).unwrap();
    assert_eq!(page.records.len(), 2);
    assert_eq!(page.records[0].1.phase, AuditPhase::Authorized);
    assert_eq!(page.records[1].1.phase, AuditPhase::Completed);
    assert_eq!(page.records[1].1.decision, AuditDecision::Allowed);
    assert_eq!(page.records[1].1.action, SecurityAction::QueryExecute);
    assert_eq!(page.records[1].1.principal_id, Some(principal_id));
}

#[test]
fn denied_engine_invocation_is_audited_without_domain_mutation() {
    let (_root, engine) = isolated_engine();
    let principal_id = CanonicalId::new("session-only").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance().as_str()).unwrap()],
    };
    let principal = Principal {
        id: principal_id.clone(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(b"session-key"),
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        grants: vec![ResourceGrant {
            action: SecurityAction::SessionCreate,
            resource_prefix: resource.clone(),
        }],
    };
    let repository = SecurityRepository::new(&engine.storage, instance());
    repository
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
            b"session-key",
            &session_request(5_000, 2),
            &id("create-session-only"),
            1_000,
            "request-create-session-only",
            "operation-create-session-only",
        )
        .unwrap();
    let before_runtime = engine.storage.runtime_cursor().unwrap();
    let denied = engine.begin_invocation(
        Invocation {
            context: mutation_context(
                &id("backup-idempotency"),
                "request-denied-backup",
                "operation-denied-backup",
            ),
            resource,
            observed_at_unix_ms: 1_100,
            attempt: 2,
            request_sha256: digest::sha256_hex(b"backup-request"),
        },
        RrdOperation::BackupCreate,
        InvocationCredential::Session {
            session_id: &lease.session_id,
            token: &lease.token,
        },
    );
    assert!(matches!(denied, Err(ServiceError::PermissionDenied)));
    assert_eq!(engine.storage.runtime_cursor().unwrap(), before_runtime);

    let page = repository.audit_since(0, 16).unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].1.phase, AuditPhase::Completed);
    assert_eq!(page.records[0].1.decision, AuditDecision::Denied);
    assert_eq!(page.records[0].1.action, SecurityAction::BackupCreate);
}
