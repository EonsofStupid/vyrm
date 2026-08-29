use super::*;
use rrd_contract::{
    BackupCoverageSnapshot, CreateInstanceBackup, DataLogicalModel, DataProperties,
    DataPropertySchema, DataRecordSchema, DataReference, DataSchemaMode, DataSchemaRegistry,
    DataTableSchema, DataTarget, DataValueType, DataVectorValue, DeleteVectorCollection,
    DeleteVectorPayloadIndex, EnsureQueryIndex, EnsureVectorIndex, EnsureVectorPayloadIndex,
    HybridFusion, ListVectorCollections, ListVectorPayloadIndexes, QueryBudget, QueryIndexKind,
    QueryValue, RestoreInstanceBackup, RetrieveVectorPoints, SearchHybrid, SearchVectors,
    VectorIndexConfiguration, VectorPayloadIndexKind, VectorQuantizationBits, VectorSearchMode,
    VectorSearchQuery,
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
                full_text: None,
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

#[test]
fn collection_point_and_payload_index_administration_is_atomic_and_restart_safe() {
    let root = tempfile::tempdir().unwrap();
    let engine = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    let lease = engine
        .create_session(
            &session_request(9_000, 8),
            &id("vector-admin-session"),
            100,
            "request-session",
            "operation-session",
        )
        .unwrap();
    let collection_request = EnsureVectorCollection {
        scope: format!("instance:{}", instance()),
        collection_id: CanonicalId::new("multimodal").unwrap(),
        vectors: vec![
            NamedVectorDefinition {
                name: CanonicalId::new("dense").unwrap(),
                field: CanonicalId::new("dense-embedding").unwrap(),
                kind: VectorValueKind::Dense,
                dimensions: 3,
                metric: VectorSearchMetric::Cosine,
                embedding_model: None,
                memory_tier: VectorMemoryTier::Cached,
            },
            NamedVectorDefinition {
                name: CanonicalId::new("sparse").unwrap(),
                field: CanonicalId::new("sparse-embedding").unwrap(),
                kind: VectorValueKind::Sparse,
                dimensions: 8,
                metric: VectorSearchMetric::Dot,
                embedding_model: None,
                memory_tier: VectorMemoryTier::Cold,
            },
            NamedVectorDefinition {
                name: CanonicalId::new("late").unwrap(),
                field: CanonicalId::new("late-embedding").unwrap(),
                kind: VectorValueKind::MultiDense,
                dimensions: 2,
                metric: VectorSearchMetric::Dot,
                embedding_model: None,
                memory_tier: VectorMemoryTier::Pinned,
            },
        ],
    };
    engine
        .ensure_vector_collection(
            &lease.session_id,
            &lease.token,
            &collection_request,
            &mutation_context(
                &id("ensure-multimodal"),
                "request-collection",
                "operation-collection",
            ),
            200,
        )
        .unwrap();
    for (ordinal, (field, kind)) in [
        ("tenant", VectorPayloadIndexKind::Keyword),
        ("priority", VectorPayloadIndexKind::Unsigned),
    ]
    .into_iter()
    .enumerate()
    {
        engine
            .ensure_vector_payload_index(
                &lease.session_id,
                &lease.token,
                &EnsureVectorPayloadIndex {
                    scope: collection_request.scope.clone(),
                    collection_id: collection_request.collection_id.clone(),
                    field: CanonicalId::new(field).unwrap(),
                    kind,
                },
                &mutation_context(
                    &id(&format!("payload-index-{field}")),
                    &format!("request-payload-index-{ordinal}"),
                    &format!("operation-payload-index-{ordinal}"),
                ),
                210 + ordinal as u64,
            )
            .unwrap();
    }
    let indexes = engine
        .list_vector_payload_indexes(
            &lease.session_id,
            &lease.token,
            &ListVectorPayloadIndexes {
                scope: collection_request.scope.clone(),
                collection_id: collection_request.collection_id.clone(),
            },
            220,
            "request-list-payload-indexes",
            "operation-list-payload-indexes",
        )
        .unwrap();
    assert_eq!(indexes.indexes.len(), 2);

    let payload = DataProperties::from([
        ("tenant".into(), QueryValue::String("alpha".into())),
        ("priority".into(), QueryValue::Unsigned(7)),
    ]);
    let subject = reference("document", "alpha");
    let dense_reference = reference("embedding", "alpha-dense");
    let sparse_reference = reference("embedding", "alpha-sparse");
    let late_reference = reference("embedding", "alpha-late");
    let point = |reference: DataReference,
                 vector_name: &str,
                 field: &str,
                 value: DataVectorValue| TransactionMutation::PutVector {
        reference,
        subject: subject.clone(),
        collection_id: Some(collection_request.collection_id.clone()),
        vector_name: Some(CanonicalId::new(vector_name).unwrap()),
        field: CanonicalId::new(field).unwrap(),
        valid_from: 1,
        valid_to: None,
        value,
        provenance: None,
        properties: payload.clone(),
    };
    commit_vectors(
        &engine,
        &lease,
        300,
        vec![
            TransactionMutation::PutSchema {
                registry: DataSchemaRegistry {
                    revision: 1,
                    migration: "install multimodal point schema".into(),
                    catalogue: DataCatalogueIdentity::default(),
                    tables: BTreeMap::from([
                        (
                            CanonicalId::new("embedding").unwrap(),
                            DataTableSchema {
                                model: DataLogicalModel::Vector,
                                mode: DataSchemaMode::Schemaless,
                                properties: BTreeMap::new(),
                                allow_additional_properties: false,
                            },
                        ),
                        (
                            CanonicalId::new("document").unwrap(),
                            DataTableSchema {
                                model: DataLogicalModel::Document,
                                mode: DataSchemaMode::Schemaless,
                                properties: BTreeMap::new(),
                                allow_additional_properties: false,
                            },
                        ),
                    ]),
                    records: BTreeMap::new(),
                    relations: BTreeMap::new(),
                    events: BTreeMap::new(),
                },
            },
            TransactionMutation::PutRecord {
                reference: subject.clone(),
                valid_from: 1,
                valid_to: None,
                properties: DataProperties::new(),
            },
            point(
                dense_reference.clone(),
                "dense",
                "dense-embedding",
                DataVectorValue::Dense {
                    values: vec![1.0, 0.0, 0.0],
                },
            ),
            point(
                sparse_reference.clone(),
                "sparse",
                "sparse-embedding",
                DataVectorValue::Sparse {
                    dimensions: 8,
                    indices: vec![1, 6],
                    values: vec![0.5, 0.75],
                },
            ),
            point(
                late_reference.clone(),
                "late",
                "late-embedding",
                DataVectorValue::MultiDense {
                    dimensions: 2,
                    vectors: vec![vec![1.0, 0.0], vec![0.0, 1.0]],
                },
            ),
        ],
    );

    let invalid_transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &BeginTransaction {
                scope: CanonicalId::new("data").unwrap(),
                timeout_ms: 1_000,
            },
            &mutation_context(
                &id("begin-invalid-payload"),
                "request-begin-invalid-payload",
                "operation-begin-invalid-payload",
            ),
            350,
        )
        .unwrap();
    let invalid_mutations = vec![TransactionMutation::PutVector {
        reference: reference("embedding", "invalid-payload"),
        subject: subject.clone(),
        collection_id: Some(collection_request.collection_id.clone()),
        vector_name: Some(CanonicalId::new("dense").unwrap()),
        field: CanonicalId::new("dense-embedding").unwrap(),
        valid_from: 1,
        valid_to: None,
        value: DataVectorValue::Dense {
            values: vec![1.0, 0.0, 0.0],
        },
        provenance: None,
        properties: DataProperties::from([
            ("tenant".into(), QueryValue::String("alpha".into())),
            ("priority".into(), QueryValue::String("not-unsigned".into())),
        ]),
    }];
    let invalid_request = CommitTransaction {
        operation_sha256: rrd_contract::transaction_operation_sha256(&invalid_mutations),
        mutations: invalid_mutations,
    };
    let invalid = engine.commit_transaction(
        &lease.session_id,
        &lease.token,
        &invalid_transaction.transaction_id,
        &id("commit-invalid-payload"),
        &invalid_request,
        351,
        "request-commit-invalid-payload",
        "operation-commit-invalid-payload",
    );
    assert!(matches!(invalid, Err(ServiceError::Vector(_))));

    let live_delete = engine.delete_vector_collection(
        &lease.session_id,
        &lease.token,
        &DeleteVectorCollection {
            scope: collection_request.scope.clone(),
            collection_id: collection_request.collection_id.clone(),
            valid_at: 1,
            max_scanned_changes: 10_000,
        },
        &mutation_context(
            &id("delete-live-collection"),
            "request-delete-live",
            "operation-delete-live",
        ),
        400,
    );
    assert!(matches!(live_delete, Err(ServiceError::Vector(_))));

    drop(engine);
    let engine = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    let reopened_indexes = engine
        .list_vector_payload_indexes(
            &lease.session_id,
            &lease.token,
            &ListVectorPayloadIndexes {
                scope: collection_request.scope.clone(),
                collection_id: collection_request.collection_id.clone(),
            },
            500,
            "request-list-reopened-indexes",
            "operation-list-reopened-indexes",
        )
        .unwrap();
    assert_eq!(reopened_indexes.indexes, indexes.indexes);
    for (vector_name, reference) in [
        ("dense", dense_reference.clone()),
        ("sparse", sparse_reference.clone()),
        ("late", late_reference.clone()),
    ] {
        let retrieved = engine
            .retrieve_vector_points(
                &lease.session_id,
                &lease.token,
                &RetrieveVectorPoints {
                    scope: collection_request.scope.clone(),
                    collection_id: collection_request.collection_id.clone(),
                    vector_name: CanonicalId::new(vector_name).unwrap(),
                    valid_at: 1,
                    references: vec![reference],
                    max_scanned_changes: 10_000,
                },
                510,
                "request-retrieve-reopened",
                "operation-retrieve-reopened",
            )
            .unwrap();
        assert_eq!(retrieved.points.len(), 1);
        assert_eq!(retrieved.points[0].payload, payload);
    }

    commit_vectors(
        &engine,
        &lease,
        600,
        [dense_reference, sparse_reference, late_reference]
            .into_iter()
            .map(|reference| TransactionMutation::RetireData {
                model: DataLogicalModel::Vector,
                target: DataTarget::Reference { reference },
                effective_at: 2,
            })
            .collect(),
    );
    let deleted_index = engine
        .delete_vector_payload_index(
            &lease.session_id,
            &lease.token,
            &DeleteVectorPayloadIndex {
                scope: collection_request.scope.clone(),
                collection_id: collection_request.collection_id.clone(),
                field: CanonicalId::new("tenant").unwrap(),
            },
            &mutation_context(
                &id("delete-tenant-index"),
                "request-delete-tenant-index",
                "operation-delete-tenant-index",
            ),
            700,
        )
        .unwrap();
    assert_eq!(deleted_index.deleted_index.field.as_str(), "tenant");
    assert_eq!(deleted_index.collection.payload_indexes.len(), 1);

    let delete_request = DeleteVectorCollection {
        scope: collection_request.scope.clone(),
        collection_id: collection_request.collection_id.clone(),
        valid_at: 2,
        max_scanned_changes: 10_000,
    };
    let delete_context = mutation_context(
        &id("delete-empty-collection"),
        "request-delete-empty",
        "operation-delete-empty",
    );
    let deleted = engine
        .delete_vector_collection(
            &lease.session_id,
            &lease.token,
            &delete_request,
            &delete_context,
            800,
        )
        .unwrap();
    assert!(!deleted.idempotent_replay);
    drop(engine);

    let engine = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    let replay = engine
        .delete_vector_collection(
            &lease.session_id,
            &lease.token,
            &delete_request,
            &delete_context,
            900,
        )
        .unwrap();
    assert!(replay.idempotent_replay);
    let collections = engine
        .list_vector_collections(
            &lease.session_id,
            &lease.token,
            &ListVectorCollections {
                scope: collection_request.scope,
            },
            901,
            "request-list-deleted",
            "operation-list-deleted",
        )
        .unwrap();
    assert!(collections.collections.is_empty());
}

