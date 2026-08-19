use std::collections::{BTreeSet, HashSet};
use vyrm_core::{
    ProjectionId, ReadStamp, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector, ScopeId,
    VectorValue,
};
use vyrm_vector::{
    search_exact_ref, HnswConfig, HnswIndex, ImmutableVectorSegment, ScoreMetric, SearchMode,
    SearchRequest, VectorCandidate, VectorCatalog, VectorQuery, VectorSegmentConfig,
};

#[test]
fn deterministic_update_delete_reopen_and_generation_compaction_soak() {
    let scope = ScopeId::new("instance:vector-lifecycle-soak").unwrap();
    let exact_id = ProjectionId::new("vector:exact:soak").unwrap();
    let hnsw_id = ProjectionId::new("vector:hnsw:soak").unwrap();
    let mut random = 0x51a7_5eed_u64;
    let mut history = Vec::new();
    let mut cursor = 0;
    for identity in 0..128 {
        cursor += 1;
        history.push(candidate(
            &scope,
            cursor,
            identity,
            vector(16, &mut random),
            false,
        ));
    }
    let exact_config = VectorSegmentConfig {
        id: exact_id.clone(),
        scope: scope.clone(),
        field: "body".into(),
        dimensions: 16,
        metric: ScoreMetric::Cosine,
        filter_properties: BTreeSet::from(["group".into()]),
    };
    let hnsw_config = HnswConfig {
        id: hnsw_id.clone(),
        scope: scope.clone(),
        field: "body".into(),
        dimensions: 16,
        metric: ScoreMetric::Cosine,
        m: 12,
        ef_construction: 64,
        max_level: 10,
        seed: 19,
        filter_properties: BTreeSet::from(["group".into()]),
    };
    let mut catalog = VectorCatalog::default();

    for generation in 1..=8 {
        if generation > 1 {
            for operation in 0..24 {
                let identity = (next(&mut random) % 128) as usize;
                cursor += 1;
                history.push(candidate(
                    &scope,
                    cursor,
                    identity,
                    vector(16, &mut random),
                    operation % 7 == 0,
                ));
            }
        }

        let segment = ImmutableVectorSegment::build(
            exact_config.clone(),
            generation,
            cursor,
            history.clone(),
        )
        .unwrap();
        let reopened_segment = ImmutableVectorSegment::from_bytes(segment.as_bytes()).unwrap();
        assert_eq!(segment.as_bytes(), reopened_segment.as_bytes());

        let index =
            HnswIndex::build(hnsw_config.clone(), generation, cursor, history.clone()).unwrap();
        let repeated =
            HnswIndex::build(hnsw_config.clone(), generation, cursor, history.clone()).unwrap();
        assert_eq!(index.as_bytes(), repeated.as_bytes());
        let reopened_index = HnswIndex::from_bytes(index.as_bytes()).unwrap();

        let query = vector(16, &mut random);
        let exact_request = request(&scope, cursor, query.clone(), SearchMode::Exact);
        let expected = search_exact_ref(&exact_request, &history).unwrap();
        assert_eq!(reopened_segment.search(&exact_request).unwrap(), expected);

        let ef = history.len();
        let approximate_request = request(
            &scope,
            cursor,
            query,
            SearchMode::RequireApproximate { exact_rerank: ef },
        );
        let approximate = reopened_index.search(&approximate_request, ef).unwrap();
        assert_eq!(identities(&approximate), identities(&expected));

        let revision = catalog.revision;
        catalog
            .publish(revision, reopened_segment.descriptor().clone())
            .unwrap();
        let revision = catalog.revision;
        catalog
            .publish(revision, reopened_index.descriptor().clone())
            .unwrap();
    }

    assert_eq!(catalog.entries.len(), 2);
    assert_eq!(catalog.retired.len(), 14);
    let protected = BTreeSet::from([(exact_id, 7), (hnsw_id, 7)]);
    let reclaimed = catalog.reclaim_retired(&protected);
    assert_eq!(reclaimed.len(), 12);
    assert_eq!(catalog.retired.len(), 2);
}

fn request(scope: &ScopeId, cursor: u64, query: Vec<f32>, mode: SearchMode) -> SearchRequest {
    SearchRequest {
        scope: scope.clone(),
        read: ReadStamp::new(scope.clone(), None, 0, cursor, Some("22".repeat(32))).unwrap(),
        valid_at: 100,
        field: "body".into(),
        query: VectorQuery::Dense { values: query },
        metric: ScoreMetric::Cosine,
        top_k: 10,
        mode,
        filter: None,
    }
}

fn candidate(
    scope: &ScopeId,
    cursor: u64,
    identity: usize,
    values: Vec<f32>,
    deleted: bool,
) -> VectorCandidate {
    let mut properties = RuntimeProperties::new();
    properties.insert(
        "group".into(),
        RuntimeValue::Unsigned((identity % 8) as u64),
    );
    VectorCandidate {
        scope: scope.clone(),
        source_cursor: cursor,
        vector: RuntimeVector {
            reference: RuntimeRef::new("embedding", format!("v-{identity:03}")).unwrap(),
            subject: RuntimeRef::new("document", format!("d-{identity:03}")).unwrap(),
            field: "body".into(),
            valid_from: 1,
            valid_to: deleted.then_some(100),
            value: VectorValue::Dense { values },
            provenance: None,
            properties,
        },
    }
}

fn identities(hits: &[vyrm_vector::SearchHit]) -> HashSet<RuntimeRef> {
    hits.iter().map(|hit| hit.reference.clone()).collect()
}

fn vector(dimensions: usize, random: &mut u64) -> Vec<f32> {
    let mut values = (0..dimensions)
        .map(|_| {
            let value = next(random);
            ((value >> 32) as u32 as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect::<Vec<_>>();
    let norm = values.iter().map(|value| value.powi(2)).sum::<f32>().sqrt();
    for value in &mut values {
        *value /= norm;
    }
    values
}

fn next(random: &mut u64) -> u64 {
    *random = random
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *random
}
