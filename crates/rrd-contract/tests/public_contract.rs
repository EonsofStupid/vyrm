use rrd_contract::{
    transaction_operation_sha256, BeginTransaction, CanonicalId, CapabilityDescriptor,
    CapabilityStatus, CloseSession, CommitReceipt, CommitTransaction, CorrelationId,
    CreateInstanceBackup, DataCatalogueIdentity, DataLogicalModel, DataRecordSchema, DataReference,
    DataSchemaMode, DataSchemaRegistry, DataTableSchema, DataTarget, DeploymentMode,
    EnsureQueryIndex, EnsureVectorCollection, EnsureVectorIndex, ErrorBody, ErrorCode,
    EstateActivityPolicySnapshot, EstateBackupJobSnapshot, EstateBackupJobState,
    EstateBackupJobsSnapshot, EstateMutationResult, EstateSnapshot, ExecuteQuery,
    ExecuteQueryTransaction, FollowChangefeed, ForwardRollbackCounts, ForwardRollbackRequest,
    HybridFusion, IdempotencyBinding, ListQueryIndexes, ListVectorCollections, Liveness,
    NamedVectorDefinition, PollLiveQuery, PreviewTransaction, ProductCapability,
    ProductCapabilityCatalogue, ProductSurface, QueryBudget, QueryExecutionAnalysisSnapshot,
    QueryExecutionSnapshot, QueryIndexKind, QueryPlanCandidate, QueryPlanSnapshot, QueryResult,
    QueryRowSnapshot, QueryValue, ReadAudit, ReadChangefeed, ReadDataSnapshot, ReadEstate,
    Readiness, RenewSession, RequestContext, RequestEnvelope, ResourceId, ResourceKind,
    ResourcePath, ResponseEnvelope, ResponseOutcome, RestoreInstanceBackup, SearchHybrid,
    SearchVectors, ServiceCapabilities, SessionEndState, SessionLease, SessionLimits,
    SessionTermination, SurfaceBinding, SurfaceDisposition, TransactionMutation,
    TransactionPreview, TransactionState, VectorIndexConfiguration, VectorMemoryTier,
    VectorPayloadCondition, VectorPayloadFilter, VectorPayloadOperator, VectorQuantizationBits,
    VectorSearchMetric, VectorSearchMode, VectorSearchQuery, VectorValueKind, PROTOCOL,
    PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixturePayload {
    action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContractFixture {
    service: ServiceCapabilities,
    request: RequestEnvelope<FixturePayload>,
    success: ResponseEnvelope<FixturePayload>,
    failure: ResponseEnvelope<FixturePayload>,
    idempotency: IdempotencyBinding,
}

#[test]
fn forward_rollback_request_is_strict_and_counts_cannot_overflow() {
    let request: ForwardRollbackRequest = serde_json::from_value(serde_json::json!({
        "target_valid_at": 100,
        "target_known_at_cursor": 7,
        "effective_at": 200,
        "reason": "restore the accepted state"
    }))
    .unwrap();
    request.validate().unwrap();

    let mut unknown = serde_json::to_value(&request).unwrap();
    unknown["rewrite_history"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ForwardRollbackRequest>(unknown).is_err());

    let counts = ForwardRollbackCounts {
        restored_records: u64::MAX,
        retired_records: 1,
        restored_relations: 0,
        retired_relations: 0,
    };
    assert_eq!(counts.checked_mutation_count(), None);
}

fn contract_fixture() -> ContractFixture {
    let request_id = CorrelationId::new("req-01j5y0h4k7").unwrap();
    let operation_id = CorrelationId::new("op-01j5y0h4k8").unwrap();
    let idempotency_key = CorrelationId::new("idem-01j5y0h4k9").unwrap();
    ContractFixture {
        service: ServiceCapabilities {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            implementation: CanonicalId::new("rrflow").unwrap(),
            implementation_version: "0.1.0".into(),
            deployment_mode: DeploymentMode::Embedded,
            instance: ResourceId::new(ResourceKind::Instance, "project-alpha").unwrap(),
            capabilities: vec![
                CapabilityDescriptor {
                    name: CanonicalId::new("logical-archive").unwrap(),
                    contract_version: 1,
                    status: CapabilityStatus::Unavailable,
                    limits: BTreeMap::new(),
                    limitation: Some("F1 implementation gate is not complete".into()),
                },
                CapabilityDescriptor {
                    name: CanonicalId::new("native-persistence").unwrap(),
                    contract_version: 1,
                    status: CapabilityStatus::Experimental,
                    limits: BTreeMap::from([(
                        CanonicalId::new("max-snapshot-bytes").unwrap(),
                        1_073_741_824,
                    )]),
                    limitation: Some(
                        "local alpha; independent-host qualification remains open".into(),
                    ),
                },
            ],
            product_capabilities: ProductCapabilityCatalogue {
                contract_version: PROTOCOL_VERSION,
                capabilities: vec![ProductCapability {
                    id: "backup-create".into(),
                    label: "Backup Create".into(),
                    category: "backup".into(),
                    summary: "Create one authenticated logical backup.".into(),
                    bindings: vec![
                        SurfaceBinding {
                            surface: ProductSurface::Engine,
                            disposition: SurfaceDisposition::Available,
                            entrypoints: vec![
                                "rrd-engine:RrdOperation::CreateInstanceBackup".into()
                            ],
                            reason: None,
                        },
                        SurfaceBinding {
                            surface: ProductSurface::RrdHttp,
                            disposition: SurfaceDisposition::Available,
                            entrypoints: vec!["POST /v1/backups/create".into()],
                            reason: None,
                        },
                        SurfaceBinding {
                            surface: ProductSurface::Mcp,
                            disposition: SurfaceDisposition::Unavailable,
                            entrypoints: Vec::new(),
                            reason: Some("No MCP runtime tool is present in this fixture.".into()),
                        },
                        SurfaceBinding {
                            surface: ProductSurface::Cli,
                            disposition: SurfaceDisposition::Unavailable,
                            entrypoints: Vec::new(),
                            reason: Some("No CLI command is present in this fixture.".into()),
                        },
                        SurfaceBinding {
                            surface: ProductSurface::Connectome,
                            disposition: SurfaceDisposition::Planned,
                            entrypoints: Vec::new(),
                            reason: Some("Connectome exposure is planned in this fixture.".into()),
                        },
                    ],
                }],
            },
        },
        request: RequestEnvelope {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            context: RequestContext {
                request_id: request_id.clone(),
                operation_id: operation_id.clone(),
                idempotency_key: Some(idempotency_key.clone()),
                deadline_unix_ms: Some(1_800_000_000_000),
            },
            resource: ResourcePath {
                segments: vec![
                    ResourceId::new(ResourceKind::Organization, "local").unwrap(),
                    ResourceId::new(ResourceKind::Estate, "developer").unwrap(),
                    ResourceId::new(ResourceKind::Project, "alpha").unwrap(),
                    ResourceId::new(ResourceKind::Instance, "project-alpha").unwrap(),
                ],
            },
            payload: FixturePayload {
                action: "backup.create".into(),
            },
        },
        success: ResponseEnvelope {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.clone(),
            operation_id: operation_id.clone(),
            outcome: ResponseOutcome::Ok {
                payload: FixturePayload {
                    action: "backup.created".into(),
                },
            },
        },
        failure: ResponseEnvelope {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            request_id,
            operation_id,
            outcome: ResponseOutcome::Error {
                error: ErrorBody {
                    code: ErrorCode::FailedPrecondition,
                    message: "source read stamp is no longer current".into(),
                    retryable: true,
                    details: BTreeMap::from([(
                        CanonicalId::new("observed-cursor").unwrap(),
                        "42".into(),
                    )]),
                },
            },
        },
        idempotency: IdempotencyBinding {
            key: idempotency_key,
            operation_sha256: "3c94150b4ea4f9dcb27d3b602e9f190debe656533c047b99e11367bc6a28017f"
                .into(),
        },
    }
}

#[test]
fn public_contract_matches_frozen_golden_json() {
    let fixture = contract_fixture();
    fixture.service.validate().unwrap();
    fixture.request.validate(true).unwrap();
    fixture.success.validate().unwrap();
    fixture.failure.validate().unwrap();
    fixture.idempotency.validate().unwrap();

    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/public-contract-v1.json")).unwrap();
    let actual = serde_json::to_value(&fixture).unwrap();
    assert_eq!(actual, expected);

    let reopened: ContractFixture = serde_json::from_value(expected).unwrap();
    assert_eq!(reopened, fixture);
}

#[test]
fn malformed_identifiers_and_unknown_fields_fail_during_decode() {
    assert!(serde_json::from_str::<CanonicalId>(r#""Not Canonical""#).is_err());
    assert!(CorrelationId::new("request/id").is_err());

    let mut request = serde_json::to_value(&contract_fixture().request).unwrap();
    request
        .as_object_mut()
        .unwrap()
        .insert("unknown".into(), serde_json::json!(true));
    assert!(serde_json::from_value::<RequestEnvelope<FixturePayload>>(request).is_err());
}

#[test]
fn estate_read_contract_has_frozen_field_names_and_strict_empty_payload() {
    let snapshot = EstateSnapshot {
        format_version: 1,
        id: CanonicalId::new("estate-a").unwrap(),
        revision: 7,
        created_at_unix_ms: 10,
        updated_at_unix_ms: 20,
        activity_policy: EstateActivityPolicySnapshot {
            idle_after_ms: 300_000,
            stale_after_ms: 1_800_000,
            neglected_after_ms: 604_800_000,
        },
        instances: Vec::new(),
        operations: Vec::new(),
        idempotency_binding_count: 0,
    };
    let expected = serde_json::json!({
        "format_version": 1,
        "id": "estate-a",
        "revision": 7,
        "created_at_unix_ms": 10,
        "updated_at_unix_ms": 20,
        "activity_policy": {
            "idle_after_ms": 300000,
            "stale_after_ms": 1800000,
            "neglected_after_ms": 604800000
        },
        "instances": [],
        "operations": [],
        "idempotency_binding_count": 0
    });
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), expected);
    assert_eq!(
        serde_json::from_value::<EstateSnapshot>(expected).unwrap(),
        snapshot
    );
    let mutation = EstateMutationResult {
        estate: snapshot,
        idempotent_replay: true,
    };
    let mutation_json = serde_json::to_value(&mutation).unwrap();
    assert_eq!(mutation_json["estate"]["id"], "estate-a");
    assert_eq!(mutation_json["idempotent_replay"], true);
    let mut unknown = mutation_json;
    unknown
        .as_object_mut()
        .unwrap()
        .insert("unknown".into(), serde_json::json!(true));
    assert!(serde_json::from_value::<EstateMutationResult>(unknown).is_err());
    assert_eq!(
        serde_json::from_str::<ReadEstate>("{}").unwrap(),
        ReadEstate {}
    );
    assert!(serde_json::from_str::<ReadEstate>(r#"{"unknown":true}"#).is_err());
}

#[test]
fn estate_backup_jobs_are_a_separate_strict_public_resource() {
    let snapshot = EstateBackupJobsSnapshot {
        estate_id: CanonicalId::new("estate-a").unwrap(),
        estate_revision: 8,
        jobs: vec![EstateBackupJobSnapshot {
            id: CanonicalId::new("backup-daily").unwrap(),
            instance_id: CanonicalId::new("instance-a").unwrap(),
            source_generation: 1,
            label: "daily.0001".into(),
            request_sha256: "a".repeat(64),
            state: EstateBackupJobState::Pending,
            attempts: 0,
            created_at_unix_ms: 80,
            updated_at_unix_ms: 80,
            recovery_policy: None,
            lease: None,
            receipts: Vec::new(),
            backup_id: None,
            archive_sha256: None,
            catalogue_sha256: None,
            error: None,
        }],
    };
    let expected = serde_json::json!({
        "estate_id": "estate-a",
        "estate_revision": 8,
        "jobs": [{
            "id": "backup-daily",
            "instance_id": "instance-a",
            "source_generation": 1,
            "label": "daily.0001",
            "request_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "state": "pending",
            "attempts": 0,
            "created_at_unix_ms": 80,
            "updated_at_unix_ms": 80
        }]
    });
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), expected);
    assert_eq!(
        serde_json::from_value::<EstateBackupJobsSnapshot>(expected.clone()).unwrap(),
        snapshot
    );
    let mut unknown = expected;
    unknown["jobs"][0]["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<EstateBackupJobsSnapshot>(unknown).is_err());
}

#[test]
fn estate_recovery_posture_is_strict_bounded_by_identity_and_path_free() {
    let recovery = serde_json::json!({
        "estate_id": "estate-a",
        "estate_revision": 12,
        "policies": [{
            "instance_id": "instance-a",
            "revision": 1,
            "max_rpo_ms": 86400000,
            "max_rto_ms": 3600000,
            "minimum_recovery_points": 2,
            "retention_ms": 604800000,
            "updated_at_unix_ms": 80
        }],
        "recovery_points": [],
        "retention_pins": [],
        "restore_evidence": [],
        "prune_intents": []
    });
    let decoded: rrd_contract::EstateRecoverySnapshot =
        serde_json::from_value(recovery.clone()).unwrap();
    assert_eq!(decoded.policies[0].revision, 1);
    assert!(!serde_json::to_string(&decoded).unwrap().contains("path"));
    let mut unknown = recovery;
    unknown["policies"][0]["target_path"] = serde_json::json!("/tmp/escape");
    assert!(serde_json::from_value::<rrd_contract::EstateRecoverySnapshot>(unknown).is_err());
}

#[test]
fn query_contract_is_transport_neutral_bounded_and_strict() {
    let request = ExecuteQuery {
        scope: "instance:project-alpha".into(),
        query: "FROM record:document AT VALID 42 KNOWN HEAD PROJECT title".into(),
        parameters: BTreeMap::from([("title".into(), QueryValue::String("alpha".into()))]),
        budget: QueryBudget::default(),
    };
    request.validate().unwrap();
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["scope"], "instance:project-alpha");
    let mut unknown = encoded.clone();
    unknown["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ExecuteQuery>(unknown).is_err());

    let result = QueryResult {
        canonical_query: request.query.clone(),
        scope: request.scope.clone(),
        read_manifest_sha256: "a".repeat(64),
        known_at_cursor: 7,
        schema_revision: 1,
        plan: QueryPlanSnapshot {
            plan_sha256: "b".repeat(64),
            exact: true,
            deterministic_order: "identity_ascending".into(),
            authorization_boundary: "scope:instance:project-alpha".into(),
            candidates: vec![QueryPlanCandidate {
                name: "authoritative_log_scan".into(),
                selected: true,
                exact: true,
                reason: "frozen fixture".into(),
            }],
        },
        execution: QueryExecutionSnapshot {
            scanned_changes: 7,
            stamp_validation: "full_hash_chain_replay".into(),
            stamp_validation_max_changes: 7,
            stamp_validation_proof_nodes: 0,
            returned_rows: 1,
            output_bytes: 32,
            truncated: false,
            analysis: Some(QueryExecutionAnalysisSnapshot {
                engine: "datafusion-55".into(),
                provider_scans: 1,
                input_rows: 2,
                input_batches: 1,
                input_memory_bytes: 512,
                output_batches: 1,
                projection_pushdown: "exact".into(),
                filter_pushdown: "unsupported_exact_post_scan".into(),
                limit_pushdown: "retained_above_scan".into(),
                physical_operators: 5,
                peak_memory_bytes: 1_024,
                spill_count: 0,
                spilled_bytes: 0,
                spilled_rows: 0,
                elapsed_micros: 50,
            }),
        },
        rows: vec![QueryRowSnapshot {
            identity: "document:alpha".into(),
            values: BTreeMap::from([("title".into(), QueryValue::String("Alpha".into()))]),
        }],
    };
    assert_eq!(
        serde_json::from_value::<QueryResult>(serde_json::to_value(&result).unwrap()).unwrap(),
        result
    );

    let mut invalid_budget = request_budget_fixture();
    invalid_budget.max_rows = 0;
    assert!(invalid_budget.validate().is_err());
    let legacy_budget: QueryBudget = serde_json::from_value(serde_json::json!({
        "max_scanned_changes": 100,
        "max_rows": 10,
        "max_output_bytes": 4096,
        "max_batch_rows": 10
    }))
    .unwrap();
    assert_eq!(legacy_budget.max_memory_bytes, 64 * 1024 * 1024);
    assert_eq!(legacy_budget.max_spill_bytes, 256 * 1024 * 1024);
    assert_eq!(legacy_budget.max_elapsed_ms, 30_000);
    legacy_budget.validate().unwrap();
    let mut invalid_memory = legacy_budget;
    invalid_memory.max_memory_bytes = rrd_contract::MAX_QUERY_MEMORY_BYTES + 1;
    assert!(invalid_memory.validate().is_err());
    let mut invalid_parameter = request.clone();
    invalid_parameter
        .parameters
        .insert("nested".into(), QueryValue::List(Vec::new()));
    assert!(invalid_parameter.validate().is_err());
}

#[test]
fn query_transaction_contract_reuses_typed_mutations_and_is_bounded() {
    let request = ExecuteQueryTransaction {
        scope: "instance:project-alpha".into(),
        program: "BEGIN; MUTATE $document; COMMIT;".into(),
        mutation_bindings: BTreeMap::from([(
            "document".into(),
            TransactionMutation::PutRecord {
                reference: DataReference {
                    kind: CanonicalId::new("document").unwrap(),
                    id: CanonicalId::new("alpha").unwrap(),
                },
                valid_from: 100,
                valid_to: None,
                properties: BTreeMap::from([("title".into(), QueryValue::String("Alpha".into()))]),
            },
        )]),
        timeout_ms: 1_000,
    };
    request.validate().unwrap();
    let mut unknown = serde_json::to_value(&request).unwrap();
    unknown["autocommit"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ExecuteQueryTransaction>(unknown).is_err());

    let mut invalid = request.clone();
    invalid.timeout_ms = 0;
    assert!(invalid.validate().is_err());
    let mut invalid = request;
    invalid.mutation_bindings.clear();
    assert!(invalid.validate().is_err());
}

fn request_budget_fixture() -> QueryBudget {
    QueryBudget::default()
}

#[test]
fn live_query_contract_is_resumable_bounded_and_strict() {
    let request = PollLiveQuery {
        scope: "instance:project-alpha".into(),
        query: "FROM record:document AT VALID 42 KNOWN HEAD PROJECT title".into(),
        parameters: BTreeMap::new(),
        after_cursor: 41,
        budget: QueryBudget::default(),
        max_delta_rows: 256,
        wait_timeout_ms: 1_000,
    };
    request.validate().unwrap();
    let mut encoded = serde_json::to_value(&request).unwrap();
    encoded["stream"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PollLiveQuery>(encoded).is_err());
    let mut invalid_wait = request.clone();
    invalid_wait.wait_timeout_ms = 5_001;
    assert!(invalid_wait.validate().is_err());
    let mut invalid = request;
    invalid.max_delta_rows = 0;
    assert!(invalid.validate().is_err());
}

#[test]
fn query_index_administration_contract_is_bounded_and_strict() {
    EnsureQueryIndex {
        scope: "instance:project-alpha".into(),
        index_id: CanonicalId::new("documents-by-status").unwrap(),
        definition_query: "FROM record:document AT VALID 42 KNOWN HEAD PROJECT status, title"
            .into(),
        unique: false,
        kind: QueryIndexKind::Scalar,
        full_text: None,
        budget: QueryBudget::default(),
    }
    .validate()
    .unwrap();
    EnsureVectorIndex {
        scope: "instance:project-alpha".into(),
        collection_id: CanonicalId::new("documents").unwrap(),
        vector_name: CanonicalId::new("title").unwrap(),
        configuration: VectorIndexConfiguration::TurboQuant {
            bits: VectorQuantizationBits::Bits2,
            seed: 11,
            filter_properties: vec![CanonicalId::new("tenant").unwrap()],
        },
        max_scanned_changes: 10_000,
    }
    .validate()
    .unwrap();
    ListQueryIndexes {
        scope: "instance:project-alpha".into(),
    }
    .validate()
    .unwrap();
    assert!(ListQueryIndexes {
        scope: String::new()
    }
    .validate()
    .is_err());
}

#[test]
fn vector_collection_contract_is_named_bounded_and_search_addressable() {
    let vector = NamedVectorDefinition {
        name: CanonicalId::new("title").unwrap(),
        field: CanonicalId::new("title_embedding").unwrap(),
        kind: VectorValueKind::Dense,
        dimensions: 2,
        metric: VectorSearchMetric::Cosine,
        embedding_model: None,
        memory_tier: VectorMemoryTier::Cached,
    };
    EnsureVectorCollection {
        scope: "instance:project-alpha".into(),
        collection_id: CanonicalId::new("documents").unwrap(),
        vectors: vec![vector.clone()],
    }
    .validate()
    .unwrap();
    ListVectorCollections {
        scope: "instance:project-alpha".into(),
    }
    .validate()
    .unwrap();
    EnsureVectorIndex {
        scope: "instance:project-alpha".into(),
        collection_id: CanonicalId::new("documents").unwrap(),
        vector_name: CanonicalId::new("title").unwrap(),
        configuration: VectorIndexConfiguration::Hnsw {
            m: 16,
            ef_construction: 100,
            max_level: 8,
            seed: 7,
            filter_properties: vec![CanonicalId::new("tenant").unwrap()],
        },
        max_scanned_changes: 10_000,
    }
    .validate()
    .unwrap();
    SearchVectors {
        scope: "instance:project-alpha".into(),
        valid_at: 42,
        collection_id: Some(CanonicalId::new("documents").unwrap()),
        vector_name: Some(vector.name),
        field: None,
        query: VectorSearchQuery::Dense {
            values: vec![0.6, 0.8],
        },
        filter: Some(VectorPayloadFilter::Condition {
            condition: VectorPayloadCondition {
                property: CanonicalId::new("tenant").unwrap(),
                operator: VectorPayloadOperator::Equals {
                    value: QueryValue::String("alpha".into()),
                },
            },
        }),
        metric: None,
        top_k: 10,
        mode: VectorSearchMode::Exact,
        max_scanned_changes: 100,
    }
    .validate()
    .unwrap();
    let approximate = SearchVectors {
        scope: "instance:project-alpha".into(),
        valid_at: 42,
        collection_id: Some(CanonicalId::new("documents").unwrap()),
        vector_name: Some(CanonicalId::new("title").unwrap()),
        field: None,
        query: VectorSearchQuery::Dense {
            values: vec![0.6, 0.8],
        },
        filter: None,
        metric: None,
        top_k: 10,
        mode: VectorSearchMode::RequireApproximate {
            exact_rerank: 20,
            ef_search: 100,
        },
        max_scanned_changes: 100,
    };
    approximate.validate().unwrap();
    SearchHybrid {
        scope: "instance:project-alpha".into(),
        valid_at: 42,
        document_kind: CanonicalId::new("document").unwrap(),
        text_field: CanonicalId::new("title").unwrap(),
        text_query: "reason ready flow".into(),
        collection_id: CanonicalId::new("documents").unwrap(),
        vector_name: CanonicalId::new("title").unwrap(),
        vector_query: VectorSearchQuery::Dense {
            values: vec![0.6, 0.8],
        },
        vector_filter: None,
        vector_mode: VectorSearchMode::RequireApproximate {
            exact_rerank: 20,
            ef_search: 100,
        },
        fusion: HybridFusion::ReciprocalRank {
            rank_constant: 60,
            text_weight_millionths: 1_000_000,
            vector_weight_millionths: 1_000_000,
        },
        top_k: 10,
        candidate_k: 20,
        max_scanned_changes: 100,
    }
    .validate()
    .unwrap();
    let mut invalid_approximate = approximate;
    invalid_approximate.mode = VectorSearchMode::AllowApproximate {
        exact_rerank: 9,
        ef_search: 100,
    };
    assert!(invalid_approximate.validate().is_err());
    assert!(SearchVectors {
        scope: "instance:project-alpha".into(),
        valid_at: 42,
        collection_id: Some(CanonicalId::new("documents").unwrap()),
        vector_name: None,
        field: None,
        query: VectorSearchQuery::Dense {
            values: vec![0.6, 0.8],
        },
        filter: None,
        metric: None,
        top_k: 10,
        mode: VectorSearchMode::Exact,
        max_scanned_changes: 100,
    }
    .validate()
    .is_err());
    let point: TransactionMutation = serde_json::from_value(serde_json::json!({
        "mutation": "put_vector",
        "reference": {"kind": "embedding", "id": "alpha-title"},
        "subject": {"kind": "document", "id": "alpha"},
        "collection_id": "documents",
        "vector_name": "title",
        "field": "title_embedding",
        "valid_from": 42,
        "value": {"kind": "dense", "values": [0.6, 0.8]},
        "properties": {"tenant": {"type": "string", "value": "alpha"}}
    }))
    .unwrap();
    point.validate().unwrap();
    let mut incomplete = serde_json::to_value(point).unwrap();
    incomplete.as_object_mut().unwrap().remove("vector_name");
    assert!(serde_json::from_value::<TransactionMutation>(incomplete)
        .unwrap()
        .validate()
        .is_err());
}

#[test]
fn changefeed_request_is_cursor_addressed_bounded_and_strict() {
    let request = ReadChangefeed {
        scope: "instance:project-alpha".into(),
        after_cursor: 41,
        limit: 256,
    };
    request.validate().unwrap();
    let mut encoded = serde_json::to_value(&request).unwrap();
    encoded["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ReadChangefeed>(encoded).is_err());

    let mut invalid = request.clone();
    invalid.limit = 0;
    assert!(invalid.validate().is_err());
    invalid.limit = rrd_contract::MAX_CHANGEFEED_PAGE + 1;
    assert!(invalid.validate().is_err());

    let mut follow = FollowChangefeed {
        read: request,
        wait_timeout_ms: 1_000,
    };
    follow.validate().unwrap();
    follow.wait_timeout_ms = rrd_contract::MAX_CHANGEFEED_WAIT_MS + 1;
    assert!(follow.validate().is_err());
}

#[test]
fn managed_backup_contract_accepts_no_filesystem_paths() {
    CreateInstanceBackup {
        label: "before-upgrade".into(),
        created_at_unix_ms: 500,
    }
    .validate()
    .unwrap();

    RestoreInstanceBackup {
        backup_sha256: "a".repeat(64),
        restore_id: CanonicalId::new("restore-a").unwrap(),
        restored_at_unix_ms: 600,
    }
    .validate()
    .unwrap();

    let with_path = serde_json::json!({
        "label": "before-upgrade",
        "created_at_unix_ms": 500,
        "target": "/tmp/escape"
    });
    assert!(serde_json::from_value::<CreateInstanceBackup>(with_path).is_err());

    let invalid = RestoreInstanceBackup {
        backup_sha256: "not-a-digest".into(),
        restore_id: CanonicalId::new("restore-a").unwrap(),
        restored_at_unix_ms: 600,
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn audit_read_contract_is_bounded_and_strict() {
    ReadAudit {
        after_sequence: 41,
        limit: 128,
    }
    .validate()
    .unwrap();
    assert!(ReadAudit {
        after_sequence: 0,
        limit: 0,
    }
    .validate()
    .is_err());
    assert!(serde_json::from_value::<ReadAudit>(serde_json::json!({
        "after_sequence": 0,
        "limit": 1,
        "include_bodies": true
    }))
    .is_err());
}

#[test]
fn diagnostic_read_contract_is_bounded_strict_and_separately_authorized() {
    rrd_contract::ReadDiagnosticSnapshot {
        scope: "instance:alpha".into(),
        graph_valid_at_unix_ms: 1_000,
        graph_known_at_cursor: Some(30),
        graph_compare_cursor: 10,
        runtime_max_scanned_changes: 1_024,
        changes_after_cursor: 10,
        change_limit: 128,
        audit_after_sequence: 20,
        audit_limit: 128,
    }
    .validate()
    .unwrap();
    assert!(rrd_contract::ReadDiagnosticSnapshot {
        scope: "instance:alpha".into(),
        graph_valid_at_unix_ms: 1_000,
        graph_known_at_cursor: Some(30),
        graph_compare_cursor: 10,
        runtime_max_scanned_changes: 1_024,
        changes_after_cursor: 0,
        change_limit: 0,
        audit_after_sequence: 0,
        audit_limit: 1,
    }
    .validate()
    .is_err());
    assert!(
        serde_json::from_value::<rrd_contract::ReadDiagnosticSnapshot>(serde_json::json!({
            "scope": "instance:alpha",
            "graph_valid_at_unix_ms": 1000,
            "graph_known_at_cursor": 30,
            "graph_compare_cursor": 10,
            "runtime_max_scanned_changes": 1024,
            "changes_after_cursor": 0,
            "change_limit": 1,
            "audit_after_sequence": 0,
            "audit_limit": 1,
            "physical_store": "/tmp/escape"
        }))
        .is_err()
    );

    let descriptor = rrd_contract::endpoint_catalogue()
        .endpoints
        .into_iter()
        .find(|endpoint| endpoint.operation.as_str() == "diagnostics-read")
        .unwrap();
    assert_eq!(descriptor.path, "/v1/diagnostics/read");
    assert_eq!(
        descriptor.action.fixed_action(),
        Some(rrd_contract::SecurityAction::DiagnosticsRead)
    );
    assert!(!descriptor.mutation);
}

#[test]
fn endpoint_catalogue_is_complete_sorted_and_transport_neutral() {
    let catalogue = rrd_contract::endpoint_catalogue();
    catalogue.validate().unwrap();
    assert_eq!(catalogue.endpoints.len(), 31);
    assert_eq!(catalogue.endpoints[0].operation.as_str(), "audit-read");
    let create = catalogue
        .endpoints
        .iter()
        .find(|endpoint| endpoint.operation.as_str() == "session-create")
        .unwrap();
    assert_eq!(create.path, "/v1/sessions");
    assert!(create.mutation);
    assert_eq!(
        create.authentication,
        rrd_contract::EndpointAuthentication::ApiKey
    );
    assert!(catalogue.endpoints.iter().all(|endpoint| {
        !endpoint.request_type.contains("::") && !endpoint.response_type.contains("::")
    }));
    let runtime_invoke = catalogue
        .endpoints
        .iter()
        .find(|endpoint| endpoint.operation.as_str() == "runtime-tool-invoke")
        .unwrap();
    assert_eq!(
        runtime_invoke.action,
        rrd_contract::EndpointAction::RuntimeToolDescriptor
    );
    assert!(runtime_invoke.action.fixed_action().is_none());
    let runtime_list = catalogue
        .endpoints
        .iter()
        .find(|endpoint| endpoint.operation.as_str() == "runtime-tool-catalogue-read")
        .unwrap();
    assert_eq!(
        runtime_list.action.fixed_action(),
        Some(rrd_contract::SecurityAction::RuntimeToolCatalogueRead)
    );
    let encoded = serde_json::to_value(&catalogue).unwrap();
    assert_eq!(
        serde_json::from_value::<rrd_contract::EndpointCatalogue>(encoded).unwrap(),
        catalogue
    );
}

#[test]
fn openapi_is_derived_from_every_catalogue_operation_and_wire_type() {
    let catalogue = rrd_contract::endpoint_catalogue();
    let document = rrd_contract::openapi_document().unwrap();
    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(document["x-rrd-protocol"], PROTOCOL);
    assert_eq!(document["x-rrd-protocol-version"], PROTOCOL_VERSION);
    assert_eq!(document["x-rrd-endpoint-count"], catalogue.endpoints.len());
    for endpoint in catalogue.endpoints {
        let method = match endpoint.method {
            rrd_contract::HttpMethod::Get => "get",
            rrd_contract::HttpMethod::Post => "post",
            rrd_contract::HttpMethod::Delete => "delete",
        };
        let operation = &document["paths"][&endpoint.path][method];
        assert_eq!(operation["operationId"], endpoint.operation.as_str());
        assert!(operation["responses"]["200"]["content"]["application/json"]["schema"].is_object());
        if endpoint.method != rrd_contract::HttpMethod::Get {
            assert!(operation["requestBody"]["content"]["application/json"]["schema"].is_object());
        }
    }
    let encoded = serde_json::to_string(&document).unwrap();
    assert!(!encoded.contains("rrd_store"));
    assert!(!encoded.contains("rrd_server"));
    let mut pretty = serde_json::to_vec_pretty(&document).unwrap();
    pretty.push(b'\n');
    assert_eq!(
        sha256_for_test(&pretty),
        rrd_contract::OPENAPI_DOCUMENT_SHA256,
        "OpenAPI drift requires an intentional protocol/schema review"
    );
}

fn sha256_for_test(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(&mut output, "{byte:02x}").unwrap();
            output
        })
}

#[test]
fn mutation_idempotency_protocol_and_capability_order_fail_closed() {
    let fixture = contract_fixture();
    let mut request = fixture.request;
    request.context.idempotency_key = None;
    assert!(request.validate(true).is_err());
    assert!(request.validate(false).is_ok());

    request.protocol_version += 1;
    assert!(request.validate(false).is_err());

    let mut service = fixture.service;
    service.capabilities.reverse();
    assert!(service.validate().is_err());
    service
        .capabilities
        .sort_by(|left, right| left.name.cmp(&right.name));
    service.capabilities.push(service.capabilities[0].clone());
    assert!(service.validate().is_err());
}

#[test]
fn resource_paths_are_explicit_bounded_and_non_repeating() {
    let empty = ResourcePath { segments: vec![] };
    assert!(empty.validate().is_err());

    let repeated = ResourcePath {
        segments: vec![
            ResourceId::new(ResourceKind::Project, "alpha").unwrap(),
            ResourceId::new(ResourceKind::Project, "beta").unwrap(),
        ],
    };
    assert!(repeated.validate().is_err());
}

#[test]
fn session_and_claim_transaction_contracts_are_bounded() {
    let limits = SessionLimits {
        idle_timeout_ms: 30_000,
        absolute_timeout_ms: 300_000,
        max_open_transactions: 4,
    };
    limits.validate().unwrap();
    SessionLease {
        session_id: CorrelationId::new("session-1").unwrap(),
        token: CorrelationId::new("token-1").unwrap(),
        issued_at_unix_ms: 100,
        idle_expires_at_unix_ms: 30_100,
        absolute_expires_at_unix_ms: 300_100,
        limits: limits.clone(),
    }
    .validate()
    .unwrap();
    BeginTransaction {
        scope: CanonicalId::new("instance-a").unwrap(),
        timeout_ms: 10_000,
    }
    .validate()
    .unwrap();
    let commit = CommitTransaction {
        operation_sha256: "3c94150b4ea4f9dcb27d3b602e9f190debe656533c047b99e11367bc6a28017f".into(),
        mutations: vec![TransactionMutation::AssertClaim {
            subject: CanonicalId::new("task-1").unwrap(),
            predicate: CanonicalId::new("status").unwrap(),
            object: "verified".into(),
            valid_from: 100,
            tx_time: 101,
            producer: CanonicalId::new("agent-test").unwrap(),
            confidence: Some(0.9),
        }],
    };
    commit.validate().unwrap();
    CommitReceipt {
        transaction_id: CorrelationId::new("transaction-1").unwrap(),
        operation_sha256: "3c94150b4ea4f9dcb27d3b602e9f190debe656533c047b99e11367bc6a28017f".into(),
        first_claim_sequence: 7,
        last_claim_sequence: 7,
        mutation_count: 1,
        runtime_commit_sha256: None,
        first_runtime_cursor: None,
        last_runtime_cursor: None,
        claim_mutation_count: None,
        idempotent_replay: false,
    }
    .validate()
    .unwrap();
    assert_eq!(TransactionState::Open, TransactionState::Open);

    let mut invalid = limits;
    invalid.idle_timeout_ms = invalid.absolute_timeout_ms + 1;
    assert!(invalid.validate().is_err());
    let mut invalid_commit = commit;
    let TransactionMutation::AssertClaim { confidence, .. } = &mut invalid_commit.mutations[0]
    else {
        unreachable!()
    };
    *confidence = Some(f32::NAN);
    assert!(invalid_commit.validate().is_err());
}

#[test]
fn lifecycle_health_and_preview_payloads_are_bounded_and_strict() {
    let mutation = TransactionMutation::AssertClaim {
        subject: CanonicalId::new("task-1").unwrap(),
        predicate: CanonicalId::new("status").unwrap(),
        object: "verified".into(),
        valid_from: 100,
        tx_time: 101,
        producer: CanonicalId::new("agent-test").unwrap(),
        confidence: None,
    };
    let preview = PreviewTransaction {
        mutations: vec![mutation.clone()],
    };
    preview.validate().unwrap();
    TransactionPreview {
        transaction_id: CorrelationId::new("transaction-1").unwrap(),
        read_cursor: 7,
        operation_sha256: "3c94150b4ea4f9dcb27d3b602e9f190debe656533c047b99e11367bc6a28017f".into(),
        mutations: preview.mutations,
    }
    .validate()
    .unwrap();
    SessionTermination {
        session_id: CorrelationId::new("session-1").unwrap(),
        state: SessionEndState::Closed,
        ended_at_unix_ms: 1,
        affected_open_transactions: 2,
        idempotent_replay: false,
    }
    .validate()
    .unwrap();
    Liveness {
        observed_at_unix_ms: 1,
    }
    .validate()
    .unwrap();
    Readiness {
        observed_at_unix_ms: 1,
        claim_sequence: 2,
        runtime_cursor: 3,
        backend: CanonicalId::new("rrd-lsm").unwrap(),
    }
    .validate()
    .unwrap();

    assert!(serde_json::from_str::<RenewSession>(r#"{"unknown":true}"#).is_err());
    assert!(serde_json::from_str::<CloseSession>(r#"{"unknown":true}"#).is_err());
    assert!(
        serde_json::from_str::<PreviewTransaction>(r#"{"mutations":[],"unknown":true}"#).is_err()
    );
}

#[test]
fn transaction_digest_has_a_cross_language_golden_vector() {
    let first: Vec<TransactionMutation> = serde_json::from_str(
        r#"[{"mutation":"assert_claim","subject":"task-1","predicate":"status","object":"verified","valid_from":100,"tx_time":101,"producer":"agent-test"}]"#,
    )
    .unwrap();
    let reordered: Vec<TransactionMutation> = serde_json::from_str(
        r#"[{"producer":"agent-test","tx_time":101,"valid_from":100,"object":"verified","predicate":"status","subject":"task-1","mutation":"assert_claim"}]"#,
    )
    .unwrap();
    let expected = "d85547ca333304b54db4bc0249b6e11f83cf975fdc7cfebcf551ec6564bb71f8";
    assert_eq!(transaction_operation_sha256(&first), expected);
    assert_eq!(transaction_operation_sha256(&reordered), expected);
}

#[test]
fn unified_catalogue_is_strict_round_trippable_and_rejects_mode_confusion() {
    let document = CanonicalId::new("document").unwrap();
    let mut registry = DataSchemaRegistry {
        revision: 1,
        migration: "freeze public catalogue".into(),
        catalogue: DataCatalogueIdentity {
            namespace: CanonicalId::new("project").unwrap(),
            database: CanonicalId::new("runtime").unwrap(),
        },
        tables: BTreeMap::from([
            (
                document.clone(),
                DataTableSchema {
                    model: DataLogicalModel::Document,
                    mode: DataSchemaMode::Strict,
                    properties: BTreeMap::new(),
                    allow_additional_properties: false,
                },
            ),
            (
                CanonicalId::new("kv").unwrap(),
                DataTableSchema {
                    model: DataLogicalModel::KeyValue,
                    mode: DataSchemaMode::Schemaless,
                    properties: BTreeMap::new(),
                    allow_additional_properties: false,
                },
            ),
        ]),
        records: BTreeMap::from([(document.clone(), DataRecordSchema::default())]),
        relations: BTreeMap::new(),
        events: BTreeMap::new(),
    };
    let mutation = TransactionMutation::PutSchema {
        registry: registry.clone(),
    };
    mutation.validate().unwrap();
    let encoded = serde_json::to_value(&mutation).unwrap();
    let decoded: TransactionMutation = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, mutation);

    registry.tables.get_mut(&document).unwrap().mode = DataSchemaMode::Schemaless;
    assert!(TransactionMutation::PutSchema { registry }
        .validate()
        .is_err());
}

#[test]
fn data_retirement_targets_are_model_typed_and_event_cursor_addressed() {
    let document = DataTarget::Reference {
        reference: DataReference {
            kind: CanonicalId::new("document").unwrap(),
            id: CanonicalId::new("alpha").unwrap(),
        },
    };
    TransactionMutation::RetireData {
        model: DataLogicalModel::Document,
        target: document.clone(),
        effective_at: 10,
    }
    .validate()
    .unwrap();
    let event = DataTarget::Event {
        kind: CanonicalId::new("observed").unwrap(),
        cursor: 7,
    };
    let retirement = TransactionMutation::RetireData {
        model: DataLogicalModel::Event,
        target: event,
        effective_at: 10,
    };
    retirement.validate().unwrap();
    assert_eq!(
        serde_json::from_value::<TransactionMutation>(serde_json::to_value(&retirement).unwrap())
            .unwrap(),
        retirement
    );
    assert!(TransactionMutation::RetireData {
        model: DataLogicalModel::Event,
        target: document,
        effective_at: 10,
    }
    .validate()
    .is_err());
    assert!(TransactionMutation::RetireData {
        model: DataLogicalModel::Vector,
        target: DataTarget::Event {
            kind: CanonicalId::new("observed").unwrap(),
            cursor: 7,
        },
        effective_at: 10,
    }
    .validate()
    .is_err());
    assert!(ReadDataSnapshot {
        valid_at: 10,
        max_scanned_changes: 1,
    }
    .validate()
    .is_ok());
    assert!(ReadDataSnapshot {
        valid_at: 0,
        max_scanned_changes: 0,
    }
    .validate()
    .is_err());
}
