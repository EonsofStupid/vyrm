#![cfg(feature = "pgvector-postgres")]

use postgres::{Client, NoTls};
use vyrm_core::{
    digest, ProjectionFamily, ProjectionId, ProjectionStamp, ProjectionState, ProjectionWork,
    ReadStamp, ScopeId, DATA_RUNTIME_CONTRACT_VERSION,
};
use vyrm_operator::{
    execute_operator_search, execute_operator_sync, IterativeScanMode, OperatorAccessPath,
    OperatorKnowledgeBinding, OperatorSearchControls, OperatorSearchRequest, OperatorSearchResult,
    OperatorSyncWork, PgvectorDeletePayload, PgvectorDeployment, PgvectorLiveAdapter,
    PgvectorRelation, PgvectorSyncPayload, OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
};
use vyrm_vector::{EmbeddingModelBinding, ScoreMetric, SearchMode, SearchRequest, VectorQuery};

fn scope() -> ScopeId {
    ScopeId::new("instance:pgvector-live-test").unwrap()
}

fn deployment() -> PgvectorDeployment {
    PgvectorDeployment {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        relation: PgvectorRelation {
            schema: "vyrm_live_test".into(),
            relation: "knowledge".into(),
            id_column: "external_id".into(),
            subject_column: "subject_id".into(),
            vector_column: "embedding".into(),
            tenant_column: "tenant_id".into(),
        },
        model_digest_column: "model_digest".into(),
        source_cursor_column: "source_cursor".into(),
        control_schema: "vyrm_live_control".into(),
        metadata_table: "metadata".into(),
        revision_table: "project_revision".into(),
        applied_work_table: "applied_work".into(),
    }
}

fn binding(deployment: &PgvectorDeployment) -> OperatorKnowledgeBinding {
    OperatorKnowledgeBinding {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        project_id: "pgvector-live-project".into(),
        member: ".".into(),
        scope: scope(),
        config_digest: deployment.digest().unwrap(),
        source_identity_digest: "11".repeat(32),
        relation_digest: deployment.relation.digest().unwrap(),
        tenant_digest: digest::sha256_hex(b"tenant-a"),
        model: EmbeddingModelBinding {
            name: "live-fixture-v1".into(),
            digest: "22".repeat(32),
        },
        dimensions: 3,
        projection: ProjectionStamp {
            contract_version: DATA_RUNTIME_CONTRACT_VERSION,
            id: ProjectionId::new("operator:pgvector:live").unwrap(),
            generation: 1,
            source_cursor: 4,
            config_digest: deployment.digest().unwrap(),
            artifact_digest: "33".repeat(32),
            state: ProjectionState::Ready,
        },
    }
}

fn work(
    binding: &OperatorKnowledgeBinding,
    cursor: u64,
    id: &str,
    subject: &str,
    vector: Vec<f32>,
) -> (OperatorSyncWork, Vec<u8>) {
    let payload = PgvectorSyncPayload {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        external_id: id.into(),
        subject_id: subject.into(),
        source_cursor: cursor,
        vector,
    }
    .canonical_bytes(binding.dimensions)
    .unwrap();
    let source = ProjectionWork::for_change(
        binding.scope.clone(),
        cursor,
        format!("{cursor:064x}"),
        cursor,
        ProjectionFamily::Vector,
    )
    .unwrap();
    let work = OperatorSyncWork::for_vector(
        binding,
        &source,
        digest::sha256_hex(format!("change-{cursor}-{id}").as_bytes()),
        digest::sha256_hex(&payload),
    )
    .unwrap();
    (work, payload)
}

fn delete_work(
    binding: &OperatorKnowledgeBinding,
    cursor: u64,
    id: &str,
) -> (OperatorSyncWork, Vec<u8>) {
    let payload = PgvectorDeletePayload {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        external_id: id.into(),
        source_cursor: cursor,
    }
    .canonical_bytes()
    .unwrap();
    let source = ProjectionWork::for_change(
        binding.scope.clone(),
        cursor,
        format!("{cursor:064x}"),
        cursor,
        ProjectionFamily::Vector,
    )
    .unwrap();
    let work = OperatorSyncWork::for_vector_delete(
        binding,
        &source,
        digest::sha256_hex(format!("delete-{cursor}-{id}").as_bytes()),
        digest::sha256_hex(&payload),
    )
    .unwrap();
    (work, payload)
}

