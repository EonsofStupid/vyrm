use super::*;

#[test]
fn catalogue_mutation_advances_the_transaction_read_stamp() {
    let (_root, engine) = isolated_engine();
    let lease = engine
        .create_session(
            &session_request(5_000, 2),
            &id("create-key"),
            1_000,
            "request-create",
            "operation-create",
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
            &mutation_context(&id("begin-key"), "request-begin", "operation-begin"),
            1_100,
        )
        .unwrap();
    let (_, state) = engine
        .load_authenticated(&lease.session_id, &lease.token)
        .unwrap();
    let before = state.transactions[&transaction.transaction_id].read.clone();

    engine
        .ensure_vector_collection(
            &lease.session_id,
            &lease.token,
            &EnsureVectorCollection {
                scope: format!("instance:{}", instance()),
                collection_id: CanonicalId::new("documents").unwrap(),
                vectors: vec![NamedVectorDefinition {
                    name: CanonicalId::new("title").unwrap(),
                    field: CanonicalId::new("title-embedding").unwrap(),
                    kind: VectorValueKind::Dense,
                    dimensions: 2,
                    metric: VectorSearchMetric::Cosine,
                    embedding_model: None,
                    memory_tier: VectorMemoryTier::Cached,
                }],
            },
            &mutation_context(
                &id("ensure-vector-key"),
                "request-vector",
                "operation-vector",
            ),
            1_200,
        )
        .unwrap();

    let after = engine.storage.runtime_read_stamp(&before.scope).unwrap();
    assert_eq!(after.commit_cursor, before.commit_cursor);
    assert_eq!(after.catalog_revision, before.catalog_revision + 1);
    assert_ne!(after.manifest_id, before.manifest_id);

    let committed = engine.commit_transaction(
        &lease.session_id,
        &lease.token,
        &transaction.transaction_id,
        &id("commit-key"),
        &commit_request("stale-catalogue"),
        1_300,
        "request-commit",
        "operation-commit",
    );
    assert!(matches!(committed, Err(ServiceError::StorageConflict(_))));
}
