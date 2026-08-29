use super::*;
use rrd_contract::{
    BackupCoverageSnapshot, CreateInstanceBackup, DataProperties, DataPropertySchema,
    DataRecordSchema, DataReference, DataSchemaRegistry, DataValueType, DataVectorValue,
    EnsureQueryIndex, EnsureVectorIndex, HybridFusion, QueryBudget, QueryIndexKind, QueryValue,
    RestoreInstanceBackup, SearchHybrid, SearchVectors, VectorIndexConfiguration,
    VectorQuantizationBits, VectorSearchMode, VectorSearchQuery,
};
use std::collections::BTreeMap;

fn reference(kind: &str, id: &str) -> DataReference {
    DataReference {
        kind: CanonicalId::new(kind).unwrap(),
        id: CanonicalId::new(id).unwrap(),
    }
}

fn vector_mutation(id: &str, values: Vec<f32>) -> TransactionMutation {
    TransactionMutation::PutVector {
        reference: reference("embedding", id),
        subject: reference("document", id),
        collection_id: Some(CanonicalId::new("documents").unwrap()),
        vector_name: Some(CanonicalId::new("body").unwrap()),
        field: CanonicalId::new("body-embedding").unwrap(),
        valid_from: 1,
        valid_to: None,
        value: DataVectorValue::Dense { values },
        provenance: None,
        properties: DataProperties::new(),
    }
}

fn document_mutation(id: &str, body: &str) -> TransactionMutation {
    TransactionMutation::PutRecord {
        reference: reference("document", id),
        valid_from: 1,
        valid_to: None,
        properties: DataProperties::from([("body".into(), QueryValue::String(body.into()))]),
    }
}

fn commit_vectors(
    engine: &RrdEngine,
    lease: &rrd_contract::SessionLease,
    ordinal: u64,
    mutations: Vec<TransactionMutation>,
) {
    let transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &BeginTransaction {
                scope: CanonicalId::new("data").unwrap(),
                timeout_ms: 1_000,
            },
            &mutation_context(
                &id(&format!("begin-key-{ordinal}")),
                &format!("request-begin-{ordinal}"),
                &format!("operation-begin-{ordinal}"),
            ),
            ordinal,
        )
        .unwrap();
    let request = CommitTransaction {
        operation_sha256: rrd_contract::transaction_operation_sha256(&mutations),
        mutations,
    };
    engine
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id(&format!("commit-key-{ordinal}")),
            &request,
            ordinal + 1,
            &format!("request-commit-{ordinal}"),
            &format!("operation-commit-{ordinal}"),
        )
        .unwrap();
}

fn index_request() -> EnsureVectorIndex {
    EnsureVectorIndex {
        scope: format!("instance:{}", instance()),
        collection_id: CanonicalId::new("documents").unwrap(),
        vector_name: CanonicalId::new("body").unwrap(),
        configuration: VectorIndexConfiguration::Hnsw {
            m: 4,
            ef_construction: 8,
            max_level: 4,
            seed: 17,
            filter_properties: Vec::new(),
        },
        max_scanned_changes: 10_000,
    }
}

fn turboquant_request() -> EnsureVectorIndex {
    EnsureVectorIndex {
        scope: format!("instance:{}", instance()),
        collection_id: CanonicalId::new("documents").unwrap(),
        vector_name: CanonicalId::new("body").unwrap(),
        configuration: VectorIndexConfiguration::TurboQuant {
            bits: VectorQuantizationBits::Bits2,
            seed: 23,
            filter_properties: Vec::new(),
        },
        max_scanned_changes: 10_000,
    }
}

fn search_request(mode: VectorSearchMode) -> SearchVectors {
    SearchVectors {
        scope: format!("instance:{}", instance()),
        valid_at: 1,
        collection_id: Some(CanonicalId::new("documents").unwrap()),
        vector_name: Some(CanonicalId::new("body").unwrap()),
        field: None,
        query: VectorSearchQuery::Dense {
            values: vec![1.0, 0.0],
        },
        filter: None,
        metric: None,
        top_k: 1,
        mode,
        max_scanned_changes: 10_000,
    }
}