fn request(
    binding: &OperatorKnowledgeBinding,
    path: OperatorAccessPath,
    stable_revision: &str,
) -> OperatorSearchRequest {
    let (mode, iterative_scan) = match path {
        OperatorAccessPath::Exact => (SearchMode::Exact, IterativeScanMode::Off),
        OperatorAccessPath::Hnsw | OperatorAccessPath::IvfFlat => (
            SearchMode::RequireApproximate { exact_rerank: 8 },
            match path {
                OperatorAccessPath::Hnsw => IterativeScanMode::StrictOrder,
                OperatorAccessPath::IvfFlat => IterativeScanMode::RelaxedOrder,
                OperatorAccessPath::Exact => unreachable!(),
            },
        ),
    };
    OperatorSearchRequest {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        binding_digest: binding.digest().unwrap(),
        required_source_cursor: binding.projection.source_cursor,
        search: SearchRequest {
            scope: binding.scope.clone(),
            read: ReadStamp::new(
                binding.scope.clone(),
                Some(1),
                1,
                binding.projection.source_cursor,
                Some("44".repeat(32)),
            )
            .unwrap(),
            valid_at: 10,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0, 0.0],
            },
            metric: ScoreMetric::Cosine,
            embedding_model: Some(binding.model.clone()),
            top_k: 3,
            mode,
            filter: None,
        },
        controls: match path {
            OperatorAccessPath::Exact => OperatorSearchControls::exact(),
            OperatorAccessPath::Hnsw => OperatorSearchControls {
                requested_path: path,
                iterative_scan,
                hnsw_ef_search: Some(80),
                hnsw_max_scan_tuples: Some(20_000),
                ivfflat_probes: None,
                ivfflat_max_probes: None,
            },
            OperatorAccessPath::IvfFlat => OperatorSearchControls {
                requested_path: path,
                iterative_scan,
                hnsw_ef_search: None,
                hnsw_max_scan_tuples: None,
                ivfflat_probes: Some(1),
                ivfflat_max_probes: Some(10),
            },
        },
        expected_stable_revision: Some(stable_revision.into()),
    }
}

fn assert_hit_parity(exact: &OperatorSearchResult, approximate: &OperatorSearchResult) {
    assert_eq!(exact.hits.len(), approximate.hits.len());
    for (exact, approximate) in exact.hits.iter().zip(&approximate.hits) {
        assert_eq!(exact.external_id, approximate.external_id);
        assert_eq!(exact.subject_id, approximate.subject_id);
        assert!((exact.score - approximate.score).abs() < 1e-12);
    }
}

