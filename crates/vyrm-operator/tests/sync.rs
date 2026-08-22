use std::collections::BTreeSet;
use vyrm_core::{
    digest, ProjectionFamily, ProjectionId, ProjectionStamp, ProjectionState, ProjectionWork,
    ScopeId, DATA_RUNTIME_CONTRACT_VERSION,
};
use vyrm_operator::{
    execute_operator_sync, OperatorAccessPath, OperatorAdapterDescriptor, OperatorKnowledgeBinding,
    OperatorSourceRevision, OperatorSyncWork, ReferenceOperatorWriter,
    OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
};
use vyrm_vector::{EmbeddingModelBinding, ScoreMetric};

fn binding() -> OperatorKnowledgeBinding {
    OperatorKnowledgeBinding {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        project_id: "sync-project".into(),
        member: ".".into(),
        scope: ScopeId::new("instance:sync-project").unwrap(),
        config_digest: "11".repeat(32),
        source_identity_digest: "22".repeat(32),
        relation_digest: "33".repeat(32),
        tenant_digest: "44".repeat(32),
        model: EmbeddingModelBinding {
            name: "sync-model".into(),
            digest: "55".repeat(32),
        },
        dimensions: 3,
        projection: ProjectionStamp {
            contract_version: DATA_RUNTIME_CONTRACT_VERSION,
            id: ProjectionId::new("operator:sync").unwrap(),
            generation: 1,
            source_cursor: 4,
            config_digest: "11".repeat(32),
            artifact_digest: "66".repeat(32),
            state: ProjectionState::Ready,
        },
    }
}

fn descriptor() -> OperatorAdapterDescriptor {
    OperatorAdapterDescriptor {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        implementation_digest: "77".repeat(32),
        max_dimensions: 2_000,
        vector_kinds: BTreeSet::from([vyrm_operator::OperatorVectorKind::Dense]),
        search_capabilities: std::collections::BTreeMap::from([(
            OperatorAccessPath::Exact,
            BTreeSet::from([ScoreMetric::Cosine]),
        )]),
        supports_tenant_filter: true,
        supports_payload_filter: false,
        supports_stable_revision: true,
    }
}

fn revision() -> OperatorSourceRevision {
    OperatorSourceRevision {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        adapter: "pgvector".into(),
        project_id: "sync-project".into(),
        source_identity_digest: "22".repeat(32),
        snapshot_digest: "88".repeat(32),
        catalog_digest: "99".repeat(32),
        stable_revision: Some("revision-4".into()),
        wal_lsn_digest: None,
    }
}

#[test]
fn retry_returns_the_same_external_revision_without_reapplying_payload() {
    let binding = binding();
    let source = ProjectionWork::for_change(
        binding.scope.clone(),
        4,
        "aa".repeat(32),
        3,
        ProjectionFamily::Vector,
    )
    .unwrap();
    let payload = b"caller-owned canonical vector payload";
    let work = OperatorSyncWork::for_vector(
        &binding,
        &source,
        "bb".repeat(32),
        digest::sha256_hex(payload),
    )
    .unwrap();
    let mut writer = ReferenceOperatorWriter::new(descriptor(), &binding, revision()).unwrap();

    let first = execute_operator_sync(&mut writer, &binding, &work, payload).unwrap();
    let replay = execute_operator_sync(&mut writer, &binding, &work, payload).unwrap();
    assert!(first.applied_now);
    assert!(!first.idempotent_replay);
    assert!(!replay.applied_now);
    assert!(replay.idempotent_replay);
    assert_eq!(first.work_id, replay.work_id);
    assert_eq!(first.revision, replay.revision);
    assert_eq!(writer.apply_count(), 1);

    assert!(execute_operator_sync(&mut writer, &binding, &work, b"substituted").is_err());
    assert_eq!(writer.apply_count(), 1);
}
