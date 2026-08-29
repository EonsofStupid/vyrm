use rrd_contract::{
    runtime_tool_arguments_sha256, transaction_operation_sha256, AuditDecision, AuditPhase,
    BeginTransaction, CanonicalId, CloseSession, CommitTransaction, CorrelationId, CreateSession,
    DataCatalogueIdentity, DataProperties, DataPropertySchema, DataRecordSchema, DataReference,
    DataSchemaRegistry, DataValueType, DataVectorValue, EnsureQueryIndex, EnsureVectorCollection,
    EstateDesiredPhase, LifecycleEnforcementLevelV1, LifecycleEventCommandV1, LifecycleEventTypeV1,
    LifecyclePayloadV1, LifecycleSessionSnapshotV1, LifecycleTraceContextV1, NamedVectorDefinition,
    QueryBudget, QueryIndexKind, QueryValue, ReadAudit, ReadChangefeed, ReadDiagnosticSnapshot,
    RequestContext, ResourceId, ResourceKind, ResourcePath, RuntimeToolInvocation, SecurityAction,
    SessionLimits, TransactionMutation, VectorMemoryTier, VectorSearchMetric, VectorValueKind,
    RUNTIME_TOOL_CATALOGUE_VERSION,
};
use rrd_core::{digest, Claim, Predicate, Producer, Subject};
use rrd_engine::{
    EstateAdminAction, EstateAdminResult, InstanceBinding, InstanceManifest, Invocation,
    InvocationCredential, NativeApplicationFormat, RrdEngine, RrdOperation,
    SecurityBootstrapOutcome, ServiceError,
};
use rrd_estate::{
    DesiredPhase, DesiredTarget, EstateRepository, LeaseRequest, LocalEstatePermission,
    LocalOperatorPolicy, MutationContext, ObservationRequest, ObservedPhase, ReceiptBoundary,
    ReceiptRequest, ScheduleBackup, SetDesired, SetRecoveryPolicy, LOCAL_OPERATOR_POLICY_FORMAT,
};
use rrd_store::Engine;
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
fn engine_exposes_only_the_reviewed_native_format_successor() {
    let matrix = RrdEngine::supported_native_format_migrations();
    assert_eq!(matrix.len(), 1);
    assert_eq!(matrix[0].source, NativeApplicationFormat::TextV1);
    assert_eq!(matrix[0].target, NativeApplicationFormat::TagV2);
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
                catalogue: DataCatalogueIdentity::default(),
                tables: BTreeMap::new(),
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

#[test]
fn estate_recovery_prune_and_restore_are_one_fenced_engine_workflow() {
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("estate-authority");
    let state_root = temporary.path().join("state");
    for path in [
        state_root.clone(),
        state_root.join("instances"),
        state_root.join("processes"),
    ] {
        fs::create_dir(&path).unwrap();
    }
    let state_root = fs::canonicalize(state_root).unwrap();
    let source = state_root.join("instances/instance-a/.rrflow/rrd");
    drop(rrd_store::PersistentEngine::open(&source).unwrap());

    let key_path = temporary.path().join("operator.key");
    let policy_path = temporary.path().join("operator.json");
    let key = [23_u8; 32];
    fs::write(&key_path, key).unwrap();
    private(&key_path);
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
            BTreeSet::from([
                LocalEstatePermission::PruneRecovery,
                LocalEstatePermission::RestoreRecovery,
            ]),
        )]),
    };
    fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    private(&policy_path);

    let authority = rrd_store::PersistentEngine::open(&database).unwrap();
    let repository = EstateRepository::new(&authority, canonical("estate-a"));
    let estate_context = |at, request: &str, operation: &str| MutationContext {
        at,
        actor: "operator-one".into(),
        request_id: request.into(),
        operation_id: canonical(operation),
    };
    repository
        .create(&estate_context(10, "create-estate", "create-estate"))
        .unwrap();
    repository
        .set_desired(&SetDesired {
            context: estate_context(20, "stop-instance", "stop-instance"),
            instance_id: canonical("instance-a"),
            idempotency_key: "stop-instance".into(),
            target: DesiredTarget {
                phase: DesiredPhase::Stopped,
                deployment_ref: canonical("rrd-server"),
                version: "0.1.0".into(),
                configuration_sha256: "a".repeat(64),
            },
        })
        .unwrap();
    repository
        .acquire_lease(&LeaseRequest {
            context: estate_context(30, "lease-stop", "stop-instance"),
            worker: canonical("estate-worker"),
            lease_ms: 1_000,
        })
        .unwrap();
    for (at, request, boundary, evidence) in [
        (40, "prepare-stop", ReceiptBoundary::Prepared, "b"),
        (50, "apply-stop", ReceiptBoundary::Applied, "c"),
    ] {
        repository
            .record_receipt(&ReceiptRequest {
                context: estate_context(at, request, "stop-instance"),
                worker: canonical("estate-worker"),
                lease_epoch: 1,
                boundary,
                evidence_sha256: evidence.repeat(64),
                error: None,
            })
            .unwrap();
    }
    repository
        .record_observation(&ObservationRequest {
            context: estate_context(60, "observe-stop", "stop-instance"),
            worker: canonical("estate-worker"),
            lease_epoch: 1,
            phase: ObservedPhase::Stopped,
            version: None,
            process_id: None,
            evidence_sha256: "d".repeat(64),
            error: None,
        })
        .unwrap();
    repository
        .record_receipt(&ReceiptRequest {
            context: estate_context(70, "complete-stop", "stop-instance"),
            worker: canonical("estate-worker"),
            lease_epoch: 1,
            boundary: ReceiptBoundary::Completed,
            evidence_sha256: "e".repeat(64),
            error: None,
        })
        .unwrap();
    repository
        .set_recovery_policy(&SetRecoveryPolicy {
            context: estate_context(75, "set-policy", "set-policy"),
            instance_id: canonical("instance-a"),
            idempotency_key: "set-policy".into(),
            max_rpo_ms: 1_000,
            max_rto_ms: 2_000,
            minimum_recovery_points: 1,
            retention_ms: 3_000,
        })
        .unwrap();
    drop(authority);

    for (job, scheduled_at) in [("backup-one", 100), ("backup-two", 200)] {
        let authority = rrd_store::PersistentEngine::open(&database).unwrap();
        EstateRepository::new(&authority, canonical("estate-a"))
            .schedule_backup(&ScheduleBackup {
                context: estate_context(scheduled_at, job, job),
                instance_id: canonical("instance-a"),
                idempotency_key: job.into(),
                label: job.into(),
            })
            .unwrap();
        drop(authority);
        for at in [scheduled_at + 10, scheduled_at + 20, scheduled_at + 30] {
            RrdEngine::reconcile_estate_backup_store(
                &database,
                canonical("estate-control-instance"),
                &state_root,
                canonical("estate-a"),
                canonical("backup-worker"),
                1_000,
                at,
                None,
            )
            .unwrap();
        }
    }
    let before =
        rrd_store::verify_backup_catalogue(&state_root.join("backups/instance-a")).unwrap();
    assert_eq!(before.backups.len(), 2);
    let first_id = before.backups[0].backup_id.clone();
    let second_id = before.backups[1].backup_id.clone();

    let pruned = RrdEngine::prune_estate_backups_store(
        &database,
        canonical("estate-control-instance"),
        &state_root,
        &policy_path,
        &key_path,
        canonical("estate-a"),
        canonical("instance-a"),
        4_000,
        4_000,
        "prune-request".into(),
        canonical("prune-operation"),
        "prune-daily".into(),
    )
    .unwrap();
    assert!(!pruned.idempotent_replay);
    assert_eq!(pruned.pruned_backup_ids, vec![first_id.clone()]);
    assert_eq!(
        pruned
            .recovery
            .recovery_points
            .iter()
            .find(|point| point.backup_sha256 == first_id)
            .unwrap()
            .pruned_at_unix_ms,
        Some(4_000)
    );
    assert!(pruned.recovery.prune_intents.is_empty());
    let physical =
        rrd_store::verify_backup_catalogue(&state_root.join("backups/instance-a")).unwrap();
    assert_eq!(physical.backups.len(), 1);
    assert_eq!(physical.backups[0].backup_id, second_id);
    let prune_replay = RrdEngine::prune_estate_backups_store(
        &database,
        canonical("estate-control-instance"),
        &state_root,
        &policy_path,
        &key_path,
        canonical("estate-a"),
        canonical("instance-a"),
        4_000,
        4_000,
        "prune-request".into(),
        canonical("prune-operation"),
        "prune-daily".into(),
    )
    .unwrap();
    assert!(prune_replay.idempotent_replay);

    let restored = RrdEngine::restore_estate_backup_store(
        &database,
        canonical("estate-control-instance"),
        &state_root,
        &policy_path,
        &key_path,
        canonical("estate-a"),
        canonical("instance-a"),
        second_id.clone(),
        canonical("restore-one"),
        4_100,
        4_200,
        "restore-request".into(),
        canonical("restore-operation"),
        "restore-daily".into(),
    )
    .unwrap();
    assert!(!restored.idempotent_replay);
    assert!(restored.reopened);
    let evidence = restored
        .recovery
        .restore_evidence
        .iter()
        .find(|evidence| evidence.restore_id.as_str() == "restore-one")
        .unwrap();
    assert!(!evidence.rpo_within_objective);
    assert!(evidence.rto_within_objective);
    assert!(state_root
        .join("restores/instance-a/restore-one/CURRENT")
        .is_file());
    assert!(state_root
        .join("instances/instance-a/.rrflow/rrd/CURRENT")
        .is_file());

    let pruned_restore = RrdEngine::restore_estate_backup_store(
        &database,
        canonical("estate-control-instance"),
        &state_root,
        &policy_path,
        &key_path,
        canonical("estate-a"),
        canonical("instance-a"),
        first_id,
        canonical("restore-pruned"),
        4_205,
        4_210,
        "restore-pruned-request".into(),
        canonical("restore-pruned-operation"),
        "restore-pruned".into(),
    )
    .unwrap_err();
    assert!(pruned_restore.to_string().contains("unavailable"));

    let restore_replay = RrdEngine::restore_estate_backup_store(
        &database,
        canonical("estate-control-instance"),
        &state_root,
        &policy_path,
        &key_path,
        canonical("estate-a"),
        canonical("instance-a"),
        second_id.clone(),
        canonical("restore-one"),
        4_100,
        4_200,
        "restore-request".into(),
        canonical("restore-operation"),
        "restore-daily".into(),
    )
    .unwrap();
    assert!(restore_replay.idempotent_replay);

    let catalogue_root = state_root.join("backups/instance-a");
    let gap_target = state_root.join("restores/instance-a/restore-gap");
    rrd_store::restore_catalogued_backup(&catalogue_root, &second_id, &gap_target, 4_250).unwrap();
    let recovered_gap = RrdEngine::restore_estate_backup_store(
        &database,
        canonical("estate-control-instance"),
        &state_root,
        &policy_path,
        &key_path,
        canonical("estate-a"),
        canonical("instance-a"),
        second_id.clone(),
        canonical("restore-gap"),
        4_250,
        4_300,
        "restore-gap-request".into(),
        canonical("restore-gap-operation"),
        "restore-gap".into(),
    )
    .unwrap();
    assert!(recovered_gap.idempotent_replay);

    let divergent_target = state_root.join("restores/instance-a/restore-divergent");
    let divergent = rrd_store::PersistentEngine::open(&divergent_target).unwrap();
    divergent
        .append_batch(&[Claim::new(
            Subject::new("restore:divergent").unwrap(),
            Predicate::new("status").unwrap(),
            "different",
            4_350,
            4_350,
            Producer {
                actor: "test:recovery".into(),
                on_behalf_of: None,
                session: None,
            },
        )])
        .unwrap();
    drop(divergent);
    let divergent_error = RrdEngine::restore_estate_backup_store(
        &database,
        canonical("estate-control-instance"),
        &state_root,
        &policy_path,
        &key_path,
        canonical("estate-a"),
        canonical("instance-a"),
        second_id,
        canonical("restore-divergent"),
        4_350,
        4_400,
        "restore-divergent-request".into(),
        canonical("restore-divergent-operation"),
        "restore-divergent".into(),
    )
    .unwrap_err();
    assert!(divergent_error.to_string().contains("watermarks differ"));
}
