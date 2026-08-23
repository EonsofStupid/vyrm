use rrd_contract::{
    AbortTransaction, BeginTransaction, CanonicalId, CloseSession, CommitTransaction,
    CorrelationId, CreateSession, RenewSession, SessionLimits, TransactionMutation,
    TransactionState,
};
use rrd_server::{RrdService, ServiceError};
use serde_json::Value;
use vyrm_core::{digest, Claim, Predicate, Producer, Subject};
use vyrm_store::{ControlTransition, Engine, MemoryEngine, NativeEngine};

const TOKEN_KEY: [u8; 32] = [7; 32];

fn id(value: &str) -> CorrelationId {
    CorrelationId::new(value).unwrap()
}

fn instance() -> CanonicalId {
    CanonicalId::new("test-instance").unwrap()
}

fn session_request(idle_timeout_ms: u64, max_open_transactions: u16) -> CreateSession {
    CreateSession {
        limits: SessionLimits {
            idle_timeout_ms,
            absolute_timeout_ms: 10_000,
            max_open_transactions,
        },
    }
}

fn begin_request() -> BeginTransaction {
    BeginTransaction {
        scope: CanonicalId::new("claims").unwrap(),
        timeout_ms: 1_000,
    }
}

fn mutation(object: &str) -> TransactionMutation {
    TransactionMutation::AssertClaim {
        subject: CanonicalId::new("document-1").unwrap(),
        predicate: CanonicalId::new("contains").unwrap(),
        object: object.into(),
        valid_from: 1_000,
        tx_time: 1_000,
        producer: CanonicalId::new("test-runner").unwrap(),
        confidence: Some(0.9),
    }
}

fn commit_request(object: &str) -> CommitTransaction {
    let mutations = vec![mutation(object)];
    CommitTransaction {
        operation_sha256: digest::sha256_hex(&serde_json::to_vec(&mutations).unwrap()),
        mutations,
    }
}

fn equivalent_claim(session_id: &CorrelationId, object: &str) -> Claim {
    let mut claim = Claim::new(
        Subject::new("document-1").unwrap(),
        Predicate::new("contains").unwrap(),
        object,
        1_000,
        1_000,
        Producer {
            actor: "test-runner".into(),
            on_behalf_of: None,
            session: Some(session_id.as_str().into()),
        },
    );
    claim.confidence = Some(0.9);
    claim
}

fn acceptance_key(session_id: &CorrelationId, idempotency_key: &CorrelationId) -> String {
    format!(
        "rrd:{}",
        digest::sha256_hex(
            format!(
                "test-instance\0{}\0{}",
                session_id.as_str(),
                idempotency_key.as_str()
            )
            .as_bytes()
        )
    )
}

#[test]
fn journal_redacts_tokens_and_records_expiry_once() {
    let service = RrdService::new(MemoryEngine::new(), instance(), TOKEN_KEY);
    let lease = service
        .create_session(
            &session_request(1_000, 2),
            &id("create-key"),
            1_000,
            "request-create",
            "operation-create",
        )
        .unwrap();
    let transaction = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &id("begin-key"),
            1_500,
            "request-begin",
            "operation-begin",
        )
        .unwrap();

    let wrong_token = id("wrong-token");
    assert!(matches!(
        service.begin_transaction(
            &lease.session_id,
            &wrong_token,
            &begin_request(),
            &id("wrong-token-key"),
            1_600,
            "request-wrong-token",
            "operation-wrong-token",
        ),
        Err(ServiceError::Unauthenticated)
    ));
    assert_eq!(
        service.engine().control_journal_since(0, 10).unwrap().len(),
        2
    );

    assert!(matches!(
        service.begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &id("expire-key"),
            2_500,
            "request-expire",
            "operation-expire",
        ),
        Err(ServiceError::SessionExpired)
    ));
    assert!(matches!(
        service.begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &id("expire-retry-key"),
            2_600,
            "request-expire-retry",
            "operation-expire-retry",
        ),
        Err(ServiceError::SessionExpired)
    ));

    let journal = service.engine().control_journal_since(0, 10).unwrap();
    assert_eq!(journal.len(), 3);
    assert_eq!(journal[0].action, "session.created");
    assert_eq!(journal[1].action, "transaction.began");
    assert_eq!(journal[2].action, "session.expired");
    assert!(journal.iter().all(|entry| entry.verify()));
    assert_eq!(
        journal[1].previous_digest.as_deref(),
        Some(journal[0].digest.as_str())
    );
    assert_eq!(
        journal[2].previous_digest.as_deref(),
        Some(journal[1].digest.as_str())
    );
    for entry in &journal {
        if let Some(replacement) = &entry.replacement {
            assert!(!String::from_utf8_lossy(replacement).contains(lease.token.as_str()));
        }
    }
    let state: Value = serde_json::from_slice(journal[2].replacement.as_ref().unwrap()).unwrap();
    assert_eq!(state["status"], "expired");
    assert_eq!(
        state["transactions"][transaction.transaction_id.as_str()]["lease"]["state"],
        "expired"
    );
}