#[test]
fn persistent_retrieval_indexes_and_hybrid_fusion_survive_reopen_and_staleness() {
    let root = tempfile::tempdir().unwrap();
    let engine = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    let lease = engine
        .create_session(
            &session_request(9_000, 4),
            &id("vector-session"),
            100,
            "request-session",
            "operation-session",
        )
        .unwrap();
    engine
        .ensure_vector_collection(
            &lease.session_id,
            &lease.token,
            &EnsureVectorCollection {
                scope: format!("instance:{}", instance()),
                collection_id: CanonicalId::new("documents").unwrap(),
                vectors: vec![NamedVectorDefinition {
                    name: CanonicalId::new("body").unwrap(),
                    field: CanonicalId::new("body-embedding").unwrap(),
                    kind: VectorValueKind::Dense,
                    dimensions: 2,
                    metric: VectorSearchMetric::Dot,
                    embedding_model: None,
                    memory_tier: VectorMemoryTier::Cached,
                }],
            },
            &mutation_context(
                &id("vector-collection"),
                "request-collection",
                "operation-collection",
            ),
            200,
        )
        .unwrap();
    commit_vectors(
        &engine,
        &lease,
        300,
        vec![
            TransactionMutation::PutSchema {
                registry: DataSchemaRegistry {
                    revision: 1,
                    migration: "install vector fixture schema".into(),
                    catalogue: DataCatalogueIdentity::default(),
                    tables: BTreeMap::new(),
                    records: BTreeMap::from([(
                        CanonicalId::new("document").unwrap(),
                        DataRecordSchema {
                            properties: BTreeMap::from([(
                                "body".into(),
                                DataPropertySchema {
                                    value_type: DataValueType::String,
                                    required: true,
                                },
                            )]),
                            allow_additional_properties: true,
                            ..DataRecordSchema::default()
                        },
                    )]),
                    relations: BTreeMap::new(),
                    events: BTreeMap::new(),
                },
            },
            document_mutation("alpha", "rrflow durable reasoning"),
            document_mutation("beta", "legacy unrelated storage"),
            document_mutation("gamma", "rrflow graph context"),
            vector_mutation("alpha", vec![1.0, 0.0]),
            vector_mutation("beta", vec![0.0, 1.0]),
            vector_mutation("gamma", vec![0.7, 0.3]),
        ],
    );

    let first = engine
        .ensure_vector_index(
            &lease.session_id,
            &lease.token,
            &index_request(),
            400,
            "request-index-1",
            "operation-index-1",
        )
        .unwrap();
    assert_eq!(first.index.kind.as_str(), "hnsw");
    assert_eq!(first.index.generation, 1);
    assert_eq!(first.index.indexed_vectors, 3);
    assert!(!first.idempotent_replay);

    let replay = engine
        .ensure_vector_index(
            &lease.session_id,
            &lease.token,
            &index_request(),
            401,
            "request-index-replay",
            "operation-index-replay",
        )
        .unwrap();
    assert_eq!(replay.index, first.index);
    assert!(replay.idempotent_replay);

    let approximate = search_request(VectorSearchMode::RequireApproximate {
        exact_rerank: 3,
        ef_search: 3,
    });
    let before_restart = engine
        .search_vectors(
            &lease.session_id,
            &lease.token,
            &approximate,
            500,
            "request-search-1",
            "operation-search-1",
        )
        .unwrap();
    assert_eq!(before_restart.access_path.as_str(), "hnsw");
    assert!(!before_restart.exact);
    assert_eq!(before_restart.hits[0].reference.id.as_str(), "alpha");
    drop(engine);

    let reopened = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    let after_restart = reopened
        .search_vectors(
            &lease.session_id,
            &lease.token,
            &approximate,
            501,
            "request-search-2",
            "operation-search-2",
        )
        .unwrap();
    assert_eq!(after_restart.access_path.as_str(), "hnsw");
    assert_eq!(after_restart.plan_sha256, before_restart.plan_sha256);

    commit_vectors(
        &reopened,
        &lease,
        600,
        vec![
            document_mutation("delta", "durable update"),
            vector_mutation("delta", vec![0.9, 0.1]),
        ],
    );
    let stale = reopened.search_vectors(
        &lease.session_id,
        &lease.token,
        &approximate,
        700,
        "request-search-stale",
        "operation-search-stale",
    );
    assert!(matches!(stale, Err(ServiceError::Vector(_))));

    let fallback = reopened
        .search_vectors(
            &lease.session_id,
            &lease.token,
            &search_request(VectorSearchMode::AllowApproximate {
                exact_rerank: 4,
                ef_search: 4,
            }),
            701,
            "request-search-fallback",
            "operation-search-fallback",
        )
        .unwrap();
    assert_eq!(fallback.access_path.as_str(), "exact_scan");
    assert!(fallback.exact);

    let rebuilt = reopened
        .ensure_vector_index(
            &lease.session_id,
            &lease.token,
            &index_request(),
            800,
            "request-index-2",
            "operation-index-2",
        )
        .unwrap();
    assert_eq!(rebuilt.index.generation, 2);
    assert_eq!(rebuilt.index.indexed_vectors, 4);
    assert!(rebuilt.index.source_cursor > first.index.source_cursor);
    let after_rebuild = reopened
        .search_vectors(
            &lease.session_id,
            &lease.token,
            &search_request(VectorSearchMode::RequireApproximate {
                exact_rerank: 4,
                ef_search: 4,
            }),
            801,
            "request-search-rebuilt",
            "operation-search-rebuilt",
        )
        .unwrap();
    assert_eq!(after_rebuild.access_path.as_str(), "hnsw");

    let turboquant = reopened
        .ensure_vector_index(
            &lease.session_id,
            &lease.token,
            &turboquant_request(),
            850,
            "request-turboquant-1",
            "operation-turboquant-1",
        )
        .unwrap();
    assert_eq!(turboquant.index.kind.as_str(), "turboquant");
    assert_eq!(turboquant.index.generation, 1);
    assert_eq!(turboquant.index.indexed_vectors, 4);
    assert!(
        turboquant.index.packed_vector_bytes.unwrap()
            < turboquant.index.full_precision_vector_bytes.unwrap()
    );
    assert!(!turboquant.idempotent_replay);
    let turboquant_replay = reopened
        .ensure_vector_index(
            &lease.session_id,
            &lease.token,
            &turboquant_request(),
            851,
            "request-turboquant-replay",
            "operation-turboquant-replay",
        )
        .unwrap();
    assert_eq!(turboquant_replay.index, turboquant.index);
    assert!(turboquant_replay.idempotent_replay);

    let turbo_search = reopened
        .search_vectors(
            &lease.session_id,
            &lease.token,
            &search_request(VectorSearchMode::RequireApproximate {
                exact_rerank: 4,
                ef_search: 4,
            }),
            860,
            "request-turbo-search-1",
            "operation-turbo-search-1",
        )
        .unwrap();
    assert_eq!(turbo_search.access_path.as_str(), "turboquant");
    assert_eq!(turbo_search.hits[0].reference.id.as_str(), "alpha");
    assert_eq!(turbo_search.hits[0].score, 1.0);
    drop(reopened);

    let reopened = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    let reopened_turbo = reopened
        .search_vectors(
            &lease.session_id,
            &lease.token,
            &search_request(VectorSearchMode::RequireApproximate {
                exact_rerank: 4,
                ef_search: 4,
            }),
            861,
            "request-turbo-search-2",
            "operation-turbo-search-2",
        )
        .unwrap();
    assert_eq!(reopened_turbo.access_path.as_str(), "turboquant");
    assert_eq!(reopened_turbo.plan_sha256, turbo_search.plan_sha256);

    let bm25 = reopened
        .ensure_query_index(
            &lease.session_id,
            &lease.token,
            &id("hybrid-bm25-index"),
            &EnsureQueryIndex {
                scope: format!("instance:{}", instance()),
                index_id: CanonicalId::new("document-body-bm25").unwrap(),
                definition_query: "FROM record:document AT VALID 1 KNOWN HEAD PROJECT body".into(),
                unique: false,
                kind: QueryIndexKind::Bm25,
                budget: QueryBudget::default(),
            },
            900,
            "request-hybrid-bm25",
            "operation-hybrid-bm25",
        )
        .unwrap();
    assert_eq!(bm25.index.kind, QueryIndexKind::Bm25);

    let hybrid_request = SearchHybrid {
        scope: format!("instance:{}", instance()),
        valid_at: 1,
        document_kind: CanonicalId::new("document").unwrap(),
        text_field: CanonicalId::new("body").unwrap(),
        text_query: "rrflow".into(),
        collection_id: CanonicalId::new("documents").unwrap(),
        vector_name: CanonicalId::new("body").unwrap(),
        vector_query: VectorSearchQuery::Dense {
            values: vec![1.0, 0.0],
        },
        vector_filter: None,
        vector_mode: VectorSearchMode::RequireApproximate {
            exact_rerank: 4,
            ef_search: 4,
        },
        fusion: HybridFusion::ReciprocalRank {
            rank_constant: 60,
            text_weight_millionths: 1_000_000,
            vector_weight_millionths: 1_000_000,
        },
        top_k: 3,
        candidate_k: 4,
        max_scanned_changes: 10_000,
    };
    let hybrid = reopened
        .search_hybrid(
            &lease.session_id,
            &lease.token,
            &hybrid_request,
            901,
            "request-hybrid-1",
            "operation-hybrid-1",
        )
        .unwrap();
    assert_eq!(hybrid.text_access_path.as_str(), "bm25_index");
    assert_eq!(hybrid.vector_access_path.as_str(), "turboquant");
    assert!(!hybrid.vector_exact);
    assert_eq!(hybrid.hits[0].subject.id.as_str(), "alpha");
    assert_eq!(hybrid.hits[0].text_rank, Some(1));
    assert_eq!(hybrid.hits[0].vector_rank, Some(1));
    assert_eq!(hybrid.read_manifest_sha256.len(), 64);
    assert_eq!(hybrid.fusion_plan_sha256.len(), 64);
    drop(reopened);

    let reopened = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    let replayed_hybrid = reopened
        .search_hybrid(
            &lease.session_id,
            &lease.token,
            &hybrid_request,
            902,
            "request-hybrid-2",
            "operation-hybrid-2",
        )
        .unwrap();
    assert_eq!(replayed_hybrid, hybrid);
}

