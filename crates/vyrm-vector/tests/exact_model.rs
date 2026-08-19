use std::cmp::Ordering;
use vyrm_core::{
    ReadStamp, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector, ScopeId, VectorValue,
};
use vyrm_vector::{
    search_exact, FilterCondition, FilterExpression, FilterOperator, ScoreMetric, SearchMode,
    SearchRequest, VectorCandidate, VectorQuery,
};

fn next(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed
}

#[test]
fn deterministic_dense_trace_matches_independent_scalar_oracle() {
    let scope = ScopeId::new("instance:model-vector").unwrap();
    let mut seed = 0x5eed_u64;
    let mut candidates = Vec::new();
    for index in 0..256 {
        let mut values = Vec::new();
        for _ in 0..16 {
            let raw = (next(&mut seed) >> 32) as u32;
            values.push((raw as f32 / u32::MAX as f32) * 2.0 - 1.0);
        }
        let mut properties = RuntimeProperties::new();
        properties.insert("bucket".into(), RuntimeValue::Unsigned(index % 7));
        candidates.push(VectorCandidate {
            scope: scope.clone(),
            source_cursor: index + 1,
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", format!("v-{index:03}")).unwrap(),
                subject: RuntimeRef::new("document", format!("d-{index:03}")).unwrap(),
                field: "body".into(),
                valid_from: 1,
                valid_to: None,
                value: VectorValue::Dense { values },
                provenance: None,
                properties,
            },
        });
    }
    let query = (0..16)
        .map(|_| {
            let raw = (next(&mut seed) >> 32) as u32;
            (raw as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect::<Vec<_>>();
    let read = ReadStamp::new(
        scope.clone(),
        None,
        0,
        candidates.len() as u64,
        Some("11".repeat(32)),
    )
    .unwrap();
    let request = SearchRequest {
        scope,
        read,
        valid_at: 2,
        field: "body".into(),
        query: VectorQuery::Dense {
            values: query.clone(),
        },
        metric: ScoreMetric::Dot,
        top_k: 25,
        mode: SearchMode::Exact,
        filter: Some(FilterExpression::Condition {
            condition: FilterCondition {
                property: "bucket".into(),
                operator: FilterOperator::Range {
                    gt: None,
                    gte: Some(RuntimeValue::Unsigned(2)),
                    lt: Some(RuntimeValue::Unsigned(6)),
                    lte: None,
                },
            },
        }),
    };

    let actual = search_exact(&request, candidates.clone()).unwrap();
    let mut expected = candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.vector.properties.get("bucket"),
                Some(RuntimeValue::Unsigned(value)) if (2..6).contains(value)
            )
        })
        .map(|candidate| {
            let VectorValue::Dense { values } = &candidate.vector.value else {
                unreachable!()
            };
            let score = query
                .iter()
                .zip(values)
                .map(|(left, right)| f64::from(*left) * f64::from(*right))
                .sum::<f64>();
            (
                candidate.vector.reference.clone(),
                candidate.source_cursor,
                score,
            )
        })
        .collect::<Vec<_>>();
    expected.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| right.1.cmp(&left.1))
    });
    expected.truncate(25);

    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.reference, expected.0);
        assert_eq!(actual.source_cursor, expected.1);
        assert_eq!(actual.score.total_cmp(&expected.2), Ordering::Equal);
    }
}