#[test]
fn quota_transaction_expiry_and_abort_are_authoritative_transitions() {
    let service = RrdService::new(MemoryEngine::new(), instance(), TOKEN_KEY);
    let lease = service
        .create_session(
            &session_request(5_000, 1),
            &id("create-key"),
            1_000,
            "request-create",
            "operation-create",
        )
        .unwrap();
    let transaction = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &id("begin-key"),
            1_100,
            "request-begin",
            "operation-begin",
        )
        .unwrap();
    assert!(matches!(
        service.begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &id("quota-key"),
            1_200,
            "request-quota",
            "operation-quota",
        ),
        Err(ServiceError::TransactionQuota)
    ));
    assert_eq!(
        service.engine().control_journal_since(0, 10).unwrap().len(),
        2
    );
    assert!(matches!(
        service.abort_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &AbortTransaction {},
            &id("expire-abort-key"),
            2_100,
            "request-expire",
            "operation-expire",
        ),
        Err(ServiceError::TransactionExpired)
    ));
    let journal = service.engine().control_journal_since(0, 10).unwrap();
    assert_eq!(journal.last().unwrap().action, "transaction.expired");

    let second = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &id("begin-key-2"),
            2_200,
            "request-begin-2",
            "operation-begin-2",
        )
        .unwrap();
    let aborted = service
        .abort_transaction(
            &lease.session_id,
            &lease.token,
            &second.transaction_id,
            &AbortTransaction {},
            &id("abort-key"),
            2_300,
            "request-abort",
            "operation-abort",
        )
        .unwrap();
    assert_eq!(aborted.state, TransactionState::Aborted);
    let replay = service
        .abort_transaction(
            &lease.session_id,
            &lease.token,
            &second.transaction_id,
            &AbortTransaction {},
            &id("abort-key"),
            2_400,
            "request-abort-replay",
            "operation-abort-replay",
        )
        .unwrap();
    assert_eq!(replay.state, TransactionState::Aborted);
    let journal = service.engine().control_journal_since(0, 10).unwrap();
    assert_eq!(journal.last().unwrap().action, "transaction.aborted");
}

#[test]
fn commit_reopens_replays_and_does_not_duplicate_claims() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("native");
    let engine = NativeEngine::open(&path).unwrap();
    let service = RrdService::new(engine, instance(), TOKEN_KEY);
    let lease = service
        .create_session(
            &session_request(5_000, 2),
            &id("create-key"),
            1_000,
            "request-create",
            "operation-create",
        )
        .unwrap();
    let transaction = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &id("begin-key"),
            1_100,
            "request-begin",
            "operation-begin",
        )
        .unwrap();
    drop(service);

    let request = commit_request("durable");
    let service = RrdService::new(NativeEngine::open(&path).unwrap(), instance(), TOKEN_KEY);
    let first = service
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("commit-key"),
            &request,
            1_200,
            "request-commit",
            "operation-commit",
        )
        .unwrap();
    assert!(!first.idempotent_replay);
    assert_eq!(service.engine().sequence().unwrap(), 1);
    drop(service);

    let service = RrdService::new(NativeEngine::open(&path).unwrap(), instance(), TOKEN_KEY);
    let replay = service
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("commit-key"),
            &request,
            1_300,
            "request-replay",
            "operation-replay",
        )
        .unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(service.engine().sequence().unwrap(), 1);
    assert!(matches!(
        service.commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("different-commit-key"),
            &request,
            1_350,
            "request-collision",
            "operation-collision",
        ),
        Err(ServiceError::IdempotencyConflict)
    ));
    assert_eq!(service.engine().sequence().unwrap(), 1);
    let actions = service
        .engine()
        .control_journal_since(0, 10)
        .unwrap()
        .into_iter()
        .map(|entry| entry.action)
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        [
            "session.created",
            "transaction.began",
            "transaction.commit_prepared",
            "transaction.committed",
        ]
    );
}

