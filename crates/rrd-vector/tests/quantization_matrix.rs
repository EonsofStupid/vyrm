use rrd_core::{
    ProjectionId, ReadStamp, RuntimeProperties, RuntimeRef, RuntimeVector, ScopeId, VectorValue,
};
use rrd_vector::{
    search_exact_ref, ProductCompression, QuantizationMethod, QuantizedKernel, QuantizedSegment,
    QuantizedSegmentConfig, ScoreMetric, SearchHit, SearchMode, SearchRequest, TurboQuantBits,
    TurboQuantSegment, TurboQuantSegmentConfig, VectorCandidate, VectorQuery,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::time::Instant;

const VECTORS: usize = 512;
const DIMENSIONS: usize = 64;
const QUERIES: usize = 8;
const TOP_K: usize = 10;
const RERANK: usize = 96;
const MAX_MEAN_ABSOLUTE_SCORE_ERROR: f64 = 0.15;
const MAX_BUILD_NANOSECONDS_PER_ARTIFACT: u128 = 5_000_000_000;
const MAX_SEARCH_MATRIX_NANOSECONDS_PER_ARTIFACT: u128 = 5_000_000_000;

enum MatrixArtifact {
    Quantized(Box<QuantizedSegment>),
    Turbo(Box<TurboQuantSegment>),
}

impl MatrixArtifact {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Quantized(segment) => segment.as_bytes(),
            Self::Turbo(segment) => segment.as_bytes(),
        }
    }

    fn accounting(&self) -> (usize, usize, usize) {
        match self {
            Self::Quantized(segment) => {
                let descriptor = segment.descriptor();
                (
                    descriptor.packed_vector_bytes,
                    descriptor.full_precision_vector_bytes,
                    descriptor.auxiliary_bytes,
                )
            }
            Self::Turbo(segment) => {
                let descriptor = segment.descriptor();
                (
                    descriptor.packed_vector_bytes,
                    descriptor.full_precision_vector_bytes,
                    0,
                )
            }
        }
    }

    fn search(
        &self,
        request: &SearchRequest,
        limit: usize,
        kernel: QuantizedKernel,
    ) -> Vec<SearchHit> {
        match self {
            Self::Quantized(segment) => segment
                .search_candidates_at(request, limit, VECTORS as u64, kernel)
                .unwrap(),
            Self::Turbo(segment) => segment
                .search_candidates_at_with_kernel(request, limit, VECTORS as u64, kernel)
                .unwrap(),
        }
    }
}

#[derive(Clone, Copy)]
enum MatrixMethod {
    Scalar,
    Product(ProductCompression),
    Binary,
    Turbo(TurboQuantBits),
}

impl MatrixMethod {
    fn label(self) -> String {
        match self {
            Self::Scalar => "scalar".into(),
            Self::Product(compression) => format!("product_{}x", compression.ratio()),
            Self::Binary => "binary".into(),
            Self::Turbo(bits) => format!(
                "turboquant_{}",
                match bits {
                    TurboQuantBits::Bits4 => "4bit",
                    TurboQuantBits::Bits2 => "2bit",
                    TurboQuantBits::Bits1_5 => "1_5bit",
                    TurboQuantBits::Bits1 => "1bit",
                }
            ),
        }
    }

    fn expected_ratio(self) -> usize {
        match self {
            Self::Scalar => 4,
            Self::Product(compression) => compression.ratio(),
            Self::Binary | Self::Turbo(TurboQuantBits::Bits1) => 32,
            Self::Turbo(TurboQuantBits::Bits1_5) => 21,
            Self::Turbo(TurboQuantBits::Bits2) => 16,
            Self::Turbo(TurboQuantBits::Bits4) => 8,
        }
    }
}

