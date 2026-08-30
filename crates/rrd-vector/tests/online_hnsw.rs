use rrd_core::{
    ProjectionId, ReadStamp, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector, ScopeId,
    VectorCollectionAddress, VectorValue,
};
use rrd_vector::{
    search_exact_ref, FilterCondition, FilterExpression, FilterOperator, HnswConfig, HnswIndex,
    HnswMaintenanceKind, ScoreMetric, SearchMode, SearchRequest, VectorCandidate, VectorQuery,
    VectorRuntime,
};
use std::collections::BTreeSet;

#[test]
fn immutable_generations_append_only_new_versions_and_reopen_exactly() {
    let scope = ScopeId::new("instance:online-hnsw-incremental").unwrap();
    let config = config(&scope, ScoreMetric::Dot);
    let base = (0..32)
        .map(|id| candidate(&scope, id as u64 + 1, id, unit(id, 32), None))
        .collect::<Vec<_>>();
    let first = HnswIndex::build(config.clone(), 1, 32, base.clone()).unwrap();

    let updated = candidate(&scope, 33, 0, vec![-2.0, 0.0], None);
    let inserted = candidate(&scope, 34, 32, vec![0.0, 2.0], None);
    let second = first
        .advance(2, 34, [updated.clone(), inserted.clone()])
        .unwrap();
    assert_eq!(
        second.descriptor().maintenance,
        HnswMaintenanceKind::Incremental
    );
    assert_eq!(second.descriptor().previous_generation, Some(1));
    assert_eq!(second.descriptor().indexed_delta_vectors, 2);
    assert_eq!(second.descriptor().nodes, first.descriptor().nodes + 2);

    let reopened = HnswIndex::from_bytes(second.as_bytes()).unwrap();
    assert_eq!(reopened.descriptor(), second.descriptor());
    assert_eq!(reopened.as_bytes(), second.as_bytes());
    let mut history = base;
    history.extend([updated, inserted]);
    let request = request(
        &scope,
        34,
        2,
        vec![-1.0, 0.0],
        ScoreMetric::Dot,
        SearchMode::RequireApproximate { exact_rerank: 34 },
        None,
        5,
    );
    let expected = search_exact_ref(&request, &history).unwrap();
    assert_eq!(reopened.search(&request, 34).unwrap(), expected);
}

#[test]
fn authoritative_delta_overlay_makes_inserts_and_retirements_immediately_searchable() {
    let scope = ScopeId::new("instance:online-hnsw-overlay").unwrap();
    let base = (0..16)
        .map(|id| candidate(&scope, id as u64 + 1, id, unit(id, 16), None))
        .collect::<Vec<_>>();
    let graph = HnswIndex::build(config(&scope, ScoreMetric::Dot), 1, 16, base.clone()).unwrap();
    let mut retired = base[0].clone();
    retired.source_cursor = 17;
    retired.vector.valid_to = Some(2);
    let inserted = candidate(&scope, 18, 16, vec![2.0, 0.0], None);
    let mut canonical = base;
    canonical.extend([retired.clone(), inserted.clone()]);

    let mut runtime = VectorRuntime::new(canonical.clone()).unwrap();
    runtime.publish(0, graph.clone()).unwrap();
    let request = request(
        &scope,
        18,
        2,
        vec![1.0, 0.0],
        ScoreMetric::Dot,
        SearchMode::RequireApproximate { exact_rerank: 16 },
        None,
        1,
    );
    let prepared = runtime.prepare_search_at(&request, 18, 16).unwrap();
    assert_eq!(
        prepared.plan().selected.kind,
        rrd_vector::AccessPathKind::Hnsw
    );
    assert_eq!(prepared.plan().selected.source_cursor, 16);
    assert_eq!(prepared.plan().selected.overlay_source_cursor, Some(18));
    assert_eq!(prepared.plan().selected.overlay_candidates, 2);
    let online = runtime.execute_search(&request, &prepared).unwrap();
    assert_eq!(online.hits[0].reference.id.as_str(), "v-016");

    let reopened_graph = HnswIndex::from_bytes(graph.as_bytes()).unwrap();
    let mut reopened = VectorRuntime::new(canonical).unwrap();
    reopened.publish(0, reopened_graph.clone()).unwrap();
    assert_eq!(reopened.search(&request, 16).unwrap(), online);

    let advanced = reopened_graph.advance(2, 18, [retired, inserted]).unwrap();
    reopened.publish(1, advanced).unwrap();
    let maintained = reopened.prepare_search_at(&request, 18, 16).unwrap();
    assert_eq!(maintained.plan().selected.overlay_source_cursor, None);
    assert_eq!(
        reopened.execute_search(&request, &maintained).unwrap().hits,
        online.hits
    );
}

