use rrd_contract::CanonicalId;
use rrd_estate::{
    ActivityClass, ActivityEvidence, DesiredPhase, DesiredTarget, Error, EstateRepository,
    LeaseRequest, MutationContext, OperationState, ReceiptBoundary, ReceiptRequest, SetDesired,
};
use rrd_store::{Engine, MemoryEngine, NativeEngine};

fn id(value: &str) -> CanonicalId {
    CanonicalId::new(value).unwrap()
}

fn context(at: u64, request: &str, operation: &str) -> MutationContext {
    MutationContext {
        at,
        actor: "rrd-estate".into(),
        request_id: request.into(),
        operation_id: id(operation),
    }
}

fn target(phase: DesiredPhase, version: &str) -> DesiredTarget {
    DesiredTarget {
        phase,
        deployment_ref: id("local-rrd"),
        version: version.into(),
        configuration_sha256: "a".repeat(64),
    }
}

fn set_request(at: u64, key: &str, operation: &str) -> SetDesired {
    SetDesired {
        context: context(at, &format!("request-{at}"), operation),
        instance_id: id("project-a"),
        idempotency_key: key.into(),
        target: target(DesiredPhase::Running, "0.1.0"),
    }
}

#[test]
fn desired_state_is_journaled_and_idempotency_is_durable() {
    let engine = MemoryEngine::new();
    let repository = EstateRepository::new(&engine, id("estate-a"));
    repository
        .create(&context(10, "create-estate", "create-estate"))
        .unwrap();

    let request = set_request(20, "deploy-project-a", "deploy-project-a");
    let accepted = repository.set_desired(&request).unwrap();
    assert!(!accepted.idempotent_replay);
    assert_eq!(accepted.document.revision, 2);
    assert_eq!(accepted.operation.state, OperationState::Pending);
    assert_eq!(
        accepted
            .document
            .instance(&id("project-a"))
            .unwrap()
            .desired
            .generation,
        1
    );

    let replay = repository.set_desired(&request).unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(replay.document.revision, 2);
    assert_eq!(engine.control_journal_since(0, 10).unwrap().len(), 2);

    let mut rebound = request;
    rebound.target.version = "0.2.0".into();
    assert!(matches!(
        repository.set_desired(&rebound),
        Err(Error::IdempotencyConflict(_))
    ));
}

#[test]
fn expired_workers_are_fenced_and_recovery_uses_a_new_epoch() {
    let engine = MemoryEngine::new();
    let repository = EstateRepository::new(&engine, id("estate-a"));
    repository
        .create(&context(10, "create-estate", "create-estate"))
        .unwrap();
    repository
        .set_desired(&set_request(20, "deploy-project-a", "deploy-project-a"))
        .unwrap();

    let leased = repository
        .acquire_lease(&LeaseRequest {
            context: context(30, "lease-one", "deploy-project-a"),
            worker: id("worker-one"),
            lease_ms: 1_000,
        })
        .unwrap();
    assert_eq!(
        leased
            .operation(&id("deploy-project-a"))
            .unwrap()
            .lease
            .as_ref()
            .unwrap()
            .epoch,
        1
    );
    repository
        .record_receipt(&ReceiptRequest {
            context: context(40, "prepared-one", "deploy-project-a"),
            worker: id("worker-one"),
            lease_epoch: 1,
            boundary: ReceiptBoundary::Prepared,
            evidence_sha256: "b".repeat(64),
            error: None,
        })
        .unwrap();

    let recovered = repository
        .acquire_lease(&LeaseRequest {
            context: context(1_031, "lease-two", "deploy-project-a"),
            worker: id("worker-two"),
            lease_ms: 1_000,
        })
        .unwrap();
    let operation = recovered.operation(&id("deploy-project-a")).unwrap();
    assert_eq!(operation.state, OperationState::Prepared);
    assert_eq!(operation.lease.as_ref().unwrap().epoch, 2);

    assert!(matches!(
        repository.record_receipt(&ReceiptRequest {
            context: context(1_040, "stale-applied", "deploy-project-a"),
            worker: id("worker-one"),
            lease_epoch: 1,
            boundary: ReceiptBoundary::Applied,
            evidence_sha256: "c".repeat(64),
            error: None,
        }),
        Err(Error::StaleLease(_))
    ));

    let applied_after_takeover = repository
        .record_receipt(&ReceiptRequest {
            context: context(1_050, "applied-two", "deploy-project-a"),
            worker: id("worker-two"),
            lease_epoch: 2,
            boundary: ReceiptBoundary::Applied,
            evidence_sha256: "d".repeat(64),
            error: None,
        })
        .unwrap();
    assert_eq!(
        applied_after_takeover
            .operation(&id("deploy-project-a"))
            .unwrap()
            .receipts
            .len(),
        2
    );

    repository
        .record_receipt(&ReceiptRequest {
            context: context(1_070, "completed-two", "deploy-project-a"),
            worker: id("worker-two"),
            lease_epoch: 2,
            boundary: ReceiptBoundary::Completed,
            evidence_sha256: "e".repeat(64),
            error: None,
        })
        .unwrap();
    let replayed_after_expiry = repository
        .record_receipt(&ReceiptRequest {
            context: context(3_000, "completed-retry", "deploy-project-a"),
            worker: id("worker-two"),
            lease_epoch: 2,
            boundary: ReceiptBoundary::Completed,
            evidence_sha256: "e".repeat(64),
            error: None,
        })
        .unwrap();
    let operation = replayed_after_expiry
        .operation(&id("deploy-project-a"))
        .unwrap();
    assert_eq!(operation.state, OperationState::Succeeded);
    assert_eq!(operation.receipts.len(), 3);
}

