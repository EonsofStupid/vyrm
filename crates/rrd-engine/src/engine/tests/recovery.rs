use super::*;

#[test]
fn commit_reopens_replays_and_does_not_duplicate_claims() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("native");
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
    let transaction = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &mutation_context(&id("begin-key"), "request-begin", "operation-begin"),
            1_100,
        )
        .unwrap();
    drop(service);

    let request = commit_request("durable");
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
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
    assert_eq!(service.storage.sequence().unwrap(), 1);
    drop(service);

    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
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
    assert_eq!(service.storage.sequence().unwrap(), 1);
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
    assert_eq!(service.storage.sequence().unwrap(), 1);
    let actions = service
        .storage
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
fn durable_prepare_reopens_without_republishing_or_rejournaling() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("prepared");
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
    let lease = service
        .create_session(
            &session_request(5_000, 2),
            &id("prepare-create-key"),
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
            &mutation_context(&id("prepare-begin-key"), "request-begin", "operation-begin"),
            1_100,
        )
        .unwrap();
    let request = PreviewTransaction {
        mutations: vec![mutation("prepared")],
        valid_at: Some(1_000),
        max_scanned_changes: 10,
    };
    let context = mutation_context(&id("prepare-key"), "request-prepare", "operation-prepare");
    let first = service
        .preview_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &request,
            &context,
            1_150,
        )
        .unwrap();
    assert!(!first.idempotent_replay);
    assert_eq!(first.prospective.known_at_cursor, 1);
    assert_eq!(service.storage.runtime_cursor().unwrap(), 0);
    assert_eq!(
        service.storage.control_journal_since(0, 10).unwrap().len(),
        3
    );
    drop(service);

    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
    let replay = service
        .preview_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &request,
            &context,
            1_200,
        )
        .unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(replay.prospective, first.prospective);
    assert_eq!(service.storage.runtime_cursor().unwrap(), 0);
    assert_eq!(
        service.storage.control_journal_since(0, 10).unwrap().len(),
        3
    );

    let mut substituted = request.clone();
    substituted.mutations = vec![mutation("substituted")];
    assert!(matches!(
        service.preview_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &substituted,
            &context,
            1_250,
        ),
        Err(ServiceError::IdempotencyConflict)
    ));
    assert!(matches!(
        service.preview_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &request,
            &mutation_context(
                &id("different-prepare-key"),
                "request-collision",
                "operation-collision",
            ),
            1_250,
        ),
        Err(ServiceError::IdempotencyConflict)
    ));
}

#[test]
fn expired_session_still_resolves_an_exact_committed_transaction_replay() {
    let (_root, service) = isolated_engine();
    let lease = service
        .create_session(
            &session_request(1_000, 1),
            &id("expired-create-key"),
            1_000,
            "request-create",
            "operation-create",
        )
        .unwrap();
    let begin = begin_request();
    let begin_context =
        mutation_context(&id("expired-begin-key"), "request-begin", "operation-begin");
    let transaction = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin,
            &begin_context,
            1_100,
        )
        .unwrap();
    let commit = commit_request("expired-replay");
    let first = service
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("expired-commit-key"),
            &commit,
            1_200,
            "request-commit",
            "operation-commit",
        )
        .unwrap();
    assert!(!first.idempotent_replay);

    assert!(matches!(
        service.renew_session(
            &lease.session_id,
            &lease.token,
            &RenewSession {},
            &id("expired-renew-key"),
            2_500,
            "request-expire",
            "operation-expire",
        ),
        Err(ServiceError::SessionExpired)
    ));

    let replayed_transaction = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin,
            &begin_context,
            2_600,
        )
        .unwrap();
    assert_eq!(
        replayed_transaction.transaction_id,
        transaction.transaction_id
    );
    assert_eq!(replayed_transaction.state, TransactionState::Committed);
    let replay = service
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("expired-commit-key"),
            &commit,
            2_700,
            "request-replay",
            "operation-replay",
        )
        .unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(service.storage.sequence().unwrap(), 1);
}

#[test]
fn retry_closes_the_journal_gap_after_a_crash_window() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("crash-window");
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
    let transaction = service
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &mutation_context(&id("begin-key"), "request-begin", "operation-begin"),
            1_100,
        )
        .unwrap();
    let request = commit_request("crash-window");
    let runtime_commit = public_runtime_commit(
        &request,
        &lease.session_id,
        &instance(),
        transaction.read_cursor,
        1_150,
    )
    .unwrap();
    let runtime_commit_sha256 = runtime_commit.digest();
    let session_key = format!(
        "server/state/test-instance/session/{}",
        lease.session_id.as_str()
    );
    let before = service
        .storage
        .control_record(&session_key)
        .unwrap()
        .unwrap();
    let mut prepared: Value = serde_json::from_slice(&before).unwrap();
    prepared["transactions"][transaction.transaction_id.as_str()]["commit_intent"] = serde_json::json!({
        "idempotency_key": "crash-key",
        "operation_sha256": request.operation_sha256.clone(),
        "runtime_at_unix_ms": 1_150,
        "runtime_commit_sha256": runtime_commit_sha256,
    });
    service
        .storage
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
    drop(service);
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
    service.storage.commit_runtime(&runtime_commit).unwrap();
    assert_eq!(
        service.storage.control_journal_since(0, 10).unwrap().len(),
        3
    );

    drop(service);
    let service = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
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
    assert_eq!(service.storage.sequence().unwrap(), 1);
    let state: Value = serde_json::from_slice(
        &service
            .storage
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
            .storage
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
    let (_root, service) = isolated_engine();
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
            &mutation_context(&id("begin-key"), "request-begin", "operation-begin"),
            1_100,
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
    assert_eq!(service.storage.sequence().unwrap(), 0);
    assert_eq!(
        service.storage.control_journal_since(0, 10).unwrap().len(),
        2
    );
}
