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

#[test]
fn engine_transaction_preserves_session_read_and_commit_identity_across_replay_and_reopen() {
    let (root, engine) = isolated_engine();
    let lease = engine
        .create_session(
            &session_request(5_000, 2),
            &id("create-stamped-key"),
            1_000,
            "request-create-stamped",
            "operation-create-stamped",
        )
        .unwrap();
    let transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &mutation_context(
                &id("begin-stamped-key"),
                "request-begin-stamped",
                "operation-begin-stamped",
            ),
            1_100,
        )
        .unwrap();
    let read = {
        let (_, state) = engine
            .load_authenticated(&lease.session_id, &lease.token)
            .unwrap();
        state.transactions[&transaction.transaction_id].read.clone()
    };
    let request = commit_request("stamped-engine-transaction");
    let expected_commit = public_runtime_commit(
        &request,
        &lease.session_id,
        &instance(),
        read.commit_cursor,
        1_200,
    )
    .unwrap();
    let expected_commit_id = expected_commit.digest();
    let receipt = engine
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("commit-stamped-key"),
            &request,
            1_200,
            "request-commit-stamped",
            "operation-commit-stamped",
        )
        .unwrap();
    assert_eq!(
        receipt.runtime_commit_sha256.as_deref(),
        Some(expected_commit_id.as_str())
    );
    assert_eq!(receipt.last_runtime_cursor, Some(read.commit_cursor + 1));
    assert!(!receipt.idempotent_replay);

    let audit = engine
        .storage
        .runtime_audit(&expected_commit_id)
        .unwrap()
        .unwrap();
    audit.validate().unwrap();
    assert_eq!(audit.request_id, expected_commit_id);
    assert_eq!(
        audit.actor,
        format!("session:{}", lease.session_id.as_str())
    );
    assert_eq!(audit.read.as_ref(), Some(&read));
    assert_eq!(audit.outcome_cursor, receipt.last_runtime_cursor);

    let replay = engine
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("commit-stamped-key"),
            &request,
            1_201,
            "request-replay-stamped",
            "operation-replay-stamped",
        )
        .unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(replay.runtime_commit_sha256, receipt.runtime_commit_sha256);
    assert_eq!(
        engine
            .storage
            .runtime_audit(&expected_commit_id)
            .unwrap()
            .as_ref(),
        Some(&audit)
    );

    drop(engine);
    let reopened = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    assert_eq!(
        reopened
            .storage
            .runtime_audit(&expected_commit_id)
            .unwrap()
            .as_ref(),
        Some(&audit)
    );
}