#[test]
fn activity_classification_keeps_evidence_and_recomputes_over_time() {
    let engine = MemoryEngine::new();
    let repository = EstateRepository::new(&engine, id("estate-a"));
    repository
        .create(&context(10, "create-estate", "create-estate"))
        .unwrap();
    repository
        .set_desired(&set_request(20, "deploy-project-a", "deploy-project-a"))
        .unwrap();

    let active = repository
        .record_activity(&ActivityEvidence {
            context: context(100, "activity-one", "activity-one"),
            instance_id: id("project-a"),
            meaningful_runtime_at: Some(100),
            heartbeat_at: Some(100),
        })
        .unwrap();
    assert_eq!(
        active.instance(&id("project-a")).unwrap().activity.class,
        ActivityClass::Active
    );

    let neglected = repository
        .refresh_activity(&context(
            8 * 24 * 60 * 60 * 1_000,
            "activity-refresh",
            "activity-refresh",
        ))
        .unwrap();
    let activity = &neglected.instance(&id("project-a")).unwrap().activity;
    assert_eq!(activity.class, ActivityClass::Neglected);
    assert_eq!(activity.last_meaningful_runtime_at, Some(100));
}

#[test]
fn native_reopen_recovers_authority_and_hash_chained_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("estate-native");
    {
        let engine = NativeEngine::open(&path).unwrap();
        let repository = EstateRepository::new(&engine, id("estate-a"));
        repository
            .create(&context(10, "create-estate", "create-estate"))
            .unwrap();
        repository
            .set_desired(&set_request(20, "deploy-project-a", "deploy-project-a"))
            .unwrap();
        repository
            .acquire_lease(&LeaseRequest {
                context: context(30, "lease-one", "deploy-project-a"),
                worker: id("worker-one"),
                lease_ms: 1_000,
            })
            .unwrap();
    }

    let reopened = NativeEngine::open(&path).unwrap();
    let repository = EstateRepository::new(&reopened, id("estate-a"));
    let document = repository.load().unwrap().unwrap();
    assert_eq!(document.revision, 3);
    assert_eq!(
        document.operation(&id("deploy-project-a")).unwrap().state,
        OperationState::Leased
    );
    let journal = reopened.control_journal_since(0, 10).unwrap();
    assert_eq!(journal.len(), 3);
    assert!(journal.iter().all(|entry| entry.verify()));
    assert_eq!(
        journal[1].previous_digest.as_deref(),
        Some(journal[0].digest.as_str())
    );
    assert_eq!(
        journal[2].previous_digest.as_deref(),
        Some(journal[1].digest.as_str())
    );
}
