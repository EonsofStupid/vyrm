use rrd_store::{Engine, NativeEngine};
use std::collections::BTreeMap;
use vyrm_core::{digest, ProjectionId, ScopeId};
use vyrm_vector::{
    CollectionError, CollectionMutationContext, NamedVectorConfig, ScoreMetric,
    VectorCollectionDefinition, VectorCollectionRepository, VectorMemoryTier, VectorValueKind,
};

fn context(at: u64, operation: &str) -> CollectionMutationContext {
    CollectionMutationContext {
        at,
        actor: "collection-test".into(),
        request_id: format!("request-{operation}"),
        operation_id: operation.into(),
    }
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
