use std::collections::BTreeSet;
use vyrm_core::{
    ProjectionId, ReadStamp, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector, ScopeId,
    VectorValue,
};
use vyrm_vector::{
    search_exact, FilterCondition, FilterExpression, FilterOperator, HnswConfig, HnswIndex,
    ImmutableVectorSegment, ScoreMetric, SearchMode, SearchRequest, VectorCandidate, VectorQuery,
    VectorSegmentConfig,
};

fn contract() -> serde_json::Value {
    let scope = ScopeId::new("instance:vector-golden").unwrap();
    let candidates = vec![
        candidate(&scope, 1, "a", [1.0, 0.0], "hot"),
        candidate(&scope, 2, "b", [0.0, 1.0], "cold"),
        candidate(&scope, 3, "c", [0.6, 0.4], "hot"),
        candidate(&scope, 4, "a", [0.8, 0.2], "hot"),
    ];
    let request = SearchRequest {
        scope: scope.clone(),
        read: ReadStamp::new(scope.clone(), None, 0, 4, Some("11".repeat(32))).unwrap(),
        valid_at: 10,
        field: "body".into(),
        query: VectorQuery::Dense {
            values: vec![1.0, 0.0],
        },
        metric: ScoreMetric::Dot,
        top_k: 2,
        mode: SearchMode::Exact,
        filter: Some(FilterExpression::Condition {
            condition: FilterCondition {
                property: "temperature".into(),
                operator: FilterOperator::Equals {
                    value: RuntimeValue::String("hot".into()),
                },
            },
        }),
    };
    let exact_hits = search_exact(&request, candidates.clone()).unwrap();
    let segment = ImmutableVectorSegment::build(
        VectorSegmentConfig {
            id: ProjectionId::new("vector:exact:golden").unwrap(),
            scope: scope.clone(),
            field: "body".into(),
            dimensions: 2,
            metric: ScoreMetric::Dot,
            filter_properties: BTreeSet::from(["temperature".into()]),
        },
        1,
        4,
        candidates.clone(),
    )
    .unwrap();
    let hnsw = HnswIndex::build(
        HnswConfig {
            id: ProjectionId::new("vector:hnsw:golden").unwrap(),
            scope,
            field: "body".into(),
            dimensions: 2,
            metric: ScoreMetric::Dot,
            m: 4,
            ef_construction: 8,
            max_level: 4,
            seed: 23,
            filter_properties: BTreeSet::from(["temperature".into()]),
        },
        1,
        4,
        candidates.clone(),
    )
    .unwrap();
    let mut approximate_request = request.clone();
    approximate_request.mode = SearchMode::RequireApproximate { exact_rerank: 4 };
    let approximate_hits = hnsw.search(&approximate_request, 4).unwrap();
    serde_json::json!({
        "comment": "M5 portable vector contract; changes require an explicit format-version decision",
        "request": request,
        "candidates": candidates,
        "exact_hits": exact_hits,
        "approximate_hits": approximate_hits,
        "segment_descriptor": segment.descriptor(),
        "hnsw_descriptor": hnsw.descriptor(),
    })
}

fn candidate(
    scope: &ScopeId,
    cursor: u64,
    id: &str,
    values: [f32; 2],
    temperature: &str,
) -> VectorCandidate {
    let mut properties = RuntimeProperties::new();
    properties.insert(
        "temperature".into(),
        RuntimeValue::String(temperature.into()),
    );
    VectorCandidate {
        scope: scope.clone(),
        source_cursor: cursor,
        vector: RuntimeVector {
            reference: RuntimeRef::new("embedding", id).unwrap(),
            subject: RuntimeRef::new("document", id).unwrap(),
            field: "body".into(),
            valid_from: 1,
            valid_to: None,
            value: VectorValue::Dense {
                values: values.into(),
            },
            provenance: None,
            properties,
        },
    }
}

#[test]
fn vector_search_wire_contract_matches_checked_in_fixture() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/vector-search-v1.json")).unwrap();
    let actual = contract();
    assert_eq!(
        actual,
        expected,
        "{}",
        serde_json::to_string_pretty(&actual).unwrap()
    );
}
