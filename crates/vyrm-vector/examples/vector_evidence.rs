use serde::Serialize;
use std::collections::{BTreeSet, HashSet};
use std::time::Instant;
use vyrm_core::{
    ProjectionId, ReadStamp, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector, ScopeId,
    VectorValue,
};
use vyrm_vector::{
    search_exact_ref, FilterCondition, FilterExpression, FilterOperator, HnswConfig, HnswIndex,
    ScalarQuantizedVector, ScoreMetric, SearchMode, SearchRequest, VectorCandidate, VectorQuery,
};

#[derive(Serialize)]
struct Evidence {
    schema: &'static str,
    profile: Profile,
    build: BuildEvidence,
    quantization: QuantizationEvidence,
    searches: Vec<SearchEvidence>,
}

#[derive(Serialize)]
struct Profile {
    vectors: usize,
    dimensions: usize,
    queries: usize,
    top_k: usize,
    m: usize,
    ef_construction: usize,
    seed: u64,
}

#[derive(Serialize)]
struct BuildEvidence {
    milliseconds: f64,
    artifact_bytes: usize,
    raw_f32_payload_bytes: usize,
    artifact_to_raw_payload_ratio: f64,
    resident_kib_before_build: Option<u64>,
    resident_kib_after_reopen: Option<u64>,
    peak_resident_kib_after_reopen: Option<u64>,
}

#[derive(Serialize)]
struct QuantizationEvidence {
    payload_bytes: usize,
    raw_payload_ratio: f64,
    mean_absolute_score_error: f64,
    recall_at_k_after_exact_rerank: f64,
    rerank_candidates: usize,
}

#[derive(Serialize)]
struct SearchEvidence {
    filter_percent: usize,
    ef_search: usize,
    estimated_graph_cost_units: u64,
    planner_preference: &'static str,
    mean_recall_at_k: f64,
    complete_result_rate: f64,
    exact_mean_milliseconds: f64,
    ann_mean_milliseconds: f64,
}

struct SearchContext<'a> {
    scope: &'a ScopeId,
    candidates: &'a [VectorCandidate],
    index: &'a HnswIndex,
    queries: &'a [Vec<f32>],
    cursor: u64,
    top_k: usize,
}

fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let vectors = parse(&arguments, 0, 2_000);
    let dimensions = parse(&arguments, 1, 32);
    let queries = parse(&arguments, 2, 40);
    let seed = 0x5eed_c1ff_u64;
    let top_k = 10;
    assert!(vectors >= 100 && dimensions > 0 && queries > 0);

    let scope = ScopeId::new("instance:vector-evidence").unwrap();
    let mut random = seed;
    let candidates = (0..vectors)
        .map(|index| candidate(&scope, index, dimensions, &mut random))
        .collect::<Vec<_>>();
    let query_values = (0..queries)
        .map(|_| unit_vector(dimensions, &mut random))
        .collect::<Vec<_>>();
    let config = HnswConfig {
        id: ProjectionId::new("vector:hnsw:evidence").unwrap(),
        scope: scope.clone(),
        field: "body".into(),
        dimensions,
        metric: ScoreMetric::Cosine,
        embedding_model: None,
        m: 16,
        ef_construction: 100,
        max_level: 12,
        seed,
        filter_properties: BTreeSet::from(["bucket".into()]),
    };
    let memory_before = process_memory();
    let started = Instant::now();
    let index = HnswIndex::build(config.clone(), 1, vectors as u64, candidates.clone()).unwrap();
    let build_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let reopened = HnswIndex::from_bytes(index.as_bytes()).unwrap();
    assert_eq!(index.descriptor(), reopened.descriptor());
    let artifact_bytes = index.as_bytes().len();
    drop(index);
    let memory_after = process_memory();

    let quantized = candidates
        .iter()
        .map(|candidate| {
            let VectorValue::Dense { values } = &candidate.vector.value else {
                unreachable!()
            };
            ScalarQuantizedVector::encode(values).unwrap()
        })
        .collect::<Vec<_>>();
    let quantization = quantify(
        &scope,
        &candidates,
        &query_values,
        &quantized,
        vectors as u64,
        top_k,
    );

    let mut searches = Vec::new();
    let search_context = SearchContext {
        scope: &scope,
        candidates: &candidates,
        index: &reopened,
        queries: &query_values,
        cursor: vectors as u64,
        top_k,
    };
    for filter_percent in [100, 50, 10, 1] {
        for ef_search in [32, 64, 128, 256] {
            searches.push(measure_search(&search_context, filter_percent, ef_search));
        }
    }

    let evidence = Evidence {
        schema: "vyrm.vector-evidence.v1",
        profile: Profile {
            vectors,
            dimensions,
            queries,
            top_k,
            m: config.m,
            ef_construction: config.ef_construction,
            seed,
        },
        build: BuildEvidence {
            milliseconds: build_ms,
            artifact_bytes,
            raw_f32_payload_bytes: vectors * dimensions * std::mem::size_of::<f32>(),
            artifact_to_raw_payload_ratio: artifact_bytes as f64
                / (vectors * dimensions * std::mem::size_of::<f32>()) as f64,
            resident_kib_before_build: memory_before.map(|memory| memory.resident_kib),
            resident_kib_after_reopen: memory_after.map(|memory| memory.resident_kib),
            peak_resident_kib_after_reopen: memory_after.map(|memory| memory.peak_kib),
        },
        quantization,
        searches,
    };
    println!("{}", serde_json::to_string_pretty(&evidence).unwrap());
}

