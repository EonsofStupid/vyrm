#![cfg(feature = "pgvector-postgres")]

use vyrm_operator::{
    PgvectorDeletePayload, PgvectorDeployment, PgvectorRelation, PgvectorSyncPayload,
    OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
};

#[test]
fn live_pgvector_deployment_and_sync_wire_contract_match_checked_in_fixture() {
    let deployment = PgvectorDeployment {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        relation: PgvectorRelation {
            schema: "operator_data".into(),
            relation: "knowledge".into(),
            id_column: "external_id".into(),
            subject_column: "subject_id".into(),
            vector_column: "embedding".into(),
            tenant_column: "tenant_id".into(),
        },
        model_digest_column: "model_digest".into(),
        source_cursor_column: "source_cursor".into(),
        control_schema: "vyrm_control".into(),
        metadata_table: "metadata".into(),
        revision_table: "project_revision".into(),
        applied_work_table: "applied_work".into(),
    };
    let sync_payload = PgvectorSyncPayload {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        external_id: "document:42:body".into(),
        subject_id: "document:42".into(),
        source_cursor: 17,
        vector: vec![0.25, -0.5, 0.75],
    };
    let delete_payload = PgvectorDeletePayload {
        contract_version: OPERATOR_KNOWLEDGE_CONTRACT_VERSION,
        external_id: "document:42:body".into(),
        source_cursor: 18,
    };
    let encoded = serde_json::to_string_pretty(&serde_json::json!({
        "delete_payload": delete_payload,
        "deployment": deployment,
        "sync_payload": sync_payload,
    }))
    .unwrap()
        + "\n";
    assert_eq!(encoded, include_str!("../fixtures/pgvector-live-v1.json"));
}
