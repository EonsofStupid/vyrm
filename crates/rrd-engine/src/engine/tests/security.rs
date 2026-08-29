use super::*;
use rrd_contract::{
    DataPropertySchema, DataRecordSchema, DataReference, DataSchemaRegistry, DataValueType,
    ExecuteQuery, QueryBudget, QueryValue,
};
use rrd_core::RuntimeValue;
use rrd_security::{
    DataPolicy, IdentityBinding, JwtIssueRequest, JwtIssuer, PolicyPredicate, Role,
};
use std::collections::{BTreeMap, BTreeSet};

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
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: Default::default(),
        grants: vec![ResourceGrant {
            action: SecurityAction::SessionCreate,
            resource_prefix: resource,
            data_policy: None,
        }],
    };
    SecurityRepository::new(&engine.storage, instance())
        .initialize(
            SecurityState {
                format_version: rrd_security::SECURITY_FORMAT,
                revision: 1,
                principals: [(principal_id.clone(), principal)].into_iter().collect(),
                roles: Default::default(),
                identity_bindings: Default::default(),
                jwt_issuers: Default::default(),
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
                        credential_revision: 1,
                        not_before_unix_ms: 1,
                        expires_at_unix_ms: u64::MAX,
                        disabled: false,
                        role_ids: Default::default(),
                        grants: vec![ResourceGrant {
                            action: SecurityAction::SessionCreate,
                            resource_prefix: resource,
                            data_policy: None,
                        }],
                    },
                )]
                .into_iter()
                .collect(),
                roles: Default::default(),
                identity_bindings: Default::default(),
                jwt_issuers: Default::default(),
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
                data_policy: None,
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
                roles: Default::default(),
                identity_bindings: Default::default(),
                jwt_issuers: Default::default(),
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
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: Default::default(),
        grants: vec![ResourceGrant {
            action: SecurityAction::SessionCreate,
            resource_prefix: resource.clone(),
            data_policy: None,
        }],
    };
    let repository = SecurityRepository::new(&engine.storage, instance());
    repository
        .initialize(
            SecurityState {
                format_version: rrd_security::SECURITY_FORMAT,
                revision: 1,
                principals: [(principal_id.clone(), principal)].into_iter().collect(),
                roles: Default::default(),
                identity_bindings: Default::default(),
                jwt_issuers: Default::default(),
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

#[test]
fn engine_identity_paths_share_one_revisioned_authority_across_reopen() {
    let (root, engine) = isolated_engine();
    let admin_id = CanonicalId::new("security-admin").unwrap();
    let service_id = CanonicalId::new("external-service").unwrap();
    let issuer_id = CanonicalId::new("engine-issuer").unwrap();
    let binding_id = CanonicalId::new("engine-binding").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance().as_str()).unwrap()],
    };
    let grant = |action| ResourceGrant {
        action,
        resource_prefix: resource.clone(),
        data_policy: None,
    };
    let admin = Principal {
        id: admin_id.clone(),
        kind: PrincipalKind::User,
        credential_sha256: digest::sha256_hex(b"admin-key"),
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: BTreeSet::new(),
        grants: vec![
            grant(SecurityAction::SessionCreate),
            grant(SecurityAction::SecurityAdmin),
        ],
    };
    let service = Principal {
        id: service_id.clone(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(b"service-key"),
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: BTreeSet::new(),
        grants: vec![
            grant(SecurityAction::SessionCreate),
            grant(SecurityAction::QueryExecute),
        ],
    };
    let authority = SecurityState {
        format_version: rrd_security::SECURITY_FORMAT,
        revision: 1,
        principals: BTreeMap::from([(admin_id.clone(), admin), (service_id.clone(), service)]),
        roles: BTreeMap::new(),
        identity_bindings: BTreeMap::from([(
            binding_id.clone(),
            IdentityBinding {
                id: binding_id,
                issuer: "https://identity.example".into(),
                subject: "workload-42".into(),
                audience: "rrd-engine".into(),
                principal_id: service_id.clone(),
                disabled: false,
            },
        )]),
        jwt_issuers: BTreeMap::from([(
            issuer_id.clone(),
            JwtIssuer {
                id: issuer_id.clone(),
                issuer: "https://rrd.example".into(),
                audience: "rrd-engine".into(),
                key_id: CanonicalId::new("engine-key-1").unwrap(),
                signing_key_sha256: digest::sha256_hex(b"engine-jwt-signing-key-material"),
                not_before_unix_ms: 1,
                expires_at_unix_ms: u64::MAX,
                disabled: false,
            },
        )]),
    };
    SecurityRepository::new(&engine.storage, instance())
        .initialize(
            authority,
            1,
            "security-test",
            "request-engine-identity-bootstrap",
            "operation-engine-identity-bootstrap",
        )
        .unwrap();
    let admin_lease = engine
        .create_authenticated_session(
            &admin_id,
            b"admin-key",
            &session_request(5_000, 2),
            &id("admin-session"),
            100,
            "request-admin-session",
            "operation-admin-session",
        )
        .unwrap();
    let jwt = engine
        .issue_principal_jwt(
            &service_id,
            b"service-key",
            b"engine-jwt-signing-key-material",
            &JwtIssueRequest {
                issuer_id,
                token_id: CanonicalId::new("engine-token-1").unwrap(),
                issued_at_unix_ms: 100,
                not_before_unix_ms: 100,
                expires_at_unix_ms: 5_000,
            },
        )
        .unwrap();
    let jwt_lease = engine
        .create_jwt_authenticated_session(
            &jwt.token,
            b"engine-jwt-signing-key-material",
            &session_request(5_000, 2),
            &id("jwt-session"),
            110,
            "request-jwt-session",
            "operation-jwt-session",
        )
        .unwrap();
    let identity_lease = engine
        .create_verified_identity_session(
            "https://identity.example",
            "workload-42",
            "rrd-engine",
            &session_request(5_000, 2),
            &id("external-session"),
            111,
            "request-external-session",
            "operation-external-session",
        )
        .unwrap();

    let repository = SecurityRepository::new(&engine.storage, instance());
    let mut rotated = repository.load().unwrap().unwrap();
    rotated.revision = 2;
    let service = rotated.principals.get_mut(&service_id).unwrap();
    service.credential_revision = 2;
    service.credential_sha256 = digest::sha256_hex(b"rotated-service-key");
    engine
        .replace_security_authority(
            &admin_lease.session_id,
            &admin_lease.token,
            1,
            rotated.clone(),
            200,
            "request-rotate-service",
            "operation-rotate-service",
        )
        .unwrap();
    engine
        .replace_security_authority(
            &admin_lease.session_id,
            &admin_lease.token,
            1,
            rotated,
            200,
            "request-rotate-service",
            "operation-rotate-service",
        )
        .unwrap();
    drop(engine);

    let reopened = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    let query_invocation = |request_id: &str| Invocation {
        context: RequestContext {
            request_id: id(request_id),
            operation_id: id("operation-revoked-query"),
            idempotency_key: None,
            deadline_unix_ms: Some(1_000),
        },
        resource: resource.clone(),
        observed_at_unix_ms: 210,
        attempt: 1,
        request_sha256: digest::sha256_hex(b"revoked-query"),
    };
    for (lease, request_id) in [
        (&jwt_lease, "request-revoked-jwt-lease"),
        (&identity_lease, "request-revoked-external-lease"),
    ] {
        let denied = reopened.begin_invocation(
            query_invocation(request_id),
            RrdOperation::QueryExecute,
            InvocationCredential::Session {
                session_id: &lease.session_id,
                token: &lease.token,
            },
        );
        assert!(
            matches!(denied, Err(ServiceError::Unauthenticated)),
            "rotated credential must invalidate {request_id}: {denied:?}"
        );
    }
    assert!(matches!(
        reopened.create_jwt_authenticated_session(
            &jwt.token,
            b"engine-jwt-signing-key-material",
            &session_request(5_000, 2),
            &id("rejected-old-jwt"),
            210,
            "request-rejected-old-jwt",
            "operation-rejected-old-jwt",
        ),
        Err(ServiceError::Unauthenticated)
    ));
    reopened
        .create_authenticated_session(
            &service_id,
            b"rotated-service-key",
            &session_request(5_000, 2),
            &id("rotated-service-session"),
            211,
            "request-rotated-service-session",
            "operation-rotated-service-session",
        )
        .unwrap();
    let persisted = SecurityRepository::new(&reopened.storage, instance())
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(persisted.revision, 2);
    assert_eq!(persisted.principals[&service_id].credential_revision, 2);
}

