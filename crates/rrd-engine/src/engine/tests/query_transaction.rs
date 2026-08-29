use super::*;
use std::collections::BTreeMap;

use rrd_contract::{
    DataLogicalModel, DataReference, DataSchemaMode, DataSchemaRegistry, DataTableSchema,
    DataTarget, ExecuteQueryTransaction, QueryValue, ReadDataSnapshot,
};

fn reference(kind: &str, value: &str) -> DataReference {
    DataReference {
        kind: CanonicalId::new(kind).unwrap(),
        id: CanonicalId::new(value).unwrap(),
    }
}

fn program_request(
    program: &str,
    mutation_bindings: BTreeMap<String, TransactionMutation>,
) -> ExecuteQueryTransaction {
    ExecuteQueryTransaction {
        scope: format!("instance:{}", instance()),
        program: program.into(),
        mutation_bindings,
        timeout_ms: 1_000,
    }
}

fn snapshot(
    engine: &RrdEngine,
    lease: &rrd_contract::SessionLease,
    valid_at: u64,
    suffix: &str,
) -> rrd_contract::DataSnapshot {
    engine
        .read_data_snapshot(
            &lease.session_id,
            &lease.token,
            &ReadDataSnapshot {
                valid_at,
                max_scanned_changes: 128,
            },
            valid_at,
            &format!("request-query-transaction-read-{suffix}"),
            &format!("operation-query-transaction-read-{suffix}"),
        )
        .unwrap()
}

