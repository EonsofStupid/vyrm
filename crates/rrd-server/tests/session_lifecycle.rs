use rrd_contract::{
    BeginTransaction, CanonicalId, CommitTransaction, CorrelationId, CreateSession, SessionLimits,
    TransactionMutation, TransactionState,
};
use rrd_server::{RrdService, ServiceError};
use serde_json::Value;
use vyrm_core::{digest, Claim, Predicate, Producer, Subject};
use vyrm_store::{Engine, MemoryEngine, NativeEngine};

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

#[test]
fn journal_redacts_tokens_and_records_expiry_once() {
    let service = RrdService::new(MemoryEngine::new(), instance());
    let lease = service
        .create_session(
            &session_request(1_000, 2),
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
        state["transactions"][transaction.transaction_id.as_str()]["state"],
        "expired"
    );
}

#[test]
fn quota_transaction_expiry_and_abort_are_authoritative_transitions() {
    let service = RrdService::new(MemoryEngine::new(), instance());
    let lease = service
        .create_session(
            &session_request(5_000, 1),
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
            2_400,
            "request-abort-replay",
            "operation-abort-replay",
        )
        .unwrap();
    assert_eq!(replay.state, TransactionState::Aborted);
    let journal = service.engine().control_journal_since(0, 10).unwrap();
    assert_eq!(journal[journal.len() - 2].action, "transaction.aborted");
    assert_eq!(journal.last().unwrap().action, "transaction.abort_replayed");
}

#[test]
fn commit_reopens_replays_and_does_not_duplicate_claims() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("native");
    let engine = NativeEngine::open(&path).unwrap();
    let service = RrdService::new(engine, instance());
    let lease = service
        .create_session(
            &session_request(5_000, 2),
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
            1_100,
            "request-begin",
            "operation-begin",
        )
        .unwrap();
    drop(service);

    let request = commit_request("durable");
    let service = RrdService::new(NativeEngine::open(&path).unwrap(), instance());
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

    let service = RrdService::new(NativeEngine::open(&path).unwrap(), instance());
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
            "transaction.committed",
            "transaction.replayed"
        ]
    );
}

#[test]
fn retry_closes_the_journal_gap_after_a_crash_window() {
    let service = RrdService::new(MemoryEngine::new(), instance());
    let lease = service
        .create_session(
            &session_request(5_000, 2),
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
            1_100,
            "request-begin",
            "operation-begin",
        )
        .unwrap();
    let request = commit_request("crash-window");

    let accepted = service
        .engine()
        .append_batch_idempotent(
            "crash-key",
            &request.operation_sha256,
            &[equivalent_claim(&lease.session_id, "crash-window")],
        )
        .unwrap();
    assert!(!accepted.idempotent_replay);
    assert_eq!(
        service.engine().control_journal_since(0, 10).unwrap().len(),
        2
    );

    let recovered = service
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("crash-key"),
            &request,
            1_200,
            "request-recover",
            "operation-recover",
        )
        .unwrap();
    assert!(recovered.idempotent_replay);
    assert_eq!(service.engine().sequence().unwrap(), 1);
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
    let service = RrdService::new(MemoryEngine::new(), instance());
    let lease = service
        .create_session(
            &session_request(5_000, 2),
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
