use rrd_core::{digest, ProjectionId, ScopeId};
use rrd_store::{Engine, NativeEngine};
use rrd_vector::{
    CollectionError, CollectionMutationContext, NamedVectorConfig, PayloadIndexDefinition,
    PayloadIndexKind, ScoreMetric, VectorCollectionDefinition, VectorCollectionRepository,
    VectorMemoryTier, VectorValueKind,
};
use std::collections::BTreeMap;

fn context(at: u64, operation: &str) -> CollectionMutationContext {
    CollectionMutationContext {
        at,
        actor: "collection-test".into(),
        request_id: format!("request-{operation}"),
        operation_id: operation.into(),
    }
}

#[test]
fn payload_index_and_collection_deletion_lifecycles_reopen_and_replay() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("native");
    let scope = ScopeId::new("instance:collection-lifecycle").unwrap();
    let collection_id = ProjectionId::new("documents").unwrap();
    let field = ProjectionId::new("tenant").unwrap();
    let ensure_collection_digest = digest::sha256_hex(b"ensure-collection");
    let ensure_index_digest = digest::sha256_hex(b"ensure-index");
    let delete_index_digest = digest::sha256_hex(b"delete-index");
    let delete_collection_digest = digest::sha256_hex(b"delete-collection");

    {
        let engine = NativeEngine::open(&root).unwrap();
        let repository = VectorCollectionRepository::new(&engine, scope.clone());
        repository
            .ensure(
                &context(100, "ensure-collection"),
                "ensure-collection".into(),
                ensure_collection_digest,
                definition(2),
            )
            .unwrap();
        let (catalogue, index, replay) = repository
            .ensure_payload_index(
                &context(200, "ensure-index"),
                "ensure-index".into(),
                ensure_index_digest.clone(),
                &collection_id,
                PayloadIndexDefinition {
                    field: field.clone(),
                    kind: PayloadIndexKind::Keyword,
                },
            )
            .unwrap();
        assert!(!replay);
        assert_eq!(catalogue.revision, 2);
        assert_eq!(index.generation, 1);
        assert_eq!(catalogue.collections[&collection_id].generation, 2);
    }

    let engine = NativeEngine::open(&root).unwrap();
    let repository = VectorCollectionRepository::new(&engine, scope);
    let (catalogue, index, replay) = repository
        .ensure_payload_index(
            &context(300, "replay-index"),
            "ensure-index".into(),
            ensure_index_digest,
            &collection_id,
            PayloadIndexDefinition {
                field: field.clone(),
                kind: PayloadIndexKind::Keyword,
            },
        )
        .unwrap();
    assert!(replay);
    assert_eq!(catalogue.revision, 2);
    assert_eq!(index.generation, 1);

    let conflict = repository.delete_payload_index(
        &context(350, "cross-kind-conflict"),
        "ensure-index".into(),
        delete_index_digest.clone(),
        &collection_id,
        &field,
    );
    assert!(matches!(
        conflict,
        Err(CollectionError::IdempotencyConflict)
    ));

    let (catalogue, deleted, replay) = repository
        .delete_payload_index(
            &context(400, "delete-index"),
            "delete-index".into(),
            delete_index_digest.clone(),
            &collection_id,
            &field,
        )
        .unwrap();
    assert!(!replay);
    assert_eq!(deleted.definition.field, field);
    assert!(catalogue.collections[&collection_id]
        .payload_indexes
        .is_empty());
    let (_, _, replay) = repository
        .delete_payload_index(
            &context(450, "replay-delete-index"),
            "delete-index".into(),
            delete_index_digest,
            &collection_id,
            &field,
        )
        .unwrap();
    assert!(replay);

    let (catalogue, deleted, replay) = repository
        .delete(
            &context(500, "delete-collection"),
            "delete-collection".into(),
            delete_collection_digest.clone(),
            &collection_id,
        )
        .unwrap();
    assert!(!replay);
    assert_eq!(deleted.definition.id, collection_id);
    assert!(catalogue.collections.is_empty());
    drop(engine);

    let reopened = NativeEngine::open(&root).unwrap();
    let repository = VectorCollectionRepository::new(
        &reopened,
        ScopeId::new("instance:collection-lifecycle").unwrap(),
    );
    let (catalogue, _, replay) = repository
        .delete(
            &context(600, "replay-delete-collection"),
            "delete-collection".into(),
            delete_collection_digest,
            &collection_id,
        )
        .unwrap();
    assert!(replay);
    assert!(catalogue.collections.is_empty());
    assert_eq!(reopened.control_journal_since(0, 10).unwrap().len(), 4);
}

fn definition(dimensions: u32) -> VectorCollectionDefinition {
    let name = ProjectionId::new("title").unwrap();
    VectorCollectionDefinition {
        id: ProjectionId::new("documents").unwrap(),
        vectors: BTreeMap::from([(
            name.clone(),
            NamedVectorConfig {
                name,
                field: "title_embedding".into(),
                kind: VectorValueKind::Dense,
                dimensions,
                metric: ScoreMetric::Cosine,
                embedding_model: None,
                memory_tier: VectorMemoryTier::Cached,
            },
        )]),
    }
}

#[test]
fn native_collection_catalogue_reopens_and_replays_exactly() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("native");
    let scope = ScopeId::new("instance:collections").unwrap();
    let request_digest = digest::sha256_hex(b"ensure-documents-v1");
    {
        let engine = NativeEngine::open(&root).unwrap();
        let repository = VectorCollectionRepository::new(&engine, scope.clone());
        let (catalogue, replay) = repository
            .ensure(
                &context(100, "ensure"),
                "ensure-documents".into(),
                request_digest.clone(),
                definition(2),
            )
            .unwrap();
        assert!(!replay);
        assert_eq!(catalogue.revision, 1);
        assert_eq!(
            catalogue.collections[&ProjectionId::new("documents").unwrap()].generation,
            1
        );
        assert_eq!(engine.control_journal_since(0, 10).unwrap().len(), 1);
    }

    let reopened = NativeEngine::open(&root).unwrap();
    let repository = VectorCollectionRepository::new(&reopened, scope);
    let (catalogue, replay) = repository
        .ensure(
            &context(200, "retry"),
            "ensure-documents".into(),
            request_digest,
            definition(2),
        )
        .unwrap();
    assert!(replay);
    assert_eq!(catalogue.revision, 1);
    assert_eq!(reopened.control_journal_since(0, 10).unwrap().len(), 1);

    let collision = repository.ensure(
        &context(300, "collision"),
        "ensure-documents".into(),
        digest::sha256_hex(b"different-request"),
        definition(3),
    );
    assert!(matches!(
        collision,
        Err(CollectionError::IdempotencyConflict)
    ));

    let (catalogue, replay) = repository
        .ensure(
            &context(400, "reconfigure"),
            "reconfigure-documents".into(),
            digest::sha256_hex(b"ensure-documents-v2"),
            definition(3),
        )
        .unwrap();
    assert!(!replay);
    let entry = &catalogue.collections[&ProjectionId::new("documents").unwrap()];
    assert_eq!(entry.generation, 2);
    assert_eq!(entry.created_at, 100);
    assert_eq!(entry.updated_at, 400);
    assert_eq!(reopened.control_journal_since(0, 10).unwrap().len(), 2);
}
