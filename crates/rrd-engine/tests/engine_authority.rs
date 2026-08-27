use rrd_contract::{
    runtime_tool_arguments_sha256, transaction_operation_sha256, AuditDecision, AuditPhase,
    BeginTransaction, CanonicalId, CloseSession, CommitTransaction, CorrelationId, CreateSession,
    DataProperties, DataPropertySchema, DataRecordSchema, DataReference, DataSchemaRegistry,
    DataValueType, DataVectorValue, EnsureQueryIndex, EnsureVectorCollection, EstateDesiredPhase,
    LifecycleEnforcementLevelV1, LifecycleEventCommandV1, LifecycleEventTypeV1, LifecyclePayloadV1,
    LifecycleSessionSnapshotV1, LifecycleTraceContextV1, NamedVectorDefinition, QueryBudget,
    QueryIndexKind, QueryValue, ReadAudit, ReadChangefeed, ReadDiagnosticSnapshot, RequestContext,
    ResourceId, ResourceKind, ResourcePath, RuntimeToolInvocation, SecurityAction, SessionLimits,
    TransactionMutation, VectorMemoryTier, VectorSearchMetric, VectorValueKind,
    RUNTIME_TOOL_CATALOGUE_VERSION,
};
use rrd_core::digest;
use rrd_engine::{
    EstateAdminAction, EstateAdminResult, InstanceBinding, InstanceManifest, Invocation,
    InvocationCredential, RrdEngine, RrdOperation, SecurityBootstrapOutcome, ServiceError,
};
use rrd_estate::{LocalEstatePermission, LocalOperatorPolicy, LOCAL_OPERATOR_POLICY_FORMAT};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

fn canonical(value: &str) -> CanonicalId {
    CanonicalId::new(value).unwrap()
}

fn correlation(value: &str) -> CorrelationId {
    CorrelationId::new(value).unwrap()
}

fn context(request: &str, operation: &str, idempotency: &str) -> RequestContext {
    RequestContext {
        request_id: correlation(request),
        operation_id: correlation(operation),
        idempotency_key: Some(correlation(idempotency)),
        deadline_unix_ms: Some(10_000),
    }
}

fn resource(instance: &CanonicalId) -> ResourcePath {
    ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance.as_str()).unwrap()],
    }
}

fn reference(kind: &str, id: &str) -> DataReference {
    DataReference {
        kind: canonical(kind),
        id: canonical(id),
    }
}

fn diagnostic_request(scope: &str) -> ReadDiagnosticSnapshot {
    ReadDiagnosticSnapshot {
        scope: scope.into(),
        graph_valid_at_unix_ms: 1_500,
        graph_known_at_cursor: None,
        graph_compare_cursor: 0,
        runtime_max_scanned_changes: 1_024,
        changes_after_cursor: 0,
        change_limit: 64,
        audit_after_sequence: 0,
        audit_limit: 64,
    }
}

