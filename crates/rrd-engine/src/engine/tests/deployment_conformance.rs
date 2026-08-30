use super::*;
use std::collections::BTreeMap;

use rrd_contract::{
    CommitTransaction, DataCatalogueIdentity, DataLogicalModel, DataPropertySchema,
    DataRecordSchema, DataReference, DataSchemaMode, DataSchemaRegistry, DataTableSchema,
    DataValueType, DeploymentConformanceCorpus, ExecuteQuery, QueryBudget, QueryValue,
};

fn corpus() -> DeploymentConformanceCorpus {
    let corpus = serde_json::from_str(include_str!(
        "../../../../../fixtures/rrd-deployment-conformance-v1.json"
    ))
    .unwrap();
    DeploymentConformanceCorpus::validate(&corpus).unwrap();
    corpus
}

fn run_corpus(engine: &RrdEngine, suffix: &str) -> Vec<CanonicalId> {
    let corpus = corpus();
    let lease = engine
        .create_session(
            &session_request(5_000, 2),
            &id(&format!("{suffix}-session-key")),
            1_000,
            &format!("request-{suffix}-session"),
            &format!("operation-{suffix}-session"),
        )
        .unwrap();
    let transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &BeginTransaction {
                scope: CanonicalId::new("data").unwrap(),
                timeout_ms: 1_000,
            },
            &mutation_context(
                &id(&format!("{suffix}-begin-key")),
                &format!("request-{suffix}-begin"),
                &format!("operation-{suffix}-begin"),
            ),
            1_100,
        )
        .unwrap();
    let table = CanonicalId::new("document").unwrap();
    let mut mutations = vec![TransactionMutation::PutSchema {
        registry: DataSchemaRegistry {
            revision: 1,
            migration: "install deployment conformance corpus".into(),
            catalogue: DataCatalogueIdentity::default(),
            tables: BTreeMap::from([(
                table.clone(),
                DataTableSchema {
                    model: DataLogicalModel::Document,
                    mode: DataSchemaMode::Strict,
                    properties: BTreeMap::new(),
                    allow_additional_properties: false,
                },
            )]),
            records: BTreeMap::from([(
                table.clone(),
                DataRecordSchema {
                    properties: BTreeMap::from([(
                        "body".into(),
                        DataPropertySchema {
                            value_type: DataValueType::String,
                            required: true,
                        },
                    )]),
                    allow_additional_properties: false,
                    unique_properties: Default::default(),
                },
            )]),
            relations: BTreeMap::new(),
            events: BTreeMap::new(),
        },
    }];
    mutations.extend(
        corpus
            .documents
            .iter()
            .map(|document| TransactionMutation::PutRecord {
                reference: DataReference {
                    kind: table.clone(),
                    id: document.id.clone(),
                },
                valid_from: corpus.query.valid_at,
                valid_to: None,
                properties: BTreeMap::from([(
                    "body".into(),
                    QueryValue::String(document.text.clone()),
                )]),
            }),
    );
    let commit = CommitTransaction {
        operation_sha256: rrd_contract::transaction_operation_sha256(&mutations),
        mutations,
    };
    let receipt = engine
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id(&format!("{suffix}-commit-key")),
            &commit,
            1_200,
            &format!("request-{suffix}-commit"),
            &format!("operation-{suffix}-commit"),
        )
        .unwrap();
    assert_eq!(
        receipt.last_runtime_cursor,
        Some(corpus.documents.len() as u64 + 1)
    );
    let result = engine
        .execute_query(
            &lease.session_id,
            &lease.token,
            &ExecuteQuery {
                scope: format!("instance:{}", instance()),
                query: corpus.query.rrflowql,
                parameters: BTreeMap::new(),
                budget: QueryBudget::default(),
            },
            1_300,
            &format!("request-{suffix}-query"),
            &format!("operation-{suffix}-query"),
        )
        .unwrap();
    assert!(result.plan.exact);
    assert_eq!(
        result.rows[0].values["body"],
        QueryValue::String(corpus.documents[0].text.clone())
    );
    result
        .rows
        .iter()
        .map(|row| {
            CanonicalId::new(
                row.identity
                    .strip_prefix("record:document:")
                    .expect("corpus row has document identity"),
            )
            .unwrap()
        })
        .collect()
}

#[test]
fn memory_and_embedded_composition_roots_pass_one_logical_corpus() {
    let corpus = corpus();
    let memory = RrdEngine::memory(instance(), TOKEN_KEY);
    assert!(!memory.has_persistent_root());
    assert_eq!(
        memory.deployment_mode(),
        rrd_contract::DeploymentMode::Memory
    );
    assert_eq!(memory.readiness(1).unwrap().backend.as_str(), "memory");
    assert_eq!(run_corpus(&memory, "memory"), corpus.expected_ids);

    let root = tempfile::tempdir().unwrap();
    let embedded = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    assert!(embedded.has_persistent_root());
    assert_eq!(
        embedded.deployment_mode(),
        rrd_contract::DeploymentMode::Embedded
    );
    assert_eq!(embedded.readiness(1).unwrap().backend.as_str(), "rrd_lsm");
    assert!(RrdEngine::open(root.path(), instance(), TOKEN_KEY).is_err());
    assert_eq!(run_corpus(&embedded, "embedded"), corpus.expected_ids);
    drop(embedded);

    let reopened = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    assert_eq!(reopened.readiness(1).unwrap().runtime_cursor, 3);
}

#[test]
fn memory_mode_rejects_durability_only_operations_before_preparing_state() {
    let engine = RrdEngine::memory(instance(), TOKEN_KEY);
    let lease = engine
        .create_session(
            &session_request(5_000, 1),
            &id("memory-backup-session"),
            1_000,
            "request-memory-backup-session",
            "operation-memory-backup-session",
        )
        .unwrap();
    let before = engine.storage.control_sequence().unwrap();
    let result = engine.create_instance_backup(
        &lease.session_id,
        &lease.token,
        &id("memory-backup-key"),
        &rrd_contract::CreateInstanceBackup {
            label: "memory-backup".into(),
            created_at_unix_ms: 1_100,
        },
        1_100,
        "request-memory-backup",
        "operation-memory-backup",
    );
    assert!(matches!(result, Err(ServiceError::Backup(_))));
    assert_eq!(engine.storage.control_sequence().unwrap(), before);
}
