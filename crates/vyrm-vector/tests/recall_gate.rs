use std::collections::{BTreeSet, HashSet};
use vyrm_core::{
    ProjectionId, ReadStamp, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector, ScopeId,
    VectorValue,
};
use vyrm_vector::{
    search_exact_ref, FilterCondition, FilterExpression, FilterOperator, HnswConfig, HnswIndex,
    ScoreMetric, SearchMode, SearchRequest, VectorCandidate, VectorQuery,
};

#[test]
fn deterministic_ann_recall_gate_covers_unfiltered_and_selective_queries() {
    let scope = ScopeId::new("instance:recall-gate").unwrap();
    let mut random = 0xa11c_e5eed_u64;
    let candidates = (0..512)
        .map(|id| candidate(&scope, id, vector(16, &mut random)))
        .collect::<Vec<_>>();
    let index = HnswIndex::build(
        HnswConfig {
            id: ProjectionId::new("vector:hnsw:recall-gate").unwrap(),
            scope: scope.clone(),
            field: "body".into(),
            dimensions: 16,
            metric: ScoreMetric::Cosine,
            m: 16,
            ef_construction: 100,
            max_level: 10,
            seed: 31,
            filter_properties: BTreeSet::from(["bucket".into()]),
        },
        1,
        512,
        candidates.clone(),
    )
    .unwrap();
    for filter_percent in [100, 10] {
        let mut recall = 0.0;
        for _ in 0..20 {
            let query = vector(16, &mut random);
            let exact_request = request(&scope, query.clone(), filter_percent, SearchMode::Exact);
            let expected = search_exact_ref(&exact_request, &candidates).unwrap();
            let approximate_request = request(
                &scope,
                query,
                filter_percent,
                SearchMode::RequireApproximate { exact_rerank: 64 },
            );
            let actual = index.search(&approximate_request, 64).unwrap();
            assert_eq!(actual.len(), expected.len());
            recall += overlap(&expected, &actual);
        }
        let mean = recall / 20.0;
        assert!(mean >= 0.95, "filter={filter_percent}% recall={mean}");
    }
}

fn request(
    scope: &ScopeId,
    query: Vec<f32>,
    filter_percent: usize,
    mode: SearchMode,
) -> SearchRequest {
    SearchRequest {
        scope: scope.clone(),
        read: ReadStamp::new(scope.clone(), None, 0, 512, Some("33".repeat(32))).unwrap(),
        valid_at: 2,
        field: "body".into(),
        query: VectorQuery::Dense { values: query },
        metric: ScoreMetric::Cosine,
        top_k: 10,
        mode,
        filter: (filter_percent < 100).then(|| FilterExpression::Condition {
            condition: FilterCondition {
                property: "bucket".into(),
                operator: FilterOperator::Range {
                    gt: None,
                    gte: None,
                    lt: Some(RuntimeValue::Unsigned(filter_percent as u64)),
                    lte: None,
                },
            },
        }),
    }
}

fn candidate(scope: &ScopeId, id: usize, values: Vec<f32>) -> VectorCandidate {
    let mut properties = RuntimeProperties::new();
    properties.insert("bucket".into(), RuntimeValue::Unsigned((id % 100) as u64));
    VectorCandidate {
        scope: scope.clone(),
        source_cursor: id as u64 + 1,
        vector: RuntimeVector {
            reference: RuntimeRef::new("embedding", format!("v-{id:04}")).unwrap(),
            subject: RuntimeRef::new("document", format!("d-{id:04}")).unwrap(),
            field: "body".into(),
            valid_from: 1,
            valid_to: None,
            value: VectorValue::Dense { values },
            provenance: None,
            properties,
        },
    }
}

fn overlap(exact: &[vyrm_vector::SearchHit], actual: &[vyrm_vector::SearchHit]) -> f64 {
    let expected = exact
        .iter()
        .map(|hit| hit.reference.clone())
        .collect::<HashSet<_>>();
    actual
        .iter()
        .filter(|hit| expected.contains(&hit.reference))
        .count() as f64
        / exact.len() as f64
}

fn vector(dimensions: usize, random: &mut u64) -> Vec<f32> {
    let mut values = (0..dimensions)
        .map(|_| {
            *random = random
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((*random >> 32) as u32 as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect::<Vec<_>>();
    let norm = values.iter().map(|value| value.powi(2)).sum::<f32>().sqrt();
    for value in &mut values {
        *value /= norm;
    }
    values
}