#[derive(Clone, Copy)]
struct ProcessMemory {
    resident_kib: u64,
    peak_kib: u64,
}

fn process_memory() -> Option<ProcessMemory> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let value = |name: &str| {
        status.lines().find_map(|line| {
            line.strip_prefix(name)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
    };
    Some(ProcessMemory {
        resident_kib: value("VmRSS:")?,
        peak_kib: value("VmHWM:")?,
    })
}

fn measure_search(
    context: &SearchContext<'_>,
    filter_percent: usize,
    ef_search: usize,
) -> SearchEvidence {
    let mut recall = 0.0;
    let mut complete = 0;
    let mut exact_time = 0.0;
    let mut ann_time = 0.0;
    for query in context.queries {
        let exact_request = request(
            context.scope,
            context.cursor,
            query,
            context.top_k,
            filter_percent,
            SearchMode::Exact,
        );
        let started = Instant::now();
        let exact = search_exact_ref(&exact_request, context.candidates).unwrap();
        exact_time += started.elapsed().as_secs_f64() * 1_000.0;
        let approximate_request = request(
            context.scope,
            context.cursor,
            query,
            context.top_k,
            filter_percent,
            SearchMode::RequireApproximate {
                exact_rerank: ef_search,
            },
        );
        let started = Instant::now();
        let approximate = context
            .index
            .search(&approximate_request, ef_search)
            .unwrap();
        ann_time += started.elapsed().as_secs_f64() * 1_000.0;
        if approximate.len() == exact.len() {
            complete += 1;
        }
        recall += recall_at_k(&exact, &approximate);
    }
    let planning_request = request(
        context.scope,
        context.cursor,
        &context.queries[0],
        context.top_k,
        filter_percent,
        SearchMode::AllowApproximate {
            exact_rerank: ef_search,
        },
    );
    let estimated_graph_cost_units = context
        .index
        .estimated_search_cost(&planning_request, ef_search)
        .unwrap();
    SearchEvidence {
        filter_percent,
        ef_search,
        estimated_graph_cost_units,
        planner_preference: if estimated_graph_cost_units < context.candidates.len() as u64 {
            "hnsw"
        } else {
            "exact_scan"
        },
        mean_recall_at_k: recall / context.queries.len() as f64,
        complete_result_rate: complete as f64 / context.queries.len() as f64,
        exact_mean_milliseconds: exact_time / context.queries.len() as f64,
        ann_mean_milliseconds: ann_time / context.queries.len() as f64,
    }
}

fn quantify(
    scope: &ScopeId,
    candidates: &[VectorCandidate],
    queries: &[Vec<f32>],
    quantized: &[ScalarQuantizedVector],
    cursor: u64,
    top_k: usize,
) -> QuantizationEvidence {
    let rerank = 64.min(candidates.len());
    let mut error = 0.0;
    let mut samples = 0;
    let mut recall = 0.0;
    for query in queries {
        let exact_request = request(scope, cursor, query, top_k, 100, SearchMode::Exact);
        let exact = search_exact_ref(&exact_request, candidates).unwrap();
        let mut approximate = quantized
            .iter()
            .enumerate()
            .map(|(index, vector)| {
                let score = vector.score(query, ScoreMetric::Cosine).unwrap();
                let VectorValue::Dense { values } = &candidates[index].vector.value else {
                    unreachable!()
                };
                let exact_score = cosine(query, values);
                error += (exact_score - score).abs();
                samples += 1;
                (index, score)
            })
            .collect::<Vec<_>>();
        approximate.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        approximate.truncate(rerank);
        let rerank_candidates = approximate
            .iter()
            .map(|(index, _)| &candidates[*index])
            .collect::<Vec<_>>();
        let reranked = search_exact_ref(&exact_request, rerank_candidates).unwrap();
        recall += recall_at_k(&exact, &reranked);
    }
    let payload_bytes = quantized
        .iter()
        .map(ScalarQuantizedVector::estimated_payload_bytes)
        .sum::<usize>();
    let raw = candidates.len() * queries[0].len() * std::mem::size_of::<f32>();
    QuantizationEvidence {
        payload_bytes,
        raw_payload_ratio: payload_bytes as f64 / raw as f64,
        mean_absolute_score_error: error / samples as f64,
        recall_at_k_after_exact_rerank: recall / queries.len() as f64,
        rerank_candidates: rerank,
    }
}

fn request(
    scope: &ScopeId,
    cursor: u64,
    query: &[f32],
    top_k: usize,
    filter_percent: usize,
    mode: SearchMode,
) -> SearchRequest {
    SearchRequest {
        scope: scope.clone(),
        read: ReadStamp::new(scope.clone(), None, 0, cursor, Some("11".repeat(32))).unwrap(),
        valid_at: 2,
        field: "body".into(),
        query: VectorQuery::Dense {
            values: query.to_vec(),
        },
        metric: ScoreMetric::Cosine,
        embedding_model: None,
        top_k,
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

fn recall_at_k(exact: &[vyrm_vector::SearchHit], actual: &[vyrm_vector::SearchHit]) -> f64 {
    if exact.is_empty() {
        return 1.0;
    }
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

fn candidate(
    scope: &ScopeId,
    index: usize,
    dimensions: usize,
    random: &mut u64,
) -> VectorCandidate {
    let mut properties = RuntimeProperties::new();
    properties.insert(
        "bucket".into(),
        RuntimeValue::Unsigned((index % 100) as u64),
    );
    VectorCandidate {
        scope: scope.clone(),
        source_cursor: index as u64 + 1,
        vector: RuntimeVector {
            reference: RuntimeRef::new("embedding", format!("v-{index:08}")).unwrap(),
            subject: RuntimeRef::new("document", format!("d-{index:08}")).unwrap(),
            field: "body".into(),
            valid_from: 1,
            valid_to: None,
            value: VectorValue::Dense {
                values: unit_vector(dimensions, random),
            },
            provenance: None,
            properties,
        },
    }
}

fn unit_vector(dimensions: usize, random: &mut u64) -> Vec<f32> {
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

fn cosine(left: &[f32], right: &[f32]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| f64::from(*left) * f64::from(*right))
        .sum()
}

fn parse(arguments: &[String], index: usize, default: usize) -> usize {
    arguments
        .get(index)
        .map(|value| value.parse().expect("evidence arguments must be integers"))
        .unwrap_or(default)
}
