use std::collections::BTreeSet;
use vyrm_core::{
    EmbeddingProvenance, ProjectionId, ReadStamp, RuntimeProperties, RuntimeRef, RuntimeVector,
    ScopeId, VectorNormalization, VectorValue,
};
use vyrm_vector::{
    search_exact, CompactDenseSegment, EmbeddingModelBinding, ScoreMetric, SearchMode,
    SearchRequest, VectorCandidate, VectorQuery, VectorSegmentConfig,
};

fn candidate(scope: &ScopeId, id: &str, model: &EmbeddingModelBinding) -> VectorCandidate {
    VectorCandidate {
        scope: scope.clone(),
        source_cursor: if id == "good" { 1 } else { 2 },
        vector: RuntimeVector {
            reference: RuntimeRef::new("embedding", id).unwrap(),
            subject: RuntimeRef::new("document", id).unwrap(),
            field: "body".into(),
            valid_from: 1,
            valid_to: None,
            value: VectorValue::Dense {
                values: vec![1.0, 0.0],
            },
            provenance: Some(EmbeddingProvenance {
                source_digest: "33".repeat(32),
                model: model.name.clone(),
                model_digest: model.digest.clone(),
                dimensions: 2,
                normalization: VectorNormalization::UnitL2,
                generation_parameters: RuntimeProperties::new(),
            }),
            properties: RuntimeProperties::new(),
        },
    }
}

#[test]
fn exact_scans_filter_and_artifacts_reject_incompatible_embedding_spaces() {
    let scope = ScopeId::new("instance:model-binding").unwrap();
    let good = EmbeddingModelBinding {
        name: "provider/model@v1".into(),
        digest: "11".repeat(32),
    };
    let other = EmbeddingModelBinding {
        name: "provider/other@v1".into(),
        digest: "22".repeat(32),
    };
    let candidates = vec![
        candidate(&scope, "good", &good),
        candidate(&scope, "other", &other),
    ];
    let request = SearchRequest {
        scope: scope.clone(),
        read: ReadStamp::new(scope.clone(), None, 0, 2, Some("44".repeat(32))).unwrap(),
        valid_at: 2,
        field: "body".into(),
        query: VectorQuery::Dense {
            values: vec![1.0, 0.0],
        },
        metric: ScoreMetric::Cosine,
        embedding_model: Some(good.clone()),
        top_k: 2,
        mode: SearchMode::Exact,
        filter: None,
    };
    let hits = search_exact(&request, candidates.clone()).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].reference.id.as_str(), "good");

    assert!(CompactDenseSegment::build(
        VectorSegmentConfig {
            id: ProjectionId::new("vector:model-bound").unwrap(),
            scope,
            field: "body".into(),
            dimensions: 2,
            metric: ScoreMetric::Cosine,
            embedding_model: Some(good),
            filter_properties: BTreeSet::new(),
        },
        1,
        2,
        candidates,
    )
    .is_err());
}
