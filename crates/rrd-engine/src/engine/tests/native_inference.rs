use super::*;
use rrd_contract::{
    transaction_operation_sha256, BeginTransaction, CommitTransaction, DataCatalogueIdentity,
    DataRecordSchema, DataReference, DataSchemaRegistry, EmbedAndSearchVectors, EmbeddingInput,
    EmbeddingNetworkPolicy, GenerateEmbeddings, ListEmbeddingModels, VectorEmbeddingModel,
    VectorSearchMode,
};
use std::collections::BTreeMap;

fn data_reference(kind: &str, value: &str) -> DataReference {
    DataReference {
        kind: CanonicalId::new(kind).unwrap(),
        id: CanonicalId::new(value).unwrap(),
    }
}

#[test]
fn native_inference_batches_with_provenance_and_searches_at_one_read_stamp() {
    let (_root, engine) = isolated_engine();
    assert_eq!(engine.install_feature_hash_embedding(64, 71).unwrap(), 1);
    let lease = engine
        .create_session(
            &session_request(9_000, 4),
            &id("embedding-session"),
            10,
            "embedding-session-request",
            "embedding-session-operation",
        )
        .unwrap();
    let scope = format!("instance:{}", instance());
    let catalogue = engine
        .list_embedding_models(
            &lease.session_id,
            &lease.token,
            &ListEmbeddingModels {
                scope: scope.clone(),
            },
            20,
            "embedding-list-request",
            "embedding-list-operation",
        )
        .unwrap();
    assert_eq!(catalogue.registry_revision, 1);
    assert_eq!(catalogue.backends.len(), 1);
    assert!(matches!(
        catalogue.backends[0].trust,
        rrd_contract::EmbeddingTrustBoundary::LocalOffline
    ));
    let backend = catalogue.backends[0].clone();

    engine
        .ensure_vector_collection(
            &lease.session_id,
            &lease.token,
            &EnsureVectorCollection {
                scope: scope.clone(),
                collection_id: CanonicalId::new("documents").unwrap(),
                vectors: vec![NamedVectorDefinition {
                    name: CanonicalId::new("body").unwrap(),
                    field: CanonicalId::new("body-embedding").unwrap(),
                    kind: VectorValueKind::Dense,
                    dimensions: 64,
                    metric: VectorSearchMetric::Dot,
                    embedding_model: Some(VectorEmbeddingModel {
                        name: backend.model_name(),
                        digest: backend.model_sha256.clone(),
                    }),
                    memory_tier: VectorMemoryTier::Cached,
                }],
            },
            &mutation_context(
                &id("embedding-collection-key"),
                "embedding-collection-request",
                "embedding-collection-operation",
            ),
            30,
        )
        .unwrap();

    let generated = engine
        .generate_embeddings(
            &lease.session_id,
            &lease.token,
            &GenerateEmbeddings {
                scope: scope.clone(),
                backend_id: backend.id.clone(),
                network_policy: EmbeddingNetworkPolicy::Deny,
                inputs: vec![
                    EmbeddingInput {
                        id: CanonicalId::new("document-a").unwrap(),
                        media_type: "text/plain".into(),
                        bytes: b"alpha blue orchard".to_vec(),
                    },
                    EmbeddingInput {
                        id: CanonicalId::new("document-b").unwrap(),
                        media_type: "text/plain".into(),
                        bytes: b"zebra quantum river".to_vec(),
                    },
                ],
            },
            40,
            "embedding-generate-request",
            "embedding-generate-operation",
        )
        .unwrap();
    assert_eq!(generated.embeddings.len(), 2);
    assert_eq!(generated.registry_revision, 1);
    assert!(generated
        .embeddings
        .iter()
        .all(|embedding| embedding.provenance.model_sha256 == backend.model_sha256));

    let mut mutations = vec![TransactionMutation::PutSchema {
        registry: DataSchemaRegistry {
            revision: 1,
            migration: "install native inference fixture".into(),
            catalogue: DataCatalogueIdentity::default(),
            tables: BTreeMap::new(),
            records: BTreeMap::from([(
                CanonicalId::new("document").unwrap(),
                DataRecordSchema {
                    allow_additional_properties: true,
                    ..DataRecordSchema::default()
                },
            )]),
            relations: BTreeMap::new(),
            events: BTreeMap::new(),
        },
    }];
    for embedding in generated.embeddings {
        let document_id = embedding.input_id.as_str().to_owned();
        mutations.push(TransactionMutation::PutRecord {
            reference: data_reference("document", &document_id),
            valid_from: 1,
            valid_to: None,
            properties: Default::default(),
        });
        mutations.push(TransactionMutation::PutVector {
            reference: data_reference("embedding", &document_id),
            subject: data_reference("document", &document_id),
            collection_id: Some(CanonicalId::new("documents").unwrap()),
            vector_name: Some(CanonicalId::new("body").unwrap()),
            field: CanonicalId::new("body-embedding").unwrap(),
            valid_from: 1,
            valid_to: None,
            value: embedding.value,
            provenance: Some(embedding.provenance),
            properties: Default::default(),
        });
    }
    let transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &BeginTransaction {
                scope: CanonicalId::new("data").unwrap(),
                timeout_ms: 1_000,
            },
            &mutation_context(
                &id("embedding-begin-key"),
                "embedding-begin-request",
                "embedding-begin-operation",
            ),
            50,
        )
        .unwrap();
    engine
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("embedding-commit-key"),
            &CommitTransaction {
                operation_sha256: transaction_operation_sha256(&mutations),
                mutations,
            },
            51,
            "embedding-commit-request",
            "embedding-commit-operation",
        )
        .unwrap();

    let before = engine.readiness(60).unwrap().runtime_cursor;
    let result = engine
        .embed_and_search_vectors(
            &lease.session_id,
            &lease.token,
            &EmbedAndSearchVectors {
                scope,
                backend_id: backend.id,
                network_policy: EmbeddingNetworkPolicy::Deny,
                input: EmbeddingInput {
                    id: CanonicalId::new("query-a").unwrap(),
                    media_type: "text/plain".into(),
                    bytes: b"alpha blue orchard".to_vec(),
                },
                valid_at: 1,
                collection_id: CanonicalId::new("documents").unwrap(),
                vector_name: CanonicalId::new("body").unwrap(),
                filter: None,
                top_k: 1,
                mode: VectorSearchMode::Exact,
                max_scanned_changes: 10_000,
            },
            61,
            "embedding-search-request",
            "embedding-search-operation",
        )
        .unwrap();
    assert_eq!(result.search.known_at_cursor, before);
    assert_eq!(result.search.hits.len(), 1);
    assert_eq!(result.search.hits[0].subject.id.as_str(), "document-a");
    assert_eq!(engine.readiness(62).unwrap().runtime_cursor, before);
}
