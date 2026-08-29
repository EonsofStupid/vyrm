use super::*;

fn assert_tree_excludes(root: &std::path::Path, secrets: &[&str]) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let file_type = entry.file_type().unwrap();
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                let bytes = std::fs::read(entry.path()).unwrap();
                for secret in secrets {
                    assert!(
                        !bytes
                            .windows(secret.len())
                            .any(|window| window == secret.as_bytes()),
                        "raw session secret reached {}",
                        entry.path().display()
                    );
                }
            }
        }
    }
}

#[test]
fn journal_redacts_tokens_and_records_expiry_once() {
    let (_root, service) = isolated_engine();
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
            &mutation_context(&id("begin-key"), "request-begin", "operation-begin"),
            1_500,
        )
        .unwrap();

    let wrong_token = id("wrong-token");
    assert!(matches!(
        service.begin_transaction(
            &lease.session_id,
            &wrong_token,
            &begin_request(),
            &mutation_context(
                &id("wrong-token-key"),
                "request-wrong-token",
                "operation-wrong-token",
            ),
            1_600,
        ),
        Err(ServiceError::Unauthenticated)
    ));
    assert_eq!(
        service.storage.control_journal_since(0, 10).unwrap().len(),
        2
    );

    assert!(matches!(
        service.begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &mutation_context(&id("expire-key"), "request-expire", "operation-expire"),
            2_500,
        ),
        Err(ServiceError::SessionExpired)
    ));
    assert!(matches!(
        service.begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &mutation_context(
                &id("expire-retry-key"),
                "request-expire-retry",
                "operation-expire-retry",
            ),
            2_600,
        ),
        Err(ServiceError::SessionExpired)
    ));

    let journal = service.storage.control_journal_since(0, 10).unwrap();
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
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("lifecycle");
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
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
            &mutation_context(&id("begin-key"), "request-begin", "operation-begin"),
            1_100,
        )
        .unwrap();
    assert!(matches!(
        service.begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &mutation_context(&id("quota-key"), "request-quota", "operation-quota"),
            1_200,
        ),
        Err(ServiceError::TransactionQuota)
    ));
    assert_eq!(
        service.storage.control_journal_since(0, 10).unwrap().len(),
        2
    );
    drop(service);
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
    assert!(matches!(
        service.abort_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &AbortTransaction {},
            &mutation_context(
                &id("expire-abort-key"),
                "request-expire",
                "operation-expire",
            ),
            2_100,
        ),
        Err(ServiceError::TransactionExpired)
    ));
    let journal = service.storage.control_journal_since(0, 10).unwrap();
    assert_eq!(journal.last().unwrap().action, "transaction.expired");

    let second = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &mutation_context(&id("begin-key-2"), "request-begin-2", "operation-begin-2"),
            2_200,
        )
        .unwrap();
    drop(service);
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
    let aborted = service
        .abort_transaction(
            &lease.session_id,
            &lease.token,
            &second.transaction_id,
            &AbortTransaction {},
            &mutation_context(&id("abort-key"), "request-abort", "operation-abort"),
            2_300,
        )
        .unwrap();
    assert_eq!(aborted.state, TransactionState::Aborted);
    drop(service);
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
    let replay = service
        .abort_transaction(
            &lease.session_id,
            &lease.token,
            &second.transaction_id,
            &AbortTransaction {},
            &mutation_context(
                &id("abort-key"),
                "request-abort-replay",
                "operation-abort-replay",
            ),
            2_400,
        )
        .unwrap();
    assert_eq!(replay.state, TransactionState::Aborted);
    let journal = service.storage.control_journal_since(0, 10).unwrap();
    assert_eq!(journal.last().unwrap().action, "transaction.aborted");
}

#[test]
fn create_and_begin_replay_exact_responses_and_reject_collisions() {
    let (_root, service) = isolated_engine();
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
        service.storage.control_journal_since(0, 10).unwrap().len(),
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
            &mutation_context(&begin_key, "request-begin", "operation-begin"),
            2_100,
        )
        .unwrap();
    let transaction_replay = service
        .begin_transaction(
            &first.session_id,
            &first.token,
            &begin_request(),
            &mutation_context(&begin_key, "request-begin-replay", "operation-begin-replay"),
            2_200,
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
            &mutation_context(
                &begin_key,
                "request-begin-collision",
                "operation-begin-collision",
            ),
            2_300,
        ),
        Err(ServiceError::IdempotencyConflict)
    ));
    assert_eq!(
        service.storage.control_journal_since(0, 10).unwrap().len(),
        2
    );
}

#[test]
fn renewal_rotates_without_persisting_tokens_and_close_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("lifecycle");
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
    let lease = service
        .create_session(
            &session_request(5_000, 2),
            &id("create-key"),
            1_000,
            "request-create",
            "operation-create",
        )
        .unwrap();
    drop(service);
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
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
    drop(service);
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
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
            &mutation_context(
                &id("old-token-begin"),
                "request-old-token",
                "operation-old-token",
            ),
            1_300,
        ),
        Err(ServiceError::Unauthenticated)
    ));
    let transaction = service
        .begin_transaction(
            &lease.session_id,
            &renewed.token,
            &begin_request(),
            &mutation_context(
                &id("new-token-begin"),
                "request-new-token",
                "operation-new-token",
            ),
            1_300,
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
    drop(service);
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
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
            &mutation_context(
                &id("abort-after-close"),
                "request-abort-after-close",
                "operation-abort-after-close",
            ),
            1_600,
        ),
        Err(ServiceError::SessionExpired)
    ));

    let journal = service.storage.control_journal_since(0, 10).unwrap();
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
    drop(service);
    assert_tree_excludes(&path, &[lease.token.as_str(), renewed.token.as_str()]);
}