#[test]
fn nested_filter_operators_participate_in_graph_traversal_without_gaps() {
    let scope = ScopeId::new("instance:online-hnsw-filter-algebra").unwrap();
    let candidates = (0..64)
        .map(|id| candidate(&scope, id as u64 + 1, id, unit(id, 64), None))
        .collect::<Vec<_>>();
    let mut config = config(&scope, ScoreMetric::Cosine);
    config.filter_properties = BTreeSet::from([
        "active".into(),
        "bucket".into(),
        "group".into(),
        "optional".into(),
    ]);
    let graph = HnswIndex::build(config, 1, 64, candidates.clone()).unwrap();
    let filter = FilterExpression::All {
        filters: vec![
            condition(
                "active",
                FilterOperator::Equals {
                    value: RuntimeValue::Bool(true),
                },
            ),
            condition(
                "bucket",
                FilterOperator::Range {
                    gt: None,
                    gte: Some(RuntimeValue::Unsigned(8)),
                    lt: Some(RuntimeValue::Unsigned(48)),
                    lte: None,
                },
            ),
            FilterExpression::Any {
                filters: vec![
                    condition(
                        "group",
                        FilterOperator::In {
                            values: vec![RuntimeValue::String("a".into())],
                        },
                    ),
                    condition("optional", FilterOperator::Exists { value: true }),
                ],
            },
            FilterExpression::Not {
                filter: Box::new(condition(
                    "group",
                    FilterOperator::NotEquals {
                        value: RuntimeValue::String("a".into()),
                    },
                )),
            },
        ],
    };
    let approximate = request(
        &scope,
        64,
        2,
        vec![1.0, 0.0],
        ScoreMetric::Cosine,
        SearchMode::RequireApproximate { exact_rerank: 64 },
        Some(filter.clone()),
        10,
    );
    let exact = request(
        &scope,
        64,
        2,
        vec![1.0, 0.0],
        ScoreMetric::Cosine,
        SearchMode::Exact,
        Some(filter),
        10,
    );
    assert_eq!(
        graph.search(&approximate, 64).unwrap(),
        search_exact_ref(&exact, &candidates).unwrap()
    );
}

fn config(scope: &ScopeId, metric: ScoreMetric) -> HnswConfig {
    HnswConfig {
        id: ProjectionId::new("hnsw-documents-body").unwrap(),
        scope: scope.clone(),
        field: "body-embedding".into(),
        dimensions: 2,
        metric,
        embedding_model: None,
        m: 8,
        ef_construction: 32,
        max_level: 8,
        seed: 41,
        filter_properties: BTreeSet::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn request(
    scope: &ScopeId,
    cursor: u64,
    valid_at: u64,
    query: Vec<f32>,
    metric: ScoreMetric,
    mode: SearchMode,
    filter: Option<FilterExpression>,
    top_k: usize,
) -> SearchRequest {
    SearchRequest {
        scope: scope.clone(),
        read: ReadStamp::new(scope.clone(), None, 0, cursor, Some("44".repeat(32))).unwrap(),
        valid_at,
        field: "body-embedding".into(),
        query: VectorQuery::Dense { values: query },
        metric,
        embedding_model: None,
        top_k,
        mode,
        filter,
    }
}

fn candidate(
    scope: &ScopeId,
    cursor: u64,
    id: usize,
    values: Vec<f32>,
    valid_to: Option<u64>,
) -> VectorCandidate {
    let mut properties = RuntimeProperties::new();
    properties.insert("active".into(), RuntimeValue::Bool(id.is_multiple_of(2)));
    properties.insert("bucket".into(), RuntimeValue::Unsigned(id as u64));
    properties.insert(
        "group".into(),
        RuntimeValue::String(if id.is_multiple_of(4) { "a" } else { "b" }.into()),
    );
    if id.is_multiple_of(8) {
        properties.insert("optional".into(), RuntimeValue::Bool(true));
    }
    VectorCandidate {
        scope: scope.clone(),
        source_cursor: cursor,
        vector: RuntimeVector {
            reference: RuntimeRef::new("embedding", format!("v-{id:03}")).unwrap(),
            subject: RuntimeRef::new("document", format!("d-{id:03}")).unwrap(),
            collection: Some(VectorCollectionAddress {
                collection_id: "documents".into(),
                vector_name: "body".into(),
            }),
            field: "body-embedding".into(),
            valid_from: 1,
            valid_to,
            value: VectorValue::Dense { values },
            provenance: None,
            properties,
        },
    }
}

fn condition(property: &str, operator: FilterOperator) -> FilterExpression {
    FilterExpression::Condition {
        condition: FilterCondition {
            property: property.into(),
            operator,
        },
    }
}

fn unit(id: usize, count: usize) -> Vec<f32> {
    let angle = id as f32 * std::f32::consts::TAU / count as f32;
    vec![angle.cos(), angle.sin()]
}