#[test]
fn one_authority_coordinates_security_data_catalogues_lifecycle_audit_and_reopen() {
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    fs::create_dir(&project).unwrap();
    InstanceManifest::ensure_dedicated(&project).unwrap();
    let binding = InstanceBinding::discover(&project).unwrap();
    let database = binding.expected_store();
    let instance = canonical(&binding.manifest.id);
    let scope = format!("instance:{instance}");

    let principal_id = canonical("authority-operator");
    let credential = temporary.path().join("principal.key");
    fs::write(&credential, b"principal-secret").unwrap();
    private(&credential);
    let security_manifest = temporary.path().join("security.json");
    let instance_resource = resource(&instance);
    let grants = [
        SecurityAction::SessionCreate,
        SecurityAction::SessionClose,
        SecurityAction::TransactionBegin,
        SecurityAction::TransactionCommit,
        SecurityAction::VectorCollectionEnsure,
        SecurityAction::QueryIndexEnsure,
        SecurityAction::LifecycleApply,
        SecurityAction::ChangefeedRead,
        SecurityAction::AuditRead,
        SecurityAction::DiagnosticsRead,
    ]
    .into_iter()
    .map(|action| {
        json!({
            "action": action,
            "resource_prefix": instance_resource,
        })
    })
    .collect::<Vec<_>>();
    fs::write(
        &security_manifest,
        serde_json::to_vec(&json!({
            "format_version": 1,
            "revision": 1,
            "principals": [{
                "id": principal_id,
                "kind": "service",
                "credential_file": credential,
                "not_before_unix_ms": 1,
                "expires_at_unix_ms": u64::MAX,
                "grants": grants,
            }],
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        RrdEngine::bootstrap_security_store(&database, instance.clone(), &security_manifest, 100,)
            .unwrap(),
        SecurityBootstrapOutcome::Initialized
    );
    let authority_secret = fs::read(database.join("RRD.SECRET")).unwrap();

    let operator_key = temporary.path().join("operator.key");
    let operator_policy = temporary.path().join("operator.json");
    let key = [13_u8; 32];
    fs::write(&operator_key, key).unwrap();
    private(&operator_key);
    let policy = LocalOperatorPolicy {
        format: LOCAL_OPERATOR_POLICY_FORMAT,
        operator_id: canonical("operator-one"),
        key_sha256: Sha256::digest(key)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        not_before_unix_ms: 1,
        expires_at_unix_ms: 10_000,
        estates: BTreeMap::from([(
            "estate-a".into(),
            BTreeSet::from([LocalEstatePermission::Create]),
        )]),
    };
    fs::write(&operator_policy, serde_json::to_vec(&policy).unwrap()).unwrap();
    private(&operator_policy);
    let create_estate = || {
        RrdEngine::administer_estate_store(
            &database,
            instance.clone(),
            &operator_policy,
            &operator_key,
            canonical("estate-a"),
            200,
            "create-estate-request".into(),
            canonical("create-estate-operation"),
            EstateAdminAction::Create,
        )
        .unwrap()
    };
    assert!(matches!(
        create_estate(),
        EstateAdminResult::Mutation(result) if !result.idempotent_replay
    ));
    assert!(matches!(
        create_estate(),
        EstateAdminResult::Mutation(result) if result.idempotent_replay
    ));

    let engine = RrdEngine::open_bound(&binding).unwrap();
    assert_eq!(engine.instance_id(), &instance);
    let lease = engine
        .create_authenticated_session(
            &principal_id,
            b"principal-secret",
            &CreateSession {
                limits: SessionLimits {
                    idle_timeout_ms: 60_000,
                    absolute_timeout_ms: 60_000,
                    max_open_transactions: 2,
                },
            },
            &correlation("authority-session"),
            1_000,
            "request-session",
            "operation-session",
        )
        .unwrap();

    let vectors = engine
        .ensure_vector_collection(
            &lease.session_id,
            &lease.token,
            &EnsureVectorCollection {
                scope: scope.clone(),
                collection_id: canonical("documents"),
                vectors: vec![NamedVectorDefinition {
                    name: canonical("body"),
                    field: canonical("body-embedding"),
                    kind: VectorValueKind::Dense,
                    dimensions: 2,
                    metric: VectorSearchMetric::Cosine,
                    embedding_model: None,
                    memory_tier: VectorMemoryTier::Cached,
                }],
            },
            &context(
                "request-vector-catalogue",
                "operation-vector-catalogue",
                "vector-catalogue-key",
            ),
            1_100,
        )
        .unwrap();
    assert_eq!(vectors.catalogue_revision, 1);

    let transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &BeginTransaction {
                scope: canonical("data"),
                timeout_ms: 10_000,
            },
            &context(
                "request-transaction-begin",
                "operation-transaction-begin",
                "transaction-begin-key",
            ),
            1_200,
        )
        .unwrap();
    let document = reference("document", "document-a");
    let mutations = vec![
        TransactionMutation::PutSchema {
            registry: DataSchemaRegistry {
                revision: 1,
                migration: "install authority fixture schema".into(),
                records: BTreeMap::from([(
                    canonical("document"),
                    DataRecordSchema {
                        properties: BTreeMap::from([(
                            "title".into(),
                            DataPropertySchema {
                                value_type: DataValueType::String,
                                required: true,
                            },
                        )]),
                        allow_additional_properties: false,
                        unique_properties: BTreeSet::new(),
                    },
                )]),
                relations: BTreeMap::new(),
                events: BTreeMap::new(),
            },
        },
        TransactionMutation::PutRecord {
            reference: document.clone(),
            valid_from: 1_250,
            valid_to: None,
            properties: DataProperties::from([(
                "title".into(),
                QueryValue::String("one authority".into()),
            )]),
        },
        TransactionMutation::PutVector {
            reference: reference("embedding", "document-a-body"),
            subject: document,
            collection_id: Some(canonical("documents")),
            vector_name: Some(canonical("body")),
            field: canonical("body-embedding"),
            valid_from: 1_250,
            valid_to: None,
            value: DataVectorValue::Dense {
                values: vec![1.0, 0.0],
            },
            provenance: None,
            properties: DataProperties::new(),
        },
        TransactionMutation::AssertClaim {
            subject: canonical("document-a"),
            predicate: canonical("authority"),
            object: "rrd-engine".into(),
            valid_from: 1_250,
            tx_time: 1_250,
            producer: canonical("authority-test"),
            confidence: Some(1.0),
        },
    ];
    let commit_request = CommitTransaction {
        operation_sha256: transaction_operation_sha256(&mutations),
        mutations,
    };
    let commit = engine
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &correlation("transaction-commit-key"),
            &commit_request,
            1_300,
            "request-transaction-commit",
            "operation-transaction-commit",
        )
        .unwrap();
    assert_eq!(commit.mutation_count, 4);
    assert_eq!(commit.claim_mutation_count, Some(1));

    let lifecycle_command = LifecycleEventCommandV1 {
        event_type: LifecycleEventTypeV1::SessionOpened,
        occurred_at_unix_ms: 1_400,
        instance_id: instance.to_string(),
        project_id: instance.to_string(),
        member_id: None,
        session_id: "authority-lifecycle".into(),
        turn_id: None,
        reasoning_run_id: None,
        attempt_id: None,
        tool_call_id: None,
        correlation_id: "authority-lifecycle-open".into(),
        actor: "agent:authority-test".into(),
        adapter_kind: "integration-test".into(),
        adapter_version: "1.0.0".into(),
        enforcement_level: LifecycleEnforcementLevelV1::Cooperative,
        scope: scope.clone(),
        payload: LifecyclePayloadV1::SessionOpened {
            resumed: false,
            provider_session_sha256: None,
        },
        read_stamp: None,
        trace: LifecycleTraceContextV1 {
            trace_id: format!("{:032x}", 1_400),
            span_id: format!("{:016x}", 1_400),
            parent_span_id: None,
        },
    };
    let lifecycle_arguments = json!({"command": lifecycle_command, "recorded_at_unix_ms": 1_400});
    let lifecycle_request = RuntimeToolInvocation {
        catalogue_version: RUNTIME_TOOL_CATALOGUE_VERSION,
        tool: canonical("rrflow_lifecycle"),
        arguments_sha256: runtime_tool_arguments_sha256(&lifecycle_arguments).unwrap(),
        arguments: lifecycle_arguments,
    };
    let lifecycle_invocation = Invocation {
        context: context("request-lifecycle", "operation-lifecycle", "lifecycle-key"),
        resource: resource(&instance),
        observed_at_unix_ms: 1_400,
        attempt: 1,
        request_sha256: digest::sha256_hex(&serde_json::to_vec(&lifecycle_request).unwrap()),
    };
    let lifecycle = engine
        .invoke_runtime_tool(
            &project,
            &lifecycle_request,
            lifecycle_invocation,
            InvocationCredential::Session {
                session_id: &lease.session_id,
                token: &lease.token,
            },
        )
        .unwrap();
    let lifecycle: LifecycleSessionSnapshotV1 = serde_json::from_str(&lifecycle.content).unwrap();
    assert_eq!(lifecycle.event_count, 1);

    let runtime_before_denial = engine.readiness(1_450).unwrap().runtime_cursor;
    let denied = engine.begin_invocation(
        Invocation {
            context: context(
                "request-denied-backup",
                "operation-denied-backup",
                "denied-backup-key",
            ),
            resource: resource(&instance),
            observed_at_unix_ms: 1_450,
            attempt: 1,
            request_sha256: digest::sha256_hex(b"denied-backup"),
        },
        RrdOperation::BackupCreate,
        InvocationCredential::Session {
            session_id: &lease.session_id,
            token: &lease.token,
        },
    );
    assert!(matches!(denied, Err(ServiceError::PermissionDenied)));
    assert_eq!(
        engine.readiness(1_451).unwrap().runtime_cursor,
        runtime_before_denial,
        "policy denial must not mutate the runtime log"
    );

    let coordinated_vectors = engine
        .ensure_vector_collection(
            &lease.session_id,
            &lease.token,
            &EnsureVectorCollection {
                scope: scope.clone(),
                collection_id: canonical("notes"),
                vectors: vec![NamedVectorDefinition {
                    name: canonical("title"),
                    field: canonical("title-embedding"),
                    kind: VectorValueKind::Dense,
                    dimensions: 2,
                    metric: VectorSearchMetric::Cosine,
                    embedding_model: None,
                    memory_tier: VectorMemoryTier::Cached,
                }],
            },
            &context(
                "request-coordinated-vector-catalogue",
                "operation-coordinated-vector-catalogue",
                "coordinated-vector-catalogue-key",
            ),
            1_480,
        )
        .unwrap();
    let vector_read = engine
        .read_diagnostic_snapshot(
            &lease.session_id,
            &lease.token,
            &diagnostic_request(&scope),
            1_490,
            "request-vector-read-stamp",
            "operation-vector-read-stamp",
        )
        .unwrap();
    let query_index = engine
        .ensure_query_index(
            &lease.session_id,
            &lease.token,
            &correlation("query-index-key"),
            &EnsureQueryIndex {
                scope: scope.clone(),
                index_id: canonical("document-title"),
                definition_query: "FROM record:document AT VALID 1250 KNOWN HEAD PROJECT title"
                    .into(),
                unique: false,
                kind: QueryIndexKind::Scalar,
                budget: QueryBudget::default(),
            },
            1_500,
            "request-query-index",
            "operation-query-index",
        )
        .unwrap();
    assert_eq!(
        query_index.catalogue_revision,
        coordinated_vectors.catalogue_revision + 1,
        "query and vector definitions must advance one shared catalogue revision"
    );

    let changes = engine
        .read_changefeed(
            &lease.session_id,
            &lease.token,
            &ReadChangefeed {
                scope: scope.clone(),
                after_cursor: 0,
                limit: 64,
            },
            1_600,
            "request-changefeed",
            "operation-changefeed",
        )
        .unwrap();
    assert_eq!(changes.through_cursor, changes.head_cursor);
    assert_eq!(changes.head_cursor, runtime_before_denial);

    let diagnostic = engine
        .read_diagnostic_snapshot(
            &lease.session_id,
            &lease.token,
            &diagnostic_request(&scope),
            1_700,
            "request-diagnostic",
            "operation-diagnostic",
        )
        .unwrap();
    assert_eq!(diagnostic.read.runtime_cursor, changes.head_cursor);
    assert!(
        diagnostic.read.catalogue_revision > vector_read.read.catalogue_revision,
        "the query build transitions must advance the same read-stamp catalogue coordinate observed after the vector transition"
    );
    assert_eq!(diagnostic.readiness.runtime_cursor, changes.head_cursor);
    assert_eq!(diagnostic.changes.head_cursor, changes.head_cursor);
    assert_eq!(diagnostic.vector_collections.revision, 2);
    assert_eq!(
        diagnostic.query_indexes.revision,
        query_index.catalogue_revision
    );

    let audit = engine
        .read_audit(
            &lease.session_id,
            &lease.token,
            &ReadAudit {
                after_sequence: 0,
                limit: 64,
            },
            1_800,
            "request-audit",
            "operation-audit",
        )
        .unwrap();
    assert_eq!(audit.records.len(), 3);
    assert_eq!(audit.records[0].phase, AuditPhase::Authorized);
    assert_eq!(audit.records[0].action, SecurityAction::LifecycleApply);
    assert_eq!(audit.records[1].phase, AuditPhase::Completed);
    assert_eq!(audit.records[1].decision, AuditDecision::Allowed);
    assert_eq!(audit.records[1].action, SecurityAction::LifecycleApply);
    assert_eq!(audit.records[2].phase, AuditPhase::Completed);
    assert_eq!(audit.records[2].decision, AuditDecision::Denied);
    assert_eq!(audit.records[2].action, SecurityAction::BackupCreate);
    assert_eq!(diagnostic.audit.records, audit.records);

    let closed = engine
        .close_session(
            &lease.session_id,
            &lease.token,
            &CloseSession {},
            &correlation("session-close-key"),
            1_900,
            "request-session-close",
            "operation-session-close",
        )
        .unwrap();
    assert!(!closed.idempotent_replay);
    drop(engine);

    let reopened = RrdEngine::open_bound(&binding).unwrap();
    assert_eq!(reopened.instance_id(), &instance);
    let readiness = reopened.readiness(2_000).unwrap();
    assert_eq!(readiness.runtime_cursor, diagnostic.read.runtime_cursor);
    assert_eq!(readiness.claim_sequence, diagnostic.read.claim_sequence);
    let close_replay = reopened
        .close_session(
            &lease.session_id,
            &lease.token,
            &CloseSession {},
            &correlation("session-close-key"),
            2_000,
            "request-session-close-replay",
            "operation-session-close-replay",
        )
        .unwrap();
    assert!(close_replay.idempotent_replay);
    assert_eq!(close_replay.ended_at_unix_ms, closed.ended_at_unix_ms);
    assert_eq!(
        fs::read(database.join("RRD.SECRET")).unwrap(),
        authority_secret
    );
    drop(reopened);
    assert_eq!(
        RrdEngine::bootstrap_security_store(&database, instance, &security_manifest, 2_100,)
            .unwrap(),
        SecurityBootstrapOutcome::Unchanged
    );
}

#[cfg(unix)]
fn private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(not(unix))]
fn private(_path: &Path) {}

#[test]
fn public_admin_phase_type_does_not_reintroduce_a_physical_estate_dependency() {
    let action = EstateAdminAction::SetDesired {
        instance: canonical("project-a"),
        idempotency_key: "desired-a".into(),
        phase: EstateDesiredPhase::Stopped,
        deployment: canonical("rrd-server"),
        version: "0.1.0".into(),
        configuration_sha256: "a".repeat(64),
    };
    assert!(matches!(action, EstateAdminAction::SetDesired { .. }));
}