#[test]
fn query_transaction_commit_cancel_failure_replay_and_reopen_share_one_authority() {
    let (root, engine) = isolated_engine();
    let lease = engine
        .create_session(
            &session_request(8_000, 8),
            &id("query-transaction-session"),
            1_000,
            "request-query-transaction-session",
            "operation-query-transaction-session",
        )
        .unwrap();
    let document = reference("document", "alpha");
    let schema = TransactionMutation::PutSchema {
        registry: DataSchemaRegistry {
            revision: 1,
            migration: "install query transaction schema".into(),
            catalogue: DataCatalogueIdentity::default(),
            tables: BTreeMap::from([(
                document.kind.clone(),
                DataTableSchema {
                    model: DataLogicalModel::Document,
                    mode: DataSchemaMode::Schemaless,
                    properties: BTreeMap::new(),
                    allow_additional_properties: false,
                },
            )]),
            records: BTreeMap::new(),
            relations: BTreeMap::new(),
            events: BTreeMap::new(),
        },
    };
    let create = TransactionMutation::PutRecord {
        reference: document.clone(),
        valid_from: 1_100,
        valid_to: None,
        properties: BTreeMap::from([("revision".into(), QueryValue::Unsigned(1))]),
    };
    let request = program_request(
        "BEGIN; MUTATE $schema; MUTATE $create; COMMIT;",
        BTreeMap::from([("create".into(), create), ("schema".into(), schema)]),
    );
    let context = mutation_context(
        &id("query-program-create"),
        "request-query-program-create",
        "operation-query-program-create",
    );
    let committed = engine
        .execute_query_transaction(&lease.session_id, &lease.token, &request, &context, 1_100)
        .unwrap();
    assert_eq!(committed.state, TransactionState::Committed);
    assert_eq!(committed.mutation_count, 2);
    assert_eq!(committed.read_cursor, 0);
    let receipt = committed.commit.as_ref().unwrap();
    assert_eq!(receipt.first_runtime_cursor, Some(1));
    assert_eq!(receipt.last_runtime_cursor, Some(2));
    let audit = engine
        .storage
        .runtime_audit(receipt.runtime_commit_sha256.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(audit.read.as_ref().unwrap().commit_cursor, 0);
    assert_eq!(audit.outcome_cursor, Some(2));

    let replay = engine
        .execute_query_transaction(&lease.session_id, &lease.token, &request, &context, 1_101)
        .unwrap();
    assert_eq!(replay.transaction_id, committed.transaction_id);
    assert!(replay.commit.as_ref().unwrap().idempotent_replay);
    assert_eq!(snapshot(&engine, &lease, 1_100, "created").entries.len(), 1);
    let changed_under_same_key = program_request(
        "BEGIN; MUTATE $create; MUTATE $schema; COMMIT;",
        request.mutation_bindings.clone(),
    );
    assert!(matches!(
        engine.execute_query_transaction(
            &lease.session_id,
            &lease.token,
            &changed_under_same_key,
            &context,
            1_102,
        ),
        Err(ServiceError::IdempotencyConflict)
    ));

    let cancel = program_request(
        "BEGIN; MUTATE $update; CANCEL;",
        BTreeMap::from([(
            "update".into(),
            TransactionMutation::PutRecord {
                reference: document.clone(),
                valid_from: 1_200,
                valid_to: None,
                properties: BTreeMap::from([("revision".into(), QueryValue::Unsigned(2))]),
            },
        )]),
    );
    let canceled = engine
        .execute_query_transaction(
            &lease.session_id,
            &lease.token,
            &cancel,
            &mutation_context(
                &id("query-program-cancel"),
                "request-query-program-cancel",
                "operation-query-program-cancel",
            ),
            1_200,
        )
        .unwrap();
    assert_eq!(canceled.state, TransactionState::Aborted);
    assert!(canceled.commit.is_none());
    let after_cancel = snapshot(&engine, &lease, 1_300, "canceled");
    assert!(matches!(
        &after_cancel.entries[0].value,
        TransactionMutation::PutRecord { properties, .. }
            if properties["revision"] == QueryValue::Unsigned(1)
    ));

    let rejected = program_request(
        "BEGIN; MUTATE $update; MUTATE $missing; COMMIT;",
        BTreeMap::from([
            (
                "update".into(),
                TransactionMutation::PutRecord {
                    reference: document.clone(),
                    valid_from: 1_300,
                    valid_to: None,
                    properties: BTreeMap::from([("revision".into(), QueryValue::Unsigned(3))]),
                },
            ),
            (
                "missing".into(),
                TransactionMutation::RetireData {
                    model: DataLogicalModel::Document,
                    target: DataTarget::Reference {
                        reference: reference("document", "missing"),
                    },
                    effective_at: 1_300,
                },
            ),
        ]),
    );
    assert!(engine
        .execute_query_transaction(
            &lease.session_id,
            &lease.token,
            &rejected,
            &mutation_context(
                &id("query-program-rejected"),
                "request-query-program-rejected",
                "operation-query-program-rejected",
            ),
            1_300,
        )
        .is_err());
    assert_eq!(snapshot(&engine, &lease, 1_300, "rejected"), after_cancel);

    let missing_binding = program_request(
        "BEGIN; MUTATE $missing; COMMIT;",
        BTreeMap::from([(
            "extra".into(),
            TransactionMutation::RetireData {
                model: DataLogicalModel::Document,
                target: DataTarget::Reference {
                    reference: document,
                },
                effective_at: 1_400,
            },
        )]),
    );
    assert!(engine
        .execute_query_transaction(
            &lease.session_id,
            &lease.token,
            &missing_binding,
            &mutation_context(
                &id("query-program-binding-error"),
                "request-query-program-binding-error",
                "operation-query-program-binding-error",
            ),
            1_400,
        )
        .is_err());

    drop(engine);
    let reopened = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    assert_eq!(snapshot(&reopened, &lease, 1_300, "reopen"), after_cancel);
    let reopened_replay = reopened
        .execute_query_transaction(&lease.session_id, &lease.token, &request, &context, 1_500)
        .unwrap();
    assert!(reopened_replay.commit.unwrap().idempotent_replay);
}

#[test]
fn query_transaction_requires_the_existing_begin_and_commit_grants() {
    let (_root, engine) = isolated_engine();
    let principal_id = CanonicalId::new("query-program-denied").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance().as_str()).unwrap()],
    };
    SecurityRepository::new(&engine.storage, instance())
        .initialize(
            SecurityState {
                format_version: rrd_security::SECURITY_FORMAT,
                revision: 1,
                principals: [(
                    principal_id.clone(),
                    Principal {
                        id: principal_id.clone(),
                        kind: PrincipalKind::Service,
                        credential_sha256: digest::sha256_hex(b"query-program-key"),
                        not_before_unix_ms: 1,
                        expires_at_unix_ms: u64::MAX,
                        disabled: false,
                        grants: vec![
                            ResourceGrant {
                                action: SecurityAction::SessionCreate,
                                resource_prefix: resource.clone(),
                            },
                            ResourceGrant {
                                action: SecurityAction::TransactionBegin,
                                resource_prefix: resource,
                            },
                        ],
                    },
                )]
                .into_iter()
                .collect(),
            },
            1,
            "query-program-security",
            "request-query-program-security",
            "operation-query-program-security",
        )
        .unwrap();
    let lease = engine
        .create_authenticated_session(
            &principal_id,
            b"query-program-key",
            &session_request(5_000, 2),
            &id("query-program-denied-session"),
            1_000,
            "request-query-program-denied-session",
            "operation-query-program-denied-session",
        )
        .unwrap();
    let request = program_request(
        "BEGIN; MUTATE $claim; COMMIT;",
        BTreeMap::from([(
            "claim".into(),
            TransactionMutation::AssertClaim {
                subject: CanonicalId::new("document-alpha").unwrap(),
                predicate: CanonicalId::new("state").unwrap(),
                object: "ready".into(),
                valid_from: 1_100,
                tx_time: 1_100,
                producer: CanonicalId::new("query-program-test").unwrap(),
                confidence: None,
            },
        )]),
    );
    let before = engine.storage.runtime_cursor().unwrap();
    let denied = engine.execute_query_transaction(
        &lease.session_id,
        &lease.token,
        &request,
        &mutation_context(
            &id("query-program-denied"),
            "request-query-program-denied",
            "operation-query-program-denied",
        ),
        1_100,
    );
    assert!(matches!(denied, Err(ServiceError::PermissionDenied)));
    assert_eq!(engine.storage.runtime_cursor().unwrap(), before);
}
