use std::collections::BTreeSet;
use vyrm_core::{
    ProjectionId, ProjectionStamp, ProjectionState, ReadStamp, ScopeId,
    DATA_RUNTIME_CONTRACT_VERSION,
};
use vyrm_operator::{
    IterativeScanMode, OperatorAccessPath, OperatorAdapterDescriptor, OperatorKnowledgeBinding,
    OperatorSearchControls, OperatorSearchRequest, OperatorSourceRevision,
    OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
};
use vyrm_vector::{EmbeddingModelBinding, ScoreMetric, SearchMode, SearchRequest, VectorQuery};

fn vector() -> serde_json::Value {
    let scope = ScopeId::new("instance:operator-golden").unwrap();
    let model = EmbeddingModelBinding {
        name: "fixture-model".into(),
        digest: "11".repeat(32),
    };
    let binding = OperatorKnowledgeBinding {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        project_id: "operator-golden".into(),
        member: ".".into(),
        scope: scope.clone(),
        config_digest: "22".repeat(32),
        source_identity_digest: "33".repeat(32),
        relation_digest: "44".repeat(32),
        tenant_digest: "55".repeat(32),
        model: model.clone(),
        dimensions: 3,
        projection: ProjectionStamp {
            contract_version: DATA_RUNTIME_CONTRACT_VERSION,
            id: ProjectionId::new("operator:pgvector:fixture").unwrap(),
            generation: 2,
            source_cursor: 9,
            config_digest: "22".repeat(32),
            artifact_digest: "66".repeat(32),
            state: ProjectionState::Ready,
        },
    };
    let descriptor = OperatorAdapterDescriptor {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        implementation_digest: "77".repeat(32),
        max_dimensions: 2_000,
        vector_kinds: BTreeSet::from([vyrm_operator::OperatorVectorKind::Dense]),
        search_capabilities: std::collections::BTreeMap::from([
            (
                OperatorAccessPath::Exact,
                BTreeSet::from([
                    ScoreMetric::Cosine,
                    ScoreMetric::Dot,
                    ScoreMetric::Euclidean,
                    ScoreMetric::Manhattan,
                ]),
            ),
            (
                OperatorAccessPath::Hnsw,
                BTreeSet::from([
                    ScoreMetric::Cosine,
                    ScoreMetric::Dot,
                    ScoreMetric::Euclidean,
                    ScoreMetric::Manhattan,
                ]),
            ),
            (
                OperatorAccessPath::IvfFlat,
                BTreeSet::from([
                    ScoreMetric::Cosine,
                    ScoreMetric::Dot,
                    ScoreMetric::Euclidean,
                ]),
            ),
        ]),
        supports_tenant_filter: true,
        supports_payload_filter: false,
        supports_stable_revision: true,
    };
    let request = OperatorSearchRequest {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        binding_digest: binding.digest().unwrap(),
        required_source_cursor: 9,
        search: SearchRequest {
            scope: scope.clone(),
            read: ReadStamp::new(scope, Some(4), 4, 9, Some("88".repeat(32))).unwrap(),
            valid_at: 1_000,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0, 0.5],
            },
            metric: ScoreMetric::Cosine,
            embedding_model: Some(model),
            top_k: 10,
            mode: SearchMode::RequireApproximate { exact_rerank: 20 },
            filter: None,
        },
        controls: OperatorSearchControls {
            requested_path: OperatorAccessPath::Hnsw,
            iterative_scan: IterativeScanMode::StrictOrder,
            hnsw_ef_search: Some(80),
            hnsw_max_scan_tuples: Some(20_000),
            ivfflat_probes: None,
            ivfflat_max_probes: None,
        },
        expected_stable_revision: Some("project-revision-9".into()),
    };
    let revision = OperatorSourceRevision {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        project_id: "operator-golden".into(),
        source_identity_digest: "33".repeat(32),
        snapshot_digest: "99".repeat(32),
        catalog_digest: "aa".repeat(32),
        stable_revision: Some("project-revision-9".into()),
        wal_lsn_digest: Some("bb".repeat(32)),
    };
    serde_json::json!({
        "descriptor": descriptor,
        "binding": binding,
        "request": request,
        "revision": revision,
    })
}

#[test]
fn operator_knowledge_wire_contract_matches_checked_in_fixture() {
    let encoded = serde_json::to_string_pretty(&vector()).unwrap() + "\n";
    assert_eq!(
        encoded,
        include_str!("../fixtures/operator-knowledge-v1.json")
    );
}
