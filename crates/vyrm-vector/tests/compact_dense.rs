use std::collections::BTreeSet;
use tempfile::tempdir;
use vyrm_core::{
    digest, ProjectionId, ReadStamp, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector,
    ScopeId, VectorValue,
};
use vyrm_vector::{
    search_exact_ref, CompactDenseSegment, DenseKernel, DenseMemoryPlacement, ScoreMetric,
    SearchMode, SearchRequest, VectorCandidate, VectorQuery, VectorSegmentConfig,
    COMPACT_DENSE_FORMAT_VERSION,
};

fn candidate(
    scope: &ScopeId,
    cursor: u64,
    id: &str,
    values: Vec<f32>,
    valid_to: Option<u64>,
) -> VectorCandidate {
    let mut properties = RuntimeProperties::new();
    properties.insert("partition".into(), RuntimeValue::String("test".into()));
    VectorCandidate {
        scope: scope.clone(),
        source_cursor: cursor,
        vector: RuntimeVector {
            reference: RuntimeRef::new("embedding", id).unwrap(),
            subject: RuntimeRef::new("document", id).unwrap(),
            field: "body".into(),
            valid_from: 1,
            valid_to,
            value: VectorValue::Dense { values },
            provenance: None,
            properties,
        },
    }
}

fn config(scope: &ScopeId, dimensions: usize, metric: ScoreMetric) -> VectorSegmentConfig {
    VectorSegmentConfig {
        id: ProjectionId::new(format!("vector:compact:{metric:?}").to_lowercase()).unwrap(),
        scope: scope.clone(),
        field: "body".into(),
        dimensions,
        metric,
        embedding_model: None,
        filter_properties: BTreeSet::from(["partition".into()]),
    }
}

fn request(scope: &ScopeId, dimensions: usize, metric: ScoreMetric) -> SearchRequest {
    let query = (0..dimensions)
        .map(|index| ((index * 17 + 3) % 23) as f32 / 23.0 - 0.4)
        .collect();
    SearchRequest {
        scope: scope.clone(),
        read: ReadStamp::new(scope.clone(), None, 0, 130, Some("11".repeat(32))).unwrap(),
        valid_at: 7,
        field: "body".into(),
        query: VectorQuery::Dense { values: query },
        metric,
        embedding_model: None,
        top_k: 12,
        mode: SearchMode::Exact,
        filter: None,
    }
}

fn corpus(scope: &ScopeId, dimensions: usize) -> Vec<VectorCandidate> {
    (1..=130)
        .map(|cursor| {
            let identity = if cursor == 130 { 3 } else { cursor };
            let values = (0..dimensions)
                .map(|dimension| {
                    (((identity * 31 + dimension as u64 * 13) % 101) as f32 / 50.0) - 1.0
                })
                .collect();
            candidate(
                scope,
                cursor,
                &format!("v{identity:03}"),
                values,
                (identity == 7).then_some(6),
            )
        })
        .collect()
}

#[test]
fn owned_mmap_scalar_and_simd_match_the_exact_oracle() {
    let scope = ScopeId::new("instance:compact-parity").unwrap();
    let candidates = corpus(&scope, 67);
    let root = tempdir().unwrap();
    for metric in [
        ScoreMetric::Cosine,
        ScoreMetric::Dot,
        ScoreMetric::Euclidean,
        ScoreMetric::Manhattan,
    ] {
        let request = request(&scope, 67, metric);
        let expected = search_exact_ref(&request, &candidates).unwrap();
        let owned =
            CompactDenseSegment::build(config(&scope, 67, metric), 1, 130, candidates.clone())
                .unwrap();
        assert_eq!(owned.memory_placement(), DenseMemoryPlacement::Owned);
        let scalar = owned.search(&request, DenseKernel::Scalar).unwrap();
        let dispatched = owned.search(&request, DenseKernel::Auto).unwrap();
        assert_hit_parity(&expected, &scalar, 1e-10);
        assert_hit_parity(&expected, &dispatched, 2e-5);

        let path = root.path().join(format!("{metric:?}.vyrdense"));
        owned.write_atomic(&path).unwrap();
        owned.write_atomic(&path).unwrap();
        let mapped = CompactDenseSegment::open_mmap(&path).unwrap();
        assert_eq!(mapped.memory_placement(), DenseMemoryPlacement::Mapped);
        assert_eq!(mapped.descriptor(), owned.descriptor());
        assert_hit_parity(
            &expected,
            &mapped.search(&request, DenseKernel::Auto).unwrap(),
            2e-5,
        );
    }
}

#[test]
fn compact_artifact_is_deterministic_bounded_and_smaller_than_json() {
    let scope = ScopeId::new("instance:compact-size").unwrap();
    let candidates = corpus(&scope, 128);
    let first = CompactDenseSegment::build(
        config(&scope, 128, ScoreMetric::Cosine),
        4,
        130,
        candidates.clone(),
    )
    .unwrap();
    let second = CompactDenseSegment::build(
        config(&scope, 128, ScoreMetric::Cosine),
        4,
        130,
        candidates.clone(),
    )
    .unwrap();
    assert_eq!(first.as_bytes(), second.as_bytes());
    assert_eq!(
        first.as_bytes()[8..10],
        COMPACT_DENSE_FORMAT_VERSION.to_le_bytes()
    );
    assert_eq!(first.vector_payload_bytes(), 130 * 512);
    let json = serde_json::to_vec(&candidates).unwrap();
    assert!(
        first.as_bytes().len() < json.len(),
        "compact={} json={}",
        first.as_bytes().len(),
        json.len()
    );
}