#[test]
fn retry_closes_the_journal_gap_after_a_crash_window() {
    let service = RrdService::new(MemoryEngine::new(), instance(), TOKEN_KEY);
    let lease = service
        .create_session(
            &session_request(5_000, 2),
            &id("create-key"),
            1_000,
            "request-create",
            "operation-create",
        )
        .unwrap();
    let transaction = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &id("begin-key"),
            1_100,
            "request-begin",
            "operation-begin",
        )
        .unwrap();
    let request = commit_request("crash-window");
    let session_key = format!(
        "server/state/test-instance/session/{}",
        lease.session_id.as_str()
    );
    let before = service
        .engine()
        .control_record(&session_key)
        .unwrap()
        .unwrap();
    let mut prepared: Value = serde_json::from_slice(&before).unwrap();
    prepared["transactions"][transaction.transaction_id.as_str()]["commit_intent"] = serde_json::json!({
        "idempotency_key": "crash-key",
        "operation_sha256": request.operation_sha256.clone(),
    });
    service
        .engine()
        .commit_control_transition(&ControlTransition {
            key: session_key,
            expected: Some(before),
            replacement: Some(serde_json::to_vec(&prepared).unwrap()),
            at: 1_150,
            actor: "rrd-server".into(),
            action: "transaction.commit_prepared".into(),
            request_id: "request-prepare".into(),
            operation_id: "operation-prepare".into(),
        })
        .unwrap();

    let crash_key = id("crash-key");
    let accepted = service
        .engine()
        .append_batch_idempotent(
            &acceptance_key(&lease.session_id, &crash_key),
            &request.operation_sha256,
            &[equivalent_claim(&lease.session_id, "crash-window")],
        )
        .unwrap();
    assert!(!accepted.idempotent_replay);
    assert_eq!(
        service.engine().control_journal_since(0, 10).unwrap().len(),
        3
    );

    let recovered = service
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &crash_key,
            &request,
            7_000,
            "request-recover",
            "operation-recover",
        )
        .unwrap();
    assert!(recovered.idempotent_replay);
    assert_eq!(service.engine().sequence().unwrap(), 1);
    let state: Value = serde_json::from_slice(
        &service
            .engine()
            .control_record(&format!(
                "server/state/test-instance/session/{}",
                lease.session_id.as_str()
            ))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(state["status"], "expired");
    assert_eq!(
        service
            .engine()
            .control_journal_since(0, 10)
            .unwrap()
            .last()
            .unwrap()
            .action,
        "transaction.committed"
    );
}

#[test]
fn an_operation_digest_mismatch_never_mutates_data_or_journal() {
    let service = RrdService::new(MemoryEngine::new(), instance(), TOKEN_KEY);
    let lease = service
        .create_session(
            &session_request(5_000, 2),
            &id("create-key"),
            1_000,
            "request-create",
            "operation-create",
        )
        .unwrap();
    let transaction = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &id("begin-key"),
            1_100,
            "request-begin",
            "operation-begin",
        )
        .unwrap();
    let mut request = commit_request("rejected");
    request.operation_sha256 = "0".repeat(64);
    assert!(matches!(
        service.commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("rejected-key"),
            &request,
            1_200,
            "request-rejected",
            "operation-rejected",
        ),
        Err(ServiceError::OperationDigestMismatch)
    ));
    assert_eq!(service.engine().sequence().unwrap(), 0);
    assert_eq!(
        service.engine().control_journal_since(0, 10).unwrap().len(),
        2
    );
}