#[test]
fn fixed_quantization_bias_recall_memory_compression_and_latency_matrix_passes() {
    let scope = ScopeId::new("instance:quantization-matrix").unwrap();
    let candidates = (0..VECTORS)
        .map(|row| candidate(&scope, row))
        .collect::<Vec<_>>();
    let queries = (0..QUERIES)
        .map(|query| deterministic_vector(query * 47 + 11))
        .collect::<Vec<_>>();
    let methods = [
        MatrixMethod::Scalar,
        MatrixMethod::Product(ProductCompression::X4),
        MatrixMethod::Product(ProductCompression::X8),
        MatrixMethod::Product(ProductCompression::X16),
        MatrixMethod::Product(ProductCompression::X32),
        MatrixMethod::Product(ProductCompression::X64),
        MatrixMethod::Binary,
        MatrixMethod::Turbo(TurboQuantBits::Bits4),
        MatrixMethod::Turbo(TurboQuantBits::Bits2),
        MatrixMethod::Turbo(TurboQuantBits::Bits1_5),
        MatrixMethod::Turbo(TurboQuantBits::Bits1),
    ];
    let mut rows = Vec::new();
    for method in methods {
        let build_started = Instant::now();
        let artifact = build(method, &scope, &candidates);
        let build_ns = build_started.elapsed().as_nanos();
        let (packed, full, auxiliary) = artifact.accounting();
        let packed_ratio = full / packed;
        assert_eq!(packed_ratio, method.expected_ratio(), "{}", method.label());
        assert!(artifact.bytes().len() >= packed + auxiliary);

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(artifact.bytes()).unwrap();
        file.flush().unwrap();
        match &artifact {
            MatrixArtifact::Quantized(segment) => {
                let mapped = QuantizedSegment::open_mmap(file.path()).unwrap();
                assert_eq!(mapped.descriptor(), segment.descriptor());
                let mut corrupt = segment.as_bytes().to_vec();
                *corrupt.last_mut().unwrap() ^= 0x80;
                assert!(QuantizedSegment::from_bytes(&corrupt).is_err());
            }
            MatrixArtifact::Turbo(segment) => {
                let mapped = TurboQuantSegment::open_mmap(file.path()).unwrap();
                assert_eq!(mapped.descriptor(), segment.descriptor());
                let mut corrupt = segment.as_bytes().to_vec();
                *corrupt.last_mut().unwrap() ^= 0x80;
                assert!(TurboQuantSegment::from_bytes(&corrupt).is_err());
            }
        }

        let mut absolute_error = 0.0;
        let mut score_samples = 0_usize;
        let mut recall = 0.0;
        for query in &queries {
            let approximate_request = request(
                &scope,
                query,
                TOP_K,
                SearchMode::RequireApproximate {
                    exact_rerank: RERANK,
                },
            );
            let automatic = artifact.search(&approximate_request, VECTORS, QuantizedKernel::Auto);
            let scalar = artifact.search(&approximate_request, VECTORS, QuantizedKernel::Scalar);
            assert_eq!(automatic.len(), scalar.len());
            for (automatic, scalar) in automatic.iter().zip(&scalar) {
                assert_eq!(automatic.reference, scalar.reference);
                let tolerance = 1e-5 * automatic.score.abs().max(scalar.score.abs()).max(1.0);
                assert!(
                    (automatic.score - scalar.score).abs() <= tolerance,
                    "{} SIMD/scalar score divergence for {:?}",
                    method.label(),
                    automatic.reference
                );
            }
        }
        let search_started = Instant::now();
        for query in &queries {
            let exact_all_request = request(&scope, query, VECTORS, SearchMode::Exact);
            let exact_all = search_exact_ref(&exact_all_request, &candidates).unwrap();
            let exact_scores = exact_all
                .iter()
                .map(|hit| (hit.reference.clone(), hit.score))
                .collect::<BTreeMap<_, _>>();
            let approximate_request = request(
                &scope,
                query,
                TOP_K,
                SearchMode::RequireApproximate {
                    exact_rerank: RERANK,
                },
            );
            let approximate_all =
                artifact.search(&approximate_request, VECTORS, QuantizedKernel::Auto);
            for hit in &approximate_all {
                absolute_error += (exact_scores[&hit.reference] - hit.score).abs();
                score_samples += 1;
            }
            let rerank_references = approximate_all
                .iter()
                .take(RERANK)
                .map(|hit| hit.reference.clone())
                .collect::<BTreeSet<_>>();
            let exact_request = request(&scope, query, TOP_K, SearchMode::Exact);
            let expected = search_exact_ref(&exact_request, &candidates).unwrap();
            let reranked = search_exact_ref(
                &exact_request,
                candidates
                    .iter()
                    .filter(|candidate| rerank_references.contains(&candidate.vector.reference)),
            )
            .unwrap();
            recall += recall_at_k(&expected, &reranked);
        }
        let search_ns = search_started.elapsed().as_nanos();
        let mean_recall = recall / QUERIES as f64;
        let mean_absolute_score_error = absolute_error / score_samples as f64;
        assert!(
            mean_recall >= 0.80,
            "{} recall={mean_recall}",
            method.label()
        );
        assert!(
            mean_absolute_score_error <= MAX_MEAN_ABSOLUTE_SCORE_ERROR,
            "{} score bias={mean_absolute_score_error}",
            method.label()
        );
        assert!(
            build_ns > 0 && build_ns <= MAX_BUILD_NANOSECONDS_PER_ARTIFACT,
            "{} build latency={build_ns}ns",
            method.label()
        );
        assert!(
            search_ns > 0 && search_ns <= MAX_SEARCH_MATRIX_NANOSECONDS_PER_ARTIFACT,
            "{} search-matrix latency={search_ns}ns",
            method.label()
        );
        rows.push(json!({
            "method": method.label(),
            "packed_compression_ratio": packed_ratio,
            "packed_vector_bytes": packed,
            "auxiliary_bytes": auxiliary,
            "artifact_bytes": artifact.bytes().len(),
            "full_precision_vector_bytes": full,
            "mean_absolute_score_error": mean_absolute_score_error,
            "recall_at_10_after_exact_rerank_96": mean_recall,
            "build_nanoseconds": build_ns,
            "search_matrix_nanoseconds": search_ns,
        }));
    }
    assert_eq!(rows.len(), 11);
    assert!(rows
        .iter()
        .any(|row| { row["method"] == "product_64x" && row["packed_compression_ratio"] == 64 }));
    eprintln!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "rrflow.quantization-evidence.v1",
            "profile": {
                "vectors": VECTORS,
                "dimensions": DIMENSIONS,
                "queries": QUERIES,
                "top_k": TOP_K,
                "exact_rerank": RERANK,
            },
            "rows": rows,
        }))
        .unwrap()
    );
}

