use rrd_contract::{CanonicalId, ResourceId, ResourceKind, ResourcePath};
use rrd_core::digest;
use rrd_security::{
    Action, AuditDecision, AuditEvent, AuditPhase, DataPolicy, Error, IdentityBinding,
    JwtIssueRequest, JwtIssuer, PolicyPredicate, Principal, PrincipalKind, ResourceGrant, Role,
    SecurityRepository, SecurityState, SECURITY_FORMAT,
};
use rrd_store::{ControlTransition, Engine, MemoryEngine, NativeEngine};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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
        credential_revision: 1,
        not_before_unix_ms: 1_000,
        expires_at_unix_ms: 10_000,
        disabled: false,
        role_ids: BTreeSet::new(),
        grants: vec![
            ResourceGrant {
                action: Action::QueryExecute,
                resource_prefix: path("alpha"),
                data_policy: None,
            },
            ResourceGrant {
                action: Action::AuditRead,
                resource_prefix: path("alpha"),
                data_policy: None,
            },
        ],
    };
    SecurityState {
        format_version: SECURITY_FORMAT,
        revision: 1,
        principals: BTreeMap::from([(principal.id.clone(), principal)]),
        roles: BTreeMap::new(),
        identity_bindings: BTreeMap::new(),
        jwt_issuers: BTreeMap::new(),
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
    assert!(matches!(
        repository.authenticate_and_authorize(
            &CanonicalId::new("connectome-local").unwrap(),
            b"correct horse battery staple",
            Action::QueryIndexEnsure,
            &path("alpha"),
            2_000,
        ),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        repository.authenticate_and_authorize(
            &CanonicalId::new("connectome-local").unwrap(),
            b"correct horse battery staple",
            Action::QueryIndexList,
            &path("alpha"),
            2_000,
        ),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        repository.authenticate_and_authorize(
            &CanonicalId::new("connectome-local").unwrap(),
            b"correct horse battery staple",
            Action::QueryLivePoll,
            &path("alpha"),
            2_000,
        ),
        Err(Error::PermissionDenied)
    ));
    drop(engine);

    let engine = NativeEngine::open(&root).unwrap();
    let repository = SecurityRepository::new(&engine, CanonicalId::new("alpha").unwrap());
    assert_eq!(repository.load().unwrap().unwrap().revision, 1);
    assert!(repository
        .authenticate_and_authorize(
            &CanonicalId::new("connectome-local").unwrap(),
            b"correct horse battery staple",
            Action::QueryExecute,
            &path("other"),
            2_000,
        )
        .is_err());
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
    let record = AuditEvent {
        audit_id: CanonicalId::new("audit-query-1").unwrap(),
        at_unix_ms: 2_000,
        principal_id: Some(CanonicalId::new("connectome-local").unwrap()),
        action: Action::QueryExecute,
        resource: path("alpha"),
        request_id: "request-query".into(),
        operation_id: "operation-query".into(),
        phase: AuditPhase::Completed,
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
    let page = repository.audit_since(0, 10).unwrap();
    assert_eq!(page.records.len(), 2);
    let bootstrap = &page.records[0].1;
    let query = &page.records[1].1;
    assert_eq!(bootstrap.action, Action::SecurityAdmin);
    assert_eq!(query.audit_id, record.audit_id);
    assert_eq!(query.status_code, record.status_code);
    assert_eq!(
        query.previous_audit_sha256,
        Some(bootstrap.audit_sha256.clone())
    );
    assert_eq!(page.chain_anchor_sha256, None);
    assert_eq!(page.chain_head_sha256, Some(query.audit_sha256.clone()));
    assert_eq!(page.through_sequence, 5);
    let journal = engine.control_journal_since(0, 10).unwrap();
    let encoded = serde_json::to_string(&journal).unwrap();
    assert!(!encoded.contains("do-not-journal-this-secret"));
    assert!(journal.iter().all(|entry| entry.verify()));
}

#[test]
fn concurrent_audit_append_has_one_linear_chain_without_forks() {
    let engine = Arc::new(MemoryEngine::new());
    SecurityRepository::new(engine.as_ref(), CanonicalId::new("alpha").unwrap())
        .initialize(
            state(b"concurrent-secret"),
            1_000,
            "bootstrap",
            "request-concurrent-bootstrap",
            "operation-concurrent-bootstrap",
        )
        .unwrap();
    let mut workers = Vec::new();
    for index in 0..8_u64 {
        let engine = Arc::clone(&engine);
        workers.push(std::thread::spawn(move || {
            SecurityRepository::new(engine.as_ref(), CanonicalId::new("alpha").unwrap())
                .append_audit(&AuditEvent {
                    audit_id: CanonicalId::new(format!("audit-concurrent-{index}")).unwrap(),
                    at_unix_ms: 2_000 + index,
                    principal_id: None,
                    action: Action::QueryExecute,
                    resource: path("alpha"),
                    request_id: format!("request-concurrent-{index}"),
                    operation_id: format!("operation-concurrent-{index}"),
                    phase: AuditPhase::Completed,
                    decision: AuditDecision::Allowed,
                    status_code: 200,
                    request_sha256: digest::sha256_hex(format!("request-{index}").as_bytes()),
                    response_sha256: digest::sha256_hex(format!("response-{index}").as_bytes()),
                })
                .unwrap();
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    let page = SecurityRepository::new(engine.as_ref(), CanonicalId::new("alpha").unwrap())
        .audit_since(0, 32)
        .unwrap();
    assert_eq!(page.records.len(), 9);
    assert!(page.records.windows(2).all(|pair| {
        pair[1].1.previous_audit_sha256.as_deref() == Some(pair[0].1.audit_sha256.as_str())
    }));
    assert_eq!(page.chain_anchor_sha256, None);
    assert_eq!(engine.control_sequence().unwrap(), 19);
}

#[test]
fn audit_read_rejects_a_substituted_durable_head() {
    let engine = MemoryEngine::new();
    let instance = CanonicalId::new("alpha").unwrap();
    SecurityRepository::new(&engine, instance.clone())
        .initialize(
            state(b"head-integrity-secret"),
            1_000,
            "bootstrap",
            "request-head-bootstrap",
            "operation-head-bootstrap",
        )
        .unwrap();
    let key = format!("server/state/{instance}/audit-head");
    let expected = engine.control_record(&key).unwrap().unwrap();
    let mut substituted: serde_json::Value = serde_json::from_slice(&expected).unwrap();
    substituted["audit_sha256"] = serde_json::Value::String("f".repeat(64));
    engine
        .commit_control_transition(&ControlTransition {
            key,
            expected: Some(expected),
            replacement: Some(serde_json::to_vec(&substituted).unwrap()),
            at: 2_000,
            actor: "tamper-test".into(),
            action: "test.audit-head-substitution".into(),
            request_id: "request-head-substitution".into(),
            operation_id: "operation-head-substitution".into(),
        })
        .unwrap();
    assert!(matches!(
        SecurityRepository::new(&engine, instance).audit_since(0, 16),
        Err(Error::Invalid(message)) if message == "audit head digest is invalid"
    ));
}

#[test]
fn roles_jwt_external_identity_rotation_revocation_and_reopen_share_one_authority() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("identity-authority");
    let signing_key = b"jwt-signing-material-never-persisted";
    let principal_id = CanonicalId::new("tenant-reader").unwrap();
    let session_role = CanonicalId::new("session-user").unwrap();
    let reader_role = CanonicalId::new("tenant-document-reader").unwrap();
    let issuer_id = CanonicalId::new("rrd-local-issuer").unwrap();
    let tenant_policy = DataPolicy {
        tenant: Some(PolicyPredicate {
            field: "tenant_id".into(),
            value: rrd_core::RuntimeValue::String("tenant-alpha".into()),
        }),
        rows: vec![PolicyPredicate {
            field: "classification".into(),
            value: rrd_core::RuntimeValue::String("public".into()),
        }],
        allowed_fields: Some(BTreeSet::from(["body".into(), "id".into()])),
    };
    let roles = BTreeMap::from([
        (
            session_role.clone(),
            Role {
                id: session_role.clone(),
                inherits: BTreeSet::new(),
                grants: vec![ResourceGrant {
                    action: Action::SessionCreate,
                    resource_prefix: path("alpha"),
                    data_policy: None,
                }],
            },
        ),
        (
            reader_role.clone(),
            Role {
                id: reader_role.clone(),
                inherits: BTreeSet::from([session_role]),
                grants: vec![ResourceGrant {
                    action: Action::QueryExecute,
                    resource_prefix: path("alpha"),
                    data_policy: Some(tenant_policy.clone()),
                }],
            },
        ),
    ]);
    let principal = Principal {
        id: principal_id.clone(),
        kind: PrincipalKind::User,
        credential_sha256: digest::sha256_hex(b"initial-api-key"),
        credential_revision: 1,
        not_before_unix_ms: 1_000,
        expires_at_unix_ms: 20_000,
        disabled: false,
        role_ids: BTreeSet::from([reader_role]),
        grants: Vec::new(),
    };
    let binding = IdentityBinding {
        id: CanonicalId::new("oidc-alice").unwrap(),
        issuer: "https://identity.example.test".into(),
        subject: "alice-external-subject".into(),
        audience: "rrflow".into(),
        principal_id: principal_id.clone(),
        disabled: false,
    };
    let issuer = JwtIssuer {
        id: issuer_id.clone(),
        issuer: "https://rrd.example.test".into(),
        audience: "rrflow-client".into(),
        key_id: CanonicalId::new("signing-key-1").unwrap(),
        signing_key_sha256: digest::sha256_hex(signing_key),
        not_before_unix_ms: 1_000,
        expires_at_unix_ms: 20_000,
        disabled: false,
    };
    let state = SecurityState {
        format_version: SECURITY_FORMAT,
        revision: 1,
        principals: BTreeMap::from([(principal_id.clone(), principal)]),
        roles,
        identity_bindings: BTreeMap::from([(binding.id.clone(), binding)]),
        jwt_issuers: BTreeMap::from([(issuer_id.clone(), issuer)]),
    };
    let engine = NativeEngine::open(&root).unwrap();
    let repository = SecurityRepository::new(&engine, CanonicalId::new("alpha").unwrap());
    repository
        .initialize(
            state,
            1_000,
            "security-admin",
            "request-initialize-authority",
            "operation-initialize-authority",
        )
        .unwrap();
    let authorization = repository
        .authenticate_and_authorize(
            &principal_id,
            b"initial-api-key",
            Action::QueryExecute,
            &path("alpha"),
            2_000,
        )
        .unwrap();
    assert_eq!(authorization.data_policy, Some(tenant_policy));
    assert_eq!(authorization.policy_revision, 1);
    assert_eq!(authorization.credential_revision, 1);
    assert_eq!(authorization.authorization_sha256.len(), 64);
    assert_eq!(
        repository
            .resolve_verified_identity(
                "https://identity.example.test",
                "alice-external-subject",
                "rrflow",
                2_000,
            )
            .unwrap(),
        principal_id
    );
    let jwt = repository
        .issue_jwt(
            &principal_id,
            b"initial-api-key",
            signing_key,
            &JwtIssueRequest {
                issuer_id,
                token_id: CanonicalId::new("token-1").unwrap(),
                issued_at_unix_ms: 2_000,
                not_before_unix_ms: 2_000,
                expires_at_unix_ms: 3_000,
            },
        )
        .unwrap();
    assert_eq!(
        repository
            .authenticate_jwt(&jwt.token, signing_key, 2_500)
            .unwrap(),
        principal_id
    );
    let mut tampered = jwt.token.clone().into_bytes();
    let last = tampered.last_mut().unwrap();
    *last = if *last == b'A' { b'B' } else { b'A' };
    assert!(repository
        .authenticate_jwt(std::str::from_utf8(&tampered).unwrap(), signing_key, 2_500)
        .is_err());
    drop(engine);

    let engine = NativeEngine::open(&root).unwrap();
    let repository = SecurityRepository::new(&engine, CanonicalId::new("alpha").unwrap());
    assert_eq!(
        repository
            .authenticate_jwt(&jwt.token, signing_key, 2_500)
            .unwrap(),
        principal_id
    );
    let mut rotated = repository.load().unwrap().unwrap();
    rotated.revision = 2;
    let principal = rotated.principals.get_mut(&principal_id).unwrap();
    principal.credential_sha256 = digest::sha256_hex(b"rotated-api-key");
    principal.credential_revision = 2;
    repository
        .replace(
            1,
            rotated.clone(),
            2_600,
            "security-admin",
            "request-rotate",
            "operation-rotate",
        )
        .unwrap();
    repository
        .replace(
            1,
            rotated,
            2_600,
            "security-admin",
            "request-rotate-replay",
            "operation-rotate",
        )
        .unwrap();
    assert!(repository
        .authenticate_jwt(&jwt.token, signing_key, 2_700)
        .is_err());
    assert!(repository
        .authenticate_principal(&principal_id, b"initial-api-key")
        .is_err());
    repository
        .authenticate_principal(&principal_id, b"rotated-api-key")
        .unwrap();

    let mut revoked = repository.load().unwrap().unwrap();
    revoked.revision = 3;
    revoked.principals.get_mut(&principal_id).unwrap().disabled = true;
    repository
        .replace(
            2,
            revoked,
            2_800,
            "security-admin",
            "request-revoke",
            "operation-revoke",
        )
        .unwrap();
    assert!(matches!(
        repository.authorize_principal(&principal_id, Action::QueryExecute, &path("alpha"), 2_900),
        Err(Error::PermissionDenied)
    ));

    let persisted = serde_json::to_vec(&engine.control_journal_since(0, 64).unwrap()).unwrap();
    assert!(!persisted
        .windows(jwt.token.len())
        .any(|window| window == jwt.token.as_bytes()));
    assert!(!persisted
        .windows(signing_key.len())
        .any(|window| window == signing_key));
}

#[test]
fn cyclic_roles_and_ambiguous_equally_specific_policy_fail_closed() {
    let mut authority = state(b"secret");
    let left = CanonicalId::new("left").unwrap();
    let right = CanonicalId::new("right").unwrap();
    authority.roles = BTreeMap::from([
        (
            left.clone(),
            Role {
                id: left.clone(),
                inherits: BTreeSet::from([right.clone()]),
                grants: Vec::new(),
            },
        ),
        (
            right.clone(),
            Role {
                id: right,
                inherits: BTreeSet::from([left]),
                grants: Vec::new(),
            },
        ),
    ]);
    assert!(authority.validate().is_err());

    let temporary = tempfile::tempdir().unwrap();
    let engine = NativeEngine::open(temporary.path()).unwrap();
    let mut ambiguous = state(b"secret");
    let constrained = CanonicalId::new("constrained-reader").unwrap();
    ambiguous.roles.insert(
        constrained.clone(),
        Role {
            id: constrained.clone(),
            inherits: BTreeSet::new(),
            grants: vec![ResourceGrant {
                action: Action::QueryExecute,
                resource_prefix: path("alpha"),
                data_policy: Some(DataPolicy {
                    tenant: Some(PolicyPredicate {
                        field: "tenant_id".into(),
                        value: rrd_core::RuntimeValue::String("tenant-alpha".into()),
                    }),
                    rows: Vec::new(),
                    allowed_fields: None,
                }),
            }],
        },
    );
    ambiguous
        .principals
        .get_mut(&CanonicalId::new("connectome-local").unwrap())
        .unwrap()
        .role_ids
        .insert(constrained);
    let repository = SecurityRepository::new(&engine, CanonicalId::new("alpha").unwrap());
    repository
        .initialize(
            ambiguous,
            1_000,
            "bootstrap",
            "request-ambiguous",
            "operation-ambiguous",
        )
        .unwrap();
    assert!(matches!(
        repository.authorize_principal(
            &CanonicalId::new("connectome-local").unwrap(),
            Action::QueryExecute,
            &path("alpha"),
            2_000,
        ),
        Err(Error::Invalid(_))
    ));
}