#[test]
fn disposable_pgvector_endpoint_proves_snapshot_paths_isolation_retry_and_reopen() {
    let Ok(url) = std::env::var("VYRM_PGVECTOR_TEST_URL") else {
        eprintln!("skipped: VYRM_PGVECTOR_TEST_URL is not configured");
        return;
    };
    assert_eq!(
        std::env::var("VYRM_PGVECTOR_TEST_DISPOSABLE").as_deref(),
        Ok("1"),
        "live test refuses schema deletion without an explicit disposable marker"
    );
    let mut client = Client::connect(&url, NoTls).unwrap();
    let database: String = client
        .query_one("SELECT current_database()", &[])
        .unwrap()
        .get(0);
    assert!(
        database.starts_with("vyrm_operator_test"),
        "live test refuses a database not named vyrm_operator_test*"
    );
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS vyrm_live_control CASCADE;
             DROP SCHEMA IF EXISTS vyrm_live_test CASCADE;
             CREATE EXTENSION IF NOT EXISTS vector;
             CREATE SCHEMA vyrm_live_test;
             CREATE TABLE vyrm_live_test.knowledge (
               tenant_id text NOT NULL,
               external_id text NOT NULL,
               subject_id text NOT NULL,
               embedding vector(3) NOT NULL,
               model_digest text NOT NULL,
               source_cursor bigint NOT NULL,
               PRIMARY KEY (tenant_id, external_id)
             );",
        )
        .unwrap();
    let deployment = deployment();
    let binding = binding(&deployment);
    PgvectorLiveAdapter::install_control_schema(&mut client, &deployment, &binding).unwrap();
    let wrong_tenant = Client::connect(&url, NoTls).unwrap();
    assert!(PgvectorLiveAdapter::from_client(
        wrong_tenant,
        deployment.clone(),
        binding.clone(),
        "tenant-b"
    )
    .is_err());
    let wrong_source = Client::connect(&url, NoTls).unwrap();
    let mut foreign_binding = binding.clone();
    foreign_binding.source_identity_digest = "99".repeat(32);
    assert!(PgvectorLiveAdapter::from_client(
        wrong_source,
        deployment.clone(),
        foreign_binding,
        "tenant-a"
    )
    .is_err());
    let mut adapter =
        PgvectorLiveAdapter::from_client(client, deployment.clone(), binding.clone(), "tenant-a")
            .unwrap();

    let fixtures = [
        (1, "doc-1", "subject-1", vec![1.0, 0.0, 0.0]),
        (2, "doc-2", "subject-2", vec![0.0, 1.0, 0.0]),
        (3, "doc-3", "subject-3", vec![0.5, 0.5, 0.0]),
        (4, "doc-4", "subject-4", vec![0.8, 0.2, 0.0]),
    ];
    let mut first = None;
    for (cursor, id, subject, vector) in fixtures {
        let (work, payload) = work(&binding, cursor, id, subject, vector);
        let receipt = execute_operator_sync(&mut adapter, &binding, &work, &payload).unwrap();
        assert!(receipt.applied_now);
        if cursor == 1 {
            let replay = execute_operator_sync(&mut adapter, &binding, &work, &payload).unwrap();
            assert!(replay.idempotent_replay);
            assert_eq!(receipt.revision, replay.revision);
            first = Some((work, payload, receipt.revision));
        }
    }

    let exact = execute_operator_search(
        &mut adapter,
        &binding,
        &request(&binding, OperatorAccessPath::Exact, "4"),
    )
    .unwrap();
    assert_eq!(exact.plan.selected_path, OperatorAccessPath::Exact);
    assert_eq!(exact.hits[0].external_id, "doc-1");
    assert_eq!(exact.hits.len(), 3);
    assert!(exact.revision.snapshot_digest.len() == 64);
    assert!(exact.revision.catalog_digest.len() == 64);

    let client = adapter.into_client();
    drop(client);
    let mut client = Client::connect(&url, NoTls).unwrap();
    client
        .batch_execute(
            "INSERT INTO vyrm_live_test.knowledge VALUES ('tenant-b', 'foreign', 'foreign', '[1,0,0]', repeat('2', 64), 4);
             INSERT INTO vyrm_live_test.knowledge VALUES ('tenant-a', 'future', 'future', '[1,0,0]', repeat('2', 64), 99);
             CREATE INDEX knowledge_hnsw ON vyrm_live_test.knowledge USING hnsw (embedding vector_cosine_ops);
             ANALYZE vyrm_live_test.knowledge;",
        )
        .unwrap();
    let mut adapter =
        PgvectorLiveAdapter::from_client(client, deployment.clone(), binding.clone(), "tenant-a")
            .unwrap();
    let hnsw = execute_operator_search(
        &mut adapter,
        &binding,
        &request(&binding, OperatorAccessPath::Hnsw, "4"),
    )
    .unwrap();
    assert_eq!(hnsw.plan.selected_path, OperatorAccessPath::Hnsw);
    assert!(hnsw.hits.iter().all(|hit| hit.external_id != "foreign"));
    assert!(hnsw.hits.iter().all(|hit| hit.external_id != "future"));
    assert_hit_parity(&exact, &hnsw);

    let mut client = adapter.into_client();
    client
        .batch_execute(
            "DROP INDEX vyrm_live_test.knowledge_hnsw;
             CREATE INDEX knowledge_ivfflat ON vyrm_live_test.knowledge USING ivfflat (embedding vector_cosine_ops) WITH (lists = 1);
             ANALYZE vyrm_live_test.knowledge;",
        )
        .unwrap();
    let mut adapter =
        PgvectorLiveAdapter::from_client(client, deployment.clone(), binding.clone(), "tenant-a")
            .unwrap();
    let ivfflat = execute_operator_search(
        &mut adapter,
        &binding,
        &request(&binding, OperatorAccessPath::IvfFlat, "4"),
    )
    .unwrap();
    assert_eq!(ivfflat.plan.selected_path, OperatorAccessPath::IvfFlat);
    assert_hit_parity(&exact, &ivfflat);

    let (first_work, first_payload, first_revision) = first.unwrap();
    let replay =
        execute_operator_sync(&mut adapter, &binding, &first_work, &first_payload).unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(replay.revision, first_revision);

    let (update_work, update_payload) = work(
        &binding,
        5,
        "doc-2",
        "subject-2-updated",
        vec![0.9, 0.1, 0.0],
    );
    assert!(
        execute_operator_sync(&mut adapter, &binding, &update_work, &update_payload)
            .unwrap()
            .applied_now
    );
    let (older_work, older_payload) =
        work(&binding, 2, "doc-2", "stale-subject", vec![0.0, 0.0, 1.0]);
    assert!(
        execute_operator_sync(&mut adapter, &binding, &older_work, &older_payload)
            .unwrap_err()
            .to_string()
            .contains("older than the external row")
    );
    let (delete_work, delete_payload) = delete_work(&binding, 6, "doc-3");
    assert!(
        execute_operator_sync(&mut adapter, &binding, &delete_work, &delete_payload)
            .unwrap()
            .applied_now
    );

    drop(adapter.into_client());
    let client = Client::connect(&url, NoTls).unwrap();
    let mut current_binding = binding.clone();
    current_binding.projection.generation = 2;
    current_binding.projection.source_cursor = 6;
    current_binding.projection.artifact_digest = "55".repeat(32);
    let mut adapter =
        PgvectorLiveAdapter::from_client(client, deployment, current_binding.clone(), "tenant-a")
            .unwrap();
    let current = execute_operator_search(
        &mut adapter,
        &current_binding,
        &request(&current_binding, OperatorAccessPath::Exact, "6"),
    )
    .unwrap();
    assert!(current.hits.iter().all(|hit| hit.external_id != "doc-3"));
    assert_eq!(current.hits[1].external_id, "doc-2");
    assert_eq!(current.hits[1].subject_id, "subject-2-updated");

    let stale = request(&current_binding, OperatorAccessPath::Exact, "5");
    assert!(
        execute_operator_search(&mut adapter, &current_binding, &stale)
            .unwrap_err()
            .to_string()
            .contains("stale")
    );
}