#[test]
fn header_metadata_payload_padding_and_truncation_corruption_fail_closed() {
    let scope = ScopeId::new("instance:compact-corrupt").unwrap();
    let artifact = CompactDenseSegment::build(
        config(&scope, 3, ScoreMetric::Dot),
        1,
        2,
        vec![
            candidate(&scope, 1, "a", vec![1.0, 2.0, 3.0], None),
            candidate(&scope, 2, "b", vec![3.0, 2.0, 1.0], None),
        ],
    )
    .unwrap();
    for offset in [0, 120, artifact.as_bytes().len() - 1] {
        let mut corrupt = artifact.as_bytes().to_vec();
        corrupt[offset] ^= 0x40;
        assert!(CompactDenseSegment::from_bytes(&corrupt).is_err());
    }
    assert!(CompactDenseSegment::from_bytes(&artifact.as_bytes()[..127]).is_err());
    assert!(
        CompactDenseSegment::from_bytes(&artifact.as_bytes()[..artifact.as_bytes().len() - 1])
            .is_err()
    );

    let root = tempdir().unwrap();
    let path = root.path().join("collision.vyrdense");
    artifact.write_atomic(&path).unwrap();
    let other = CompactDenseSegment::build(
        config(&scope, 3, ScoreMetric::Dot),
        2,
        2,
        vec![candidate(&scope, 1, "c", vec![1.0, 1.0, 1.0], None)],
    )
    .unwrap();
    assert!(other.write_atomic(&path).is_err());
}

#[test]
fn concurrent_publication_never_overwrites_the_winning_artifact() {
    let scope = ScopeId::new("instance:compact-race").unwrap();
    let first = CompactDenseSegment::build(
        config(&scope, 3, ScoreMetric::Dot),
        1,
        1,
        vec![candidate(&scope, 1, "a", vec![1.0, 0.0, 0.0], None)],
    )
    .unwrap();
    let second = CompactDenseSegment::build(
        config(&scope, 3, ScoreMetric::Dot),
        2,
        1,
        vec![candidate(&scope, 1, "b", vec![0.0, 1.0, 0.0], None)],
    )
    .unwrap();
    let root = tempdir().unwrap();
    let path = root.path().join("race.vyrdense");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let handles = [first.clone(), second.clone()].map(|artifact| {
        let path = path.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            artifact.write_atomic(path)
        })
    });
    barrier.wait();
    let outcomes = handles.map(|handle| handle.join().unwrap());
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    let published = CompactDenseSegment::open_mmap(path).unwrap();
    assert!(published.as_bytes() == first.as_bytes() || published.as_bytes() == second.as_bytes());
}

#[test]
fn stale_reads_and_non_dense_queries_are_denied() {
    let scope = ScopeId::new("instance:compact-deny").unwrap();
    let artifact = CompactDenseSegment::build(
        config(&scope, 3, ScoreMetric::Dot),
        1,
        1,
        vec![candidate(&scope, 1, "a", vec![1.0, 0.0, 0.0], None)],
    )
    .unwrap();
    let stale = SearchRequest {
        scope: scope.clone(),
        read: ReadStamp::new(scope.clone(), None, 0, 2, Some(digest::sha256_hex(b"head"))).unwrap(),
        valid_at: 2,
        field: "body".into(),
        query: VectorQuery::Dense {
            values: vec![1.0, 0.0, 0.0],
        },
        metric: ScoreMetric::Dot,
        embedding_model: None,
        top_k: 1,
        mode: SearchMode::Exact,
        filter: None,
    };
    assert!(artifact.search(&stale, DenseKernel::Auto).is_err());

    let sparse = SearchRequest {
        read: ReadStamp::new(scope.clone(), None, 0, 1, Some(digest::sha256_hex(b"head"))).unwrap(),
        scope,
        query: VectorQuery::Sparse {
            dimensions: 3,
            indices: vec![0],
            values: vec![1.0],
        },
        ..stale
    };
    assert!(artifact.search(&sparse, DenseKernel::Auto).is_err());
}

fn assert_hit_parity(
    expected: &[vyrm_vector::SearchHit],
    actual: &[vyrm_vector::SearchHit],
    epsilon: f64,
) {
    assert_eq!(expected.len(), actual.len());
    for (expected, actual) in expected.iter().zip(actual) {
        assert_eq!(expected.reference, actual.reference);
        assert_eq!(expected.subject, actual.subject);
        assert_eq!(expected.source_cursor, actual.source_cursor);
        assert!(
            (expected.score - actual.score).abs() <= epsilon,
            "expected {} actual {}",
            expected.score,
            actual.score
        );
    }
}