#[test]
fn application_backup_restores_turboquant_payload_before_instance_activation() {
    let root = tempfile::tempdir().unwrap();
    let source_root = root.path().join("source");
    let engine = RrdEngine::open(&source_root, instance(), TOKEN_KEY).unwrap();
    let lease = engine
        .create_session(
            &session_request(9_000, 4),
            &id("backup-vector-session"),
            100,
            "request-session",
            "operation-session",
        )
        .unwrap();
    engine
        .ensure_vector_collection(
            &lease.session_id,
            &lease.token,
            &EnsureVectorCollection {
                scope: format!("instance:{}", instance()),
                collection_id: CanonicalId::new("documents").unwrap(),
                vectors: vec![NamedVectorDefinition {
                    name: CanonicalId::new("body").unwrap(),
                    field: CanonicalId::new("body-embedding").unwrap(),
                    kind: VectorValueKind::Dense,
                    dimensions: 2,
                    metric: VectorSearchMetric::Dot,
                    embedding_model: None,
                    memory_tier: VectorMemoryTier::Cached,
                }],
            },
            &mutation_context(
                &id("backup-vector-collection"),
                "request-collection",
                "operation-collection",
            ),
            200,
        )
        .unwrap();
    commit_vectors(
        &engine,
        &lease,
        300,
        vec![
            TransactionMutation::PutSchema {
                registry: DataSchemaRegistry {
                    revision: 1,
                    migration: "install backup vector fixture schema".into(),
                    catalogue: DataCatalogueIdentity::default(),
                    tables: BTreeMap::new(),
                    records: BTreeMap::from([(
                        CanonicalId::new("document").unwrap(),
                        DataRecordSchema {
                            properties: BTreeMap::from([(
                                "body".into(),
                                DataPropertySchema {
                                    value_type: DataValueType::String,
                                    required: false,
                                },
                            )]),
                            allow_additional_properties: true,
                            ..DataRecordSchema::default()
                        },
                    )]),
                    relations: BTreeMap::new(),
                    events: BTreeMap::new(),
                },
            },
            document_mutation("alpha", "rrflow backup alpha"),
            document_mutation("beta", "rrflow backup beta"),
            vector_mutation("alpha", vec![1.0, 0.0]),
            vector_mutation("beta", vec![0.0, 1.0]),
        ],
    );
    let index = engine
        .ensure_vector_index(
            &lease.session_id,
            &lease.token,
            &turboquant_request(),
            400,
            "request-backup-turboquant",
            "operation-backup-turboquant",
        )
        .unwrap();
    assert_eq!(index.index.kind.as_str(), "turboquant");

    let backup = engine
        .create_instance_backup(
            &lease.session_id,
            &lease.token,
            &id("backup-vector-key"),
            &CreateInstanceBackup {
                label: "vector-complete".into(),
                created_at_unix_ms: 500,
            },
            500,
            "request-backup-vector",
            "operation-backup-vector",
        )
        .unwrap();
    assert_eq!(
        backup.backup.object_payloads,
        BackupCoverageSnapshot::Included
    );
    assert!(backup.backup.application_complete);

    let restore_id = CanonicalId::new("vector-restore").unwrap();
    let restored = engine
        .restore_instance_backup(
            &lease.session_id,
            &lease.token,
            &id("restore-vector-key"),
            &RestoreInstanceBackup {
                backup_sha256: backup.backup.backup_sha256.clone(),
                restore_id: restore_id.clone(),
                restored_at_unix_ms: 600,
            },
            600,
            "request-restore-vector",
            "operation-restore-vector",
        )
        .unwrap();
    assert!(restored.reopened);
    drop(engine);
    std::fs::remove_dir_all(&source_root).unwrap();

    let restored_root = root
        .path()
        .join("rrd-service")
        .join(instance().as_str())
        .join("restores")
        .join(restore_id.as_str());
    let engine = RrdEngine::open(&restored_root, instance(), TOKEN_KEY).unwrap();
    let restored_lease = engine
        .create_session(
            &session_request(9_000, 2),
            &id("restored-vector-session"),
            700,
            "request-restored-session",
            "operation-restored-session",
        )
        .unwrap();
    let result = engine
        .search_vectors(
            &restored_lease.session_id,
            &restored_lease.token,
            &search_request(VectorSearchMode::RequireApproximate {
                exact_rerank: 2,
                ef_search: 2,
            }),
            800,
            "request-restored-search",
            "operation-restored-search",
        )
        .unwrap();
    assert_eq!(result.access_path.as_str(), "turboquant");
    assert_eq!(result.hits[0].reference.id.as_str(), "alpha");
}