#[test]
fn vector_administration_uses_distinct_deny_by_default_actions() {
    let (_root, engine) = isolated_engine();
    let admin_id = CanonicalId::new("vector-admin").unwrap();
    let limited_id = CanonicalId::new("vector-limited").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance().as_str()).unwrap()],
    };
    let principal =
        |principal_id: CanonicalId, credential: &[u8], actions: &[SecurityAction]| Principal {
            id: principal_id,
            kind: PrincipalKind::Service,
            credential_sha256: digest::sha256_hex(credential),
            credential_revision: 1,
            not_before_unix_ms: 1,
            expires_at_unix_ms: u64::MAX,
            disabled: false,
            role_ids: Default::default(),
            grants: actions
                .iter()
                .copied()
                .map(|action| ResourceGrant {
                    action,
                    resource_prefix: resource.clone(),
                    data_policy: None,
                })
                .collect(),
        };
    SecurityRepository::new(&engine.storage, instance())
        .initialize(
            SecurityState {
                format_version: rrd_security::SECURITY_FORMAT,
                revision: 1,
                principals: BTreeMap::from([
                    (
                        admin_id.clone(),
                        principal(
                            admin_id.clone(),
                            b"vector-admin-key",
                            &[
                                SecurityAction::SessionCreate,
                                SecurityAction::VectorCollectionEnsure,
                                SecurityAction::VectorCollectionDelete,
                                SecurityAction::VectorPayloadIndexEnsure,
                                SecurityAction::VectorPayloadIndexList,
                                SecurityAction::VectorPayloadIndexDelete,
                            ],
                        ),
                    ),
                    (
                        limited_id.clone(),
                        principal(
                            limited_id.clone(),
                            b"vector-limited-key",
                            &[SecurityAction::SessionCreate],
                        ),
                    ),
                ]),
                roles: Default::default(),
                identity_bindings: Default::default(),
                jwt_issuers: Default::default(),
            },
            1,
            "vector-security-test",
            "request-vector-security",
            "operation-vector-security",
        )
        .unwrap();
    let admin = engine
        .create_authenticated_session(
            &admin_id,
            b"vector-admin-key",
            &session_request(5_000, 2),
            &id("vector-admin-session"),
            100,
            "request-admin-session",
            "operation-admin-session",
        )
        .unwrap();
    let limited = engine
        .create_authenticated_session(
            &limited_id,
            b"vector-limited-key",
            &session_request(5_000, 2),
            &id("vector-limited-session"),
            100,
            "request-limited-session",
            "operation-limited-session",
        )
        .unwrap();
    let scope = format!("instance:{}", instance());
    let collection_id = CanonicalId::new("secured").unwrap();
    engine
        .ensure_vector_collection(
            &admin.session_id,
            &admin.token,
            &EnsureVectorCollection {
                scope: scope.clone(),
                collection_id: collection_id.clone(),
                vectors: vec![NamedVectorDefinition {
                    name: CanonicalId::new("dense").unwrap(),
                    field: CanonicalId::new("embedding").unwrap(),
                    kind: VectorValueKind::Dense,
                    dimensions: 2,
                    metric: VectorSearchMetric::Dot,
                    embedding_model: None,
                    memory_tier: VectorMemoryTier::Cached,
                }],
            },
            &mutation_context(
                &id("ensure-secured"),
                "request-ensure-secured",
                "operation-ensure-secured",
            ),
            200,
        )
        .unwrap();
    let ensure_index = EnsureVectorPayloadIndex {
        scope: scope.clone(),
        collection_id: collection_id.clone(),
        field: CanonicalId::new("tenant").unwrap(),
        kind: VectorPayloadIndexKind::Keyword,
    };
    assert!(matches!(
        engine.ensure_vector_payload_index(
            &limited.session_id,
            &limited.token,
            &ensure_index,
            &mutation_context(
                &id("limited-ensure-index"),
                "request-limited-ensure-index",
                "operation-limited-ensure-index",
            ),
            210,
        ),
        Err(ServiceError::PermissionDenied)
    ));
    assert!(matches!(
        engine.list_vector_payload_indexes(
            &limited.session_id,
            &limited.token,
            &ListVectorPayloadIndexes {
                scope: scope.clone(),
                collection_id: collection_id.clone(),
            },
            211,
            "request-limited-list-index",
            "operation-limited-list-index",
        ),
        Err(ServiceError::PermissionDenied)
    ));
    assert!(matches!(
        engine.delete_vector_payload_index(
            &limited.session_id,
            &limited.token,
            &DeleteVectorPayloadIndex {
                scope: scope.clone(),
                collection_id: collection_id.clone(),
                field: CanonicalId::new("tenant").unwrap(),
            },
            &mutation_context(
                &id("limited-delete-index"),
                "request-limited-delete-index",
                "operation-limited-delete-index",
            ),
            212,
        ),
        Err(ServiceError::PermissionDenied)
    ));
    assert!(matches!(
        engine.delete_vector_collection(
            &limited.session_id,
            &limited.token,
            &DeleteVectorCollection {
                scope,
                collection_id,
                valid_at: 1,
                max_scanned_changes: 10,
            },
            &mutation_context(
                &id("limited-delete-collection"),
                "request-limited-delete-collection",
                "operation-limited-delete-collection",
            ),
            213,
        ),
        Err(ServiceError::PermissionDenied)
    ));
}