#[test]
fn create_and_begin_replay_exact_responses_and_reject_collisions() {
    let service = RrdService::new(MemoryEngine::new(), instance(), TOKEN_KEY);
    let request = session_request(5_000, 2);
    let create_key = id("create-key");
    let first = service
        .create_session(
            &request,
            &create_key,
            1_000,
            "request-create",
            "operation-create",
        )
        .unwrap();
    let replay = service
        .create_session(
            &request,
            &create_key,
            2_000,
            "request-create-replay",
            "operation-create-replay",
        )
        .unwrap();
    assert_eq!(replay, first);
    assert_eq!(
        service.engine().control_journal_since(0, 10).unwrap().len(),
        1
    );
    assert!(matches!(
        service.create_session(
            &session_request(4_000, 2),
            &create_key,
            2_000,
            "request-create-collision",
            "operation-create-collision",
        ),
        Err(ServiceError::IdempotencyConflict)
    ));

    let begin_key = id("begin-key");
    let transaction = service
        .begin_transaction(
            &first.session_id,
            &first.token,
            &begin_request(),
            &begin_key,
            2_100,
            "request-begin",
            "operation-begin",
        )
        .unwrap();
    let transaction_replay = service
        .begin_transaction(
            &first.session_id,
            &first.token,
            &begin_request(),
            &begin_key,
            2_200,
            "request-begin-replay",
            "operation-begin-replay",
        )
        .unwrap();
    assert_eq!(transaction_replay, transaction);
    let mut collision = begin_request();
    collision.timeout_ms = 2_000;
    assert!(matches!(
        service.begin_transaction(
            &first.session_id,
            &first.token,
            &collision,
            &begin_key,
            2_300,
            "request-begin-collision",
            "operation-begin-collision",
        ),
        Err(ServiceError::IdempotencyConflict)
    ));
    assert_eq!(
        service.engine().control_journal_since(0, 10).unwrap().len(),
        2
    );
}

#[test]
fn renewal_rotates_without_persisting_tokens_and_close_is_idempotent() {
    let service = RrdService::new(MemoryEngine::new(), instance(), TOKEN_KEY);
    let lease = service
        .create_session(
            &session_request(5_000, 2),
            &id("create-key"),
            1_000,
            "request-create",
            "operation-create",
        )
        .unwrap();
    let renewal_key = id("renew-key");
    let renewed = service
        .renew_session(
            &lease.session_id,
            &lease.token,
            &RenewSession {},
            &renewal_key,
            1_100,
            "request-renew",
            "operation-renew",
        )
        .unwrap();
    assert_ne!(renewed.token, lease.token);
    let replay = service
        .renew_session(
            &lease.session_id,
            &lease.token,
            &RenewSession {},
            &renewal_key,
            1_200,
            "request-renew-replay",
            "operation-renew-replay",
        )
        .unwrap();
    assert_eq!(replay, renewed);
    assert!(matches!(
        service.begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &id("old-token-begin"),
            1_300,
            "request-old-token",
            "operation-old-token",
        ),
        Err(ServiceError::Unauthenticated)
    ));
    let transaction = service
        .begin_transaction(
            &lease.session_id,
            &renewed.token,
            &begin_request(),
            &id("new-token-begin"),
            1_300,
            "request-new-token",
            "operation-new-token",
        )
        .unwrap();
    let closed = service
        .close_session(
            &lease.session_id,
            &renewed.token,
            &CloseSession {},
            &id("close-key"),
            1_400,
            "request-close",
            "operation-close",
        )
        .unwrap();
    assert_eq!(closed.affected_open_transactions, 1);
    assert!(!closed.idempotent_replay);
    let close_replay = service
        .close_session(
            &lease.session_id,
            &renewed.token,
            &CloseSession {},
            &id("close-key"),
            1_500,
            "request-close-replay",
            "operation-close-replay",
        )
        .unwrap();
    assert!(close_replay.idempotent_replay);
    assert_eq!(close_replay.ended_at_unix_ms, closed.ended_at_unix_ms);
    assert!(matches!(
        service.abort_transaction(
            &lease.session_id,
            &renewed.token,
            &transaction.transaction_id,
            &AbortTransaction {},
            &id("abort-after-close"),
            1_600,
            "request-abort-after-close",
            "operation-abort-after-close",
        ),
        Err(ServiceError::SessionExpired)
    ));

    let journal = service.engine().control_journal_since(0, 10).unwrap();
    assert_eq!(
        journal
            .iter()
            .map(|entry| entry.action.as_str())
            .collect::<Vec<_>>(),
        [
            "session.created",
            "session.renewed",
            "transaction.began",
            "session.closed"
        ]
    );
    for entry in journal {
        let replacement = String::from_utf8_lossy(entry.replacement.as_ref().unwrap());
        assert!(!replacement.contains(lease.token.as_str()));
        assert!(!replacement.contains(renewed.token.as_str()));
    }
}
