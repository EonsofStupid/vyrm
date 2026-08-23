use rrd_contract::CanonicalId;
use rrd_estate::{
    public_backup_jobs, BackupJobState, DesiredPhase, DesiredTarget, Error, EstateDocument,
    EstateRepository, LeaseRequest, MutationContext, ObservationRequest, ObservedPhase,
    ReceiptBoundary, ReceiptRequest, ScheduleBackup, SetDesired,
};
use vyrm_store::{Engine, MemoryEngine, NativeEngine};

fn id(value: &str) -> CanonicalId {
    CanonicalId::new(value).unwrap()
}

fn context(at: u64, request: &str, operation: &str) -> MutationContext {
    MutationContext {
        at,
        actor: "operator-one".into(),
        request_id: request.into(),
        operation_id: id(operation),
    }
}

fn prepare_stopped_instance<E: Engine>(engine: &E) {
    let repository = EstateRepository::new(engine, id("estate-a"));
    repository
        .create(&context(10, "create-estate", "create-estate"))
        .unwrap();
    repository
        .set_desired(&SetDesired {
            context: context(20, "stop-instance", "stop-instance"),
            instance_id: id("instance-a"),
            idempotency_key: "stop-instance".into(),
            target: DesiredTarget {
                phase: DesiredPhase::Stopped,
                deployment_ref: id("rrd-server"),
                version: "0.1.0".into(),
                configuration_sha256: "a".repeat(64),
            },
        })
        .unwrap();
    repository
        .acquire_lease(&LeaseRequest {
            context: context(30, "lease-stop", "stop-instance"),
            worker: id("worker-one"),
            lease_ms: 1_000,
        })
        .unwrap();
    repository
        .record_receipt(&ReceiptRequest {
            context: context(40, "prepare-stop", "stop-instance"),
            worker: id("worker-one"),
            lease_epoch: 1,
            boundary: ReceiptBoundary::Prepared,
            evidence_sha256: "b".repeat(64),
            error: None,
        })
        .unwrap();
    repository
        .record_receipt(&ReceiptRequest {
            context: context(50, "apply-stop", "stop-instance"),
            worker: id("worker-one"),
            lease_epoch: 1,
            boundary: ReceiptBoundary::Applied,
            evidence_sha256: "c".repeat(64),
            error: None,
        })
        .unwrap();
    repository
        .record_observation(&ObservationRequest {
            context: context(60, "observe-stop", "stop-instance"),
            worker: id("worker-one"),
            lease_epoch: 1,
            phase: ObservedPhase::Stopped,
            version: None,
            process_id: None,
            evidence_sha256: "d".repeat(64),
            error: None,
        })
        .unwrap();
    repository
        .record_receipt(&ReceiptRequest {
            context: context(70, "complete-stop", "stop-instance"),
            worker: id("worker-one"),
            lease_epoch: 1,
            boundary: ReceiptBoundary::Completed,
            evidence_sha256: "e".repeat(64),
            error: None,
        })
        .unwrap();
}

fn backup_request() -> ScheduleBackup {
    ScheduleBackup {
        context: context(80, "backup-daily", "backup-daily"),
        instance_id: id("instance-a"),
        idempotency_key: "backup-daily".into(),
        label: "daily.0001".into(),
    }
}

#[test]
fn backup_schedule_is_quiescence_bound_and_durably_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("estate-native");
    {
        let engine = NativeEngine::open(&database).unwrap();
        prepare_stopped_instance(&engine);
        let repository = EstateRepository::new(&engine, id("estate-a"));
        let accepted = repository.schedule_backup(&backup_request()).unwrap();
        assert!(!accepted.idempotent_replay);
        assert_eq!(accepted.job.state, BackupJobState::Pending);
        assert_eq!(accepted.job.source_generation, 1);
        assert_eq!(accepted.document.revision, 8);
        let public = public_backup_jobs(&accepted.document);
        assert_eq!(public.estate_revision, 8);
        assert_eq!(public.jobs.len(), 1);
        assert_eq!(public.jobs[0].label, "daily.0001");
    }

    let engine = NativeEngine::open(&database).unwrap();
    let repository = EstateRepository::new(&engine, id("estate-a"));
    let replay = repository.schedule_backup(&backup_request()).unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(replay.document.revision, 8);
    assert_eq!(replay.document.backup_jobs.len(), 1);
    let journal = engine.control_journal_since(0, 32).unwrap();
    assert_eq!(journal.last().unwrap().action, "estate.backup.schedule");
    assert_eq!(
        journal
            .iter()
            .filter(|entry| entry.action == "estate.backup.schedule")
            .count(),
        1
    );

    let mut rebound = backup_request();
    rebound.label = "daily.0002".into();
    assert!(matches!(
        repository.schedule_backup(&rebound),
        Err(Error::IdempotencyConflict(_))
    ));
}

#[test]
fn backup_schedule_denies_an_unobserved_or_running_instance() {
    let engine = MemoryEngine::new();
    let repository = EstateRepository::new(&engine, id("estate-a"));
    repository
        .create(&context(10, "create-estate", "create-estate"))
        .unwrap();
    repository
        .set_desired(&SetDesired {
            context: context(20, "run-instance", "run-instance"),
            instance_id: id("instance-a"),
            idempotency_key: "run-instance".into(),
            target: DesiredTarget {
                phase: DesiredPhase::Running,
                deployment_ref: id("rrd-server"),
                version: "0.1.0".into(),
                configuration_sha256: "a".repeat(64),
            },
        })
        .unwrap();
    let error = repository.schedule_backup(&backup_request()).unwrap_err();
    assert!(error
        .to_string()
        .contains("requires desired and observed stopped"));
    assert!(repository.load().unwrap().unwrap().backup_jobs.is_empty());
}

#[test]
fn legacy_estate_documents_decode_without_backup_fields() {
    let document = EstateDocument::new(id("estate-a"), 10).unwrap();
    let encoded = serde_json::to_value(&document).unwrap();
    assert!(encoded.get("backup_jobs").is_none());
    assert!(encoded.get("backup_idempotency").is_none());

    let decoded: EstateDocument = serde_json::from_value(encoded).unwrap();
    assert!(decoded.backup_jobs.is_empty());
    assert!(decoded.backup_idempotency.is_empty());
    decoded.validate().unwrap();
}