#[test]
fn checked_in_quantization_evidence_is_complete_and_honestly_scoped() {
    let evidence: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/evidence/g04-w04-quantization-local-512x64.json"
    ))
    .unwrap();
    assert_eq!(evidence["schema"], "rrflow.quantization-evidence.v1");
    assert_eq!(evidence["profile"]["vectors"], VECTORS);
    assert_eq!(evidence["profile"]["dimensions"], DIMENSIONS);
    assert_eq!(evidence["rows"].as_array().unwrap().len(), 11);
    assert!(evidence["rows"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| { row["method"] == "product_64x" && row["packed_compression_ratio"] == 64 }));
    assert!(evidence["rows"].as_array().unwrap().iter().all(|row| {
        row["recall_at_10_after_exact_rerank_96"]
            .as_f64()
            .is_some_and(|recall| recall >= 0.80)
    }));
    assert!(evidence["scope"]
        .as_str()
        .is_some_and(|scope| scope.contains("not release SLOs")));
}

fn build(method: MatrixMethod, scope: &ScopeId, candidates: &[VectorCandidate]) -> MatrixArtifact {
    let id = ProjectionId::new(format!("quant-matrix-{}", method.label())).unwrap();
    match method {
        MatrixMethod::Scalar | MatrixMethod::Product(_) | MatrixMethod::Binary => {
            MatrixArtifact::Quantized(Box::new(
                QuantizedSegment::build(
                    QuantizedSegmentConfig {
                        id,
                        scope: scope.clone(),
                        field: "body".into(),
                        dimensions: DIMENSIONS,
                        metric: ScoreMetric::Cosine,
                        method: match method {
                            MatrixMethod::Scalar => QuantizationMethod::Scalar,
                            MatrixMethod::Product(compression) => {
                                QuantizationMethod::Product { compression }
                            }
                            MatrixMethod::Binary => QuantizationMethod::Binary,
                            MatrixMethod::Turbo(_) => unreachable!(),
                        },
                        embedding_model: None,
                        filter_properties: BTreeSet::new(),
                    },
                    1,
                    VECTORS as u64,
                    candidates.to_vec(),
                )
                .unwrap(),
            ))
        }
        MatrixMethod::Turbo(bits) => MatrixArtifact::Turbo(Box::new(
            TurboQuantSegment::build(
                TurboQuantSegmentConfig {
                    id,
                    scope: scope.clone(),
                    field: "body".into(),
                    dimensions: DIMENSIONS,
                    metric: ScoreMetric::Cosine,
                    bits,
                    seed: 0x5eed,
                    embedding_model: None,
                    filter_properties: BTreeSet::new(),
                },
                1,
                VECTORS as u64,
                candidates.to_vec(),
            )
            .unwrap(),
        )),
    }
}

fn request(scope: &ScopeId, query: &[f32], top_k: usize, mode: SearchMode) -> SearchRequest {
    SearchRequest {
        read: ReadStamp::new(
            scope.clone(),
            None,
            0,
            VECTORS as u64,
            Some("11".repeat(32)),
        )
        .unwrap(),
        scope: scope.clone(),
        valid_at: 10,
        field: "body".into(),
        query: VectorQuery::Dense {
            values: query.to_vec(),
        },
        metric: ScoreMetric::Cosine,
        embedding_model: None,
        top_k,
        mode,
        filter: None,
    }
}

fn candidate(scope: &ScopeId, row: usize) -> VectorCandidate {
    VectorCandidate {
        scope: scope.clone(),
        source_cursor: row as u64 + 1,
        vector: RuntimeVector {
            reference: RuntimeRef::new("embedding", format!("v{row:04}")).unwrap(),
            subject: RuntimeRef::new("document", format!("v{row:04}")).unwrap(),
            collection: None,
            field: "body".into(),
            valid_from: 1,
            valid_to: None,
            value: VectorValue::Dense {
                values: deterministic_vector(row),
            },
            provenance: None,
            properties: RuntimeProperties::new(),
        },
    }
}

fn deterministic_vector(seed: usize) -> Vec<f32> {
    let mut state = (seed as u64 + 1) * 0x9e37_79b9;
    let mut values = (0..DIMENSIONS)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 40) as i32 - (1 << 23)) as f32 / (1 << 23) as f32
        })
        .collect::<Vec<_>>();
    let norm = values
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt() as f32;
    for value in &mut values {
        *value /= norm;
    }
    values
}

fn recall_at_k(expected: &[SearchHit], actual: &[SearchHit]) -> f64 {
    let expected = expected
        .iter()
        .map(|hit| hit.reference.clone())
        .collect::<BTreeSet<_>>();
    actual
        .iter()
        .filter(|hit| expected.contains(&hit.reference))
        .count() as f64
        / expected.len() as f64
}