#[test]
fn compiled_role_policy_injects_tenant_rows_and_fields_before_query_planning() {
    let (_root, engine) = isolated_engine();
    let principal_id = CanonicalId::new("tenant-alpha-reader").unwrap();
    let base_role_id = CanonicalId::new("data-session-writer").unwrap();
    let reader_role_id = CanonicalId::new("tenant-alpha-viewer").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance().as_str()).unwrap()],
    };
    let base_role = Role {
        id: base_role_id.clone(),
        inherits: BTreeSet::new(),
        grants: [
            SecurityAction::SessionCreate,
            SecurityAction::TransactionBegin,
            SecurityAction::TransactionCommit,
        ]
        .into_iter()
        .map(|action| ResourceGrant {
            action,
            resource_prefix: resource.clone(),
            data_policy: None,
        })
        .collect(),
    };
    let reader_role = Role {
        id: reader_role_id.clone(),
        inherits: BTreeSet::from([base_role_id.clone()]),
        grants: vec![ResourceGrant {
            action: SecurityAction::QueryExecute,
            resource_prefix: resource.clone(),
            data_policy: Some(DataPolicy {
                tenant: Some(PolicyPredicate {
                    field: "tenant_id".into(),
                    value: RuntimeValue::String("tenant-alpha".into()),
                }),
                rows: vec![PolicyPredicate {
                    field: "classification".into(),
                    value: RuntimeValue::String("public".into()),
                }],
                allowed_fields: Some(BTreeSet::from(["body".into(), "id".into()])),
            }),
        }],
    };
    let principal = Principal {
        id: principal_id.clone(),
        kind: PrincipalKind::User,
        credential_sha256: digest::sha256_hex(b"tenant-alpha-key"),
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: BTreeSet::from([reader_role_id.clone()]),
        grants: Vec::new(),
    };
    SecurityRepository::new(&engine.storage, instance())
        .initialize(
            SecurityState {
                format_version: rrd_security::SECURITY_FORMAT,
                revision: 1,
                principals: BTreeMap::from([(principal_id.clone(), principal)]),
                roles: BTreeMap::from([(base_role_id, base_role), (reader_role_id, reader_role)]),
                identity_bindings: BTreeMap::new(),
                jwt_issuers: BTreeMap::new(),
            },
            1,
            "security-test",
            "request-tenant-policy-bootstrap",
            "operation-tenant-policy-bootstrap",
        )
        .unwrap();
    let lease = engine
        .create_authenticated_session(
            &principal_id,
            b"tenant-alpha-key",
            &session_request(9_000, 2),
            &id("tenant-policy-session"),
            10,
            "request-tenant-policy-session",
            "operation-tenant-policy-session",
        )
        .unwrap();
    let transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &BeginTransaction {
                scope: CanonicalId::new("data").unwrap(),
                timeout_ms: 1_000,
            },
            &mutation_context(
                &id("tenant-policy-begin"),
                "request-tenant-policy-begin",
                "operation-tenant-policy-begin",
            ),
            20,
        )
        .unwrap();
    let property = |value_type| DataPropertySchema {
        value_type,
        required: true,
    };
    let record = |id: &str, tenant: &str, classification: &str, body: &str, secret: &str| {
        TransactionMutation::PutRecord {
            reference: DataReference {
                kind: CanonicalId::new("document").unwrap(),
                id: CanonicalId::new(id).unwrap(),
            },
            valid_from: 20,
            valid_to: None,
            properties: BTreeMap::from([
                ("body".into(), QueryValue::String(body.into())),
                (
                    "classification".into(),
                    QueryValue::String(classification.into()),
                ),
                ("secret".into(), QueryValue::String(secret.into())),
                ("tenant_id".into(), QueryValue::String(tenant.into())),
            ]),
        }
    };
    let mutations = vec![
        TransactionMutation::PutSchema {
            registry: DataSchemaRegistry {
                revision: 1,
                migration: "install tenant policy fixture".into(),
                catalogue: DataCatalogueIdentity::default(),
                tables: BTreeMap::new(),
                records: BTreeMap::from([(
                    CanonicalId::new("document").unwrap(),
                    DataRecordSchema {
                        properties: BTreeMap::from([
                            ("body".into(), property(DataValueType::String)),
                            ("classification".into(), property(DataValueType::String)),
                            ("secret".into(), property(DataValueType::String)),
                            ("tenant_id".into(), property(DataValueType::String)),
                        ]),
                        ..DataRecordSchema::default()
                    },
                )]),
                relations: BTreeMap::new(),
                events: BTreeMap::new(),
            },
        },
        record(
            "alpha-public",
            "tenant-alpha",
            "public",
            "visible",
            "alpha-secret",
        ),
        record(
            "alpha-private",
            "tenant-alpha",
            "private",
            "hidden",
            "private-secret",
        ),
        record(
            "beta-public",
            "tenant-beta",
            "public",
            "other",
            "beta-secret",
        ),
    ];
    engine
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("tenant-policy-commit"),
            &CommitTransaction {
                operation_sha256: rrd_contract::transaction_operation_sha256(&mutations),
                mutations,
            },
            20,
            "request-tenant-policy-commit",
            "operation-tenant-policy-commit",
        )
        .unwrap();
    let query = ExecuteQuery {
        scope: format!("instance:{}", instance()),
        query: "FROM record:document AT VALID 20 KNOWN HEAD PROJECT *".into(),
        parameters: BTreeMap::new(),
        budget: QueryBudget::default(),
    };
    let result = engine
        .execute_query(
            &lease.session_id,
            &lease.token,
            &query,
            30,
            "request-tenant-query",
            "operation-tenant-query",
        )
        .unwrap();
    assert_eq!(result.plan.security_policy_revision, 1);
    assert_eq!(result.plan.authorization_sha256.len(), 64);
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].identity, "record:document:alpha-public");
    assert_eq!(
        result.rows[0]
            .values
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["body".into(), "id".into()])
    );
    assert!(result
        .canonical_query
        .contains("tenant_id = \"tenant-alpha\""));
    assert!(result
        .canonical_query
        .contains("classification = \"public\""));
    let denied = engine.execute_query(
        &lease.session_id,
        &lease.token,
        &ExecuteQuery {
            query: "FROM record:document AT VALID 20 KNOWN HEAD PROJECT secret".into(),
            ..query
        },
        31,
        "request-secret-query",
        "operation-secret-query",
    );
    assert!(matches!(denied, Err(ServiceError::PermissionDenied)));
}
