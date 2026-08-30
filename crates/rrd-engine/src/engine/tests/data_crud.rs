use super::*;
use std::collections::BTreeMap;

use rrd_contract::{
    DataLogicalModel, DataReference, DataSchemaMode, DataSchemaRegistry, DataTableSchema,
    DataTarget, QueryValue, ReadDataSnapshot,
};

fn data_ref(kind: &str, value: &str) -> DataReference {
    DataReference {
        kind: CanonicalId::new(kind).unwrap(),
        id: CanonicalId::new(value).unwrap(),
    }
}

fn event_target(kind: &str, cursor: u64) -> DataTarget {
    DataTarget::Event {
        kind: CanonicalId::new(kind).unwrap(),
        cursor,
    }
}

fn commit_data(
    engine: &RrdEngine,
    lease: &rrd_contract::SessionLease,
    mutations: Vec<TransactionMutation>,
    at: u64,
    suffix: &str,
) -> rrd_contract::CommitReceipt {
    let transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &BeginTransaction {
                scope: CanonicalId::new("data").unwrap(),
                timeout_ms: 1_000,
            },
            &mutation_context(
                &id(&format!("begin-key-{suffix}")),
                &format!("request-begin-{suffix}"),
                &format!("operation-begin-{suffix}"),
            ),
            at,
        )
        .unwrap();
    let request = CommitTransaction {
        operation_sha256: rrd_contract::transaction_operation_sha256(&mutations),
        mutations,
    };
    engine
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id(&format!("commit-key-{suffix}")),
            &request,
            at,
            &format!("request-commit-{suffix}"),
            &format!("operation-commit-{suffix}"),
        )
        .unwrap()
}

fn committed_audit(
    engine: &RrdEngine,
    receipt: &rrd_contract::CommitReceipt,
) -> (String, rrd_core::AuditEnvelope) {
    let commit_id = receipt.runtime_commit_sha256.clone().unwrap();
    let audit = engine.storage.runtime_audit(&commit_id).unwrap().unwrap();
    audit.validate().unwrap();
    assert_eq!(
        audit.read.as_ref().unwrap().commit_cursor + 1,
        receipt.first_runtime_cursor.unwrap()
    );
    assert_eq!(audit.outcome_cursor, receipt.last_runtime_cursor);
    (commit_id, audit)
}

#[test]
fn public_engine_reads_updates_event_corrections_and_retirements_at_one_stamp() {
    let (root, engine) = isolated_engine();
    let lease = engine
        .create_session(
            &session_request(5_000, 4),
            &id("data-crud-session"),
            1_000,
            "request-data-crud-session",
            "operation-data-crud-session",
        )
        .unwrap();
    let document = data_ref("document", "alpha");
    let initial = vec![
        TransactionMutation::PutSchema {
            registry: DataSchemaRegistry {
                revision: 1,
                migration: "install CRUD fixture".into(),
                catalogue: DataCatalogueIdentity::default(),
                tables: BTreeMap::from([
                    (
                        document.kind.clone(),
                        DataTableSchema {
                            model: DataLogicalModel::Document,
                            mode: DataSchemaMode::Schemaless,
                            properties: BTreeMap::new(),
                            allow_additional_properties: false,
                        },
                    ),
                    (
                        CanonicalId::new("observed").unwrap(),
                        DataTableSchema {
                            model: DataLogicalModel::Event,
                            mode: DataSchemaMode::Schemaless,
                            properties: BTreeMap::new(),
                            allow_additional_properties: false,
                        },
                    ),
                ]),
                records: BTreeMap::new(),
                relations: BTreeMap::new(),
                events: BTreeMap::new(),
            },
        },
        TransactionMutation::PutRecord {
            reference: document.clone(),
            valid_from: 1_100,
            valid_to: None,
            properties: BTreeMap::from([("revision".into(), QueryValue::Unsigned(1))]),
        },
        TransactionMutation::AppendEvent {
            kind: CanonicalId::new("observed").unwrap(),
            subject: Some(document.clone()),
            properties: BTreeMap::from([("state".into(), QueryValue::String("created".into()))]),
        },
    ];
    let initial_receipt = commit_data(&engine, &lease, initial, 1_100, "initial");
    committed_audit(&engine, &initial_receipt);

    let first = engine
        .read_data_snapshot(
            &lease.session_id,
            &lease.token,
            &ReadDataSnapshot {
                valid_at: 1_100,
                max_scanned_changes: 64,
            },
            1_101,
            "request-data-read-first",
            "operation-data-read-first",
        )
        .unwrap();
    assert_eq!(first.known_at_cursor, 3);
    assert_eq!(first.schema_revision, 1);
    assert_eq!(first.entries.len(), 2);
    assert!(first
        .entries
        .iter()
        .any(|entry| entry.target == event_target("observed", 3)));

    let update = vec![
        TransactionMutation::PutRecord {
            reference: document.clone(),
            valid_from: 1_200,
            valid_to: None,
            properties: BTreeMap::from([("revision".into(), QueryValue::Unsigned(2))]),
        },
        TransactionMutation::RetireData {
            model: DataLogicalModel::Event,
            target: event_target("observed", 3),
            effective_at: 1_200,
        },
        TransactionMutation::AppendEvent {
            kind: CanonicalId::new("observed").unwrap(),
            subject: Some(document.clone()),
            properties: BTreeMap::from([("state".into(), QueryValue::String("corrected".into()))]),
        },
    ];
    let update_receipt = commit_data(&engine, &lease, update, 1_200, "update");
    committed_audit(&engine, &update_receipt);
    let updated = engine
        .read_data_snapshot(
            &lease.session_id,
            &lease.token,
            &ReadDataSnapshot {
                valid_at: 1_250,
                max_scanned_changes: 64,
            },
            1_250,
            "request-data-read-update",
            "operation-data-read-update",
        )
        .unwrap();
    assert_eq!(updated.known_at_cursor, 6);
    assert_eq!(updated.entries.len(), 2);
    assert!(updated
        .entries
        .iter()
        .any(|entry| entry.target == event_target("observed", 6)));

    let retirement_receipt = commit_data(
        &engine,
        &lease,
        vec![
            TransactionMutation::RetireData {
                model: DataLogicalModel::Document,
                target: DataTarget::Reference {
                    reference: document,
                },
                effective_at: 1_300,
            },
            TransactionMutation::RetireData {
                model: DataLogicalModel::Event,
                target: event_target("observed", 6),
                effective_at: 1_300,
            },
        ],
        1_300,
        "retire",
    );
    let (retirement_commit_id, retirement_audit) = committed_audit(&engine, &retirement_receipt);
    let retired = engine
        .read_data_snapshot(
            &lease.session_id,
            &lease.token,
            &ReadDataSnapshot {
                valid_at: 1_300,
                max_scanned_changes: 64,
            },
            1_301,
            "request-data-read-retired",
            "operation-data-read-retired",
        )
        .unwrap();
    assert!(retired.entries.is_empty());

    drop(engine);
    let reopened = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    assert_eq!(
        reopened
            .storage
            .runtime_audit(&retirement_commit_id)
            .unwrap(),
        Some(retirement_audit)
    );
    let reopened_snapshot = reopened
        .read_data_snapshot(
            &lease.session_id,
            &lease.token,
            &ReadDataSnapshot {
                valid_at: 1_300,
                max_scanned_changes: 64,
            },
            1_302,
            "request-data-read-reopen",
            "operation-data-read-reopen",
        )
        .unwrap();
    assert_eq!(reopened_snapshot, retired);
}
