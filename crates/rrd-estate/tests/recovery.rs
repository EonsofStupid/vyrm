use rrd_contract::CanonicalId;
use rrd_estate::{
    public_recovery_snapshot, retention_decision, BackupCompleteRequest, BackupLeaseRequest,
    BackupPreparedRequest, BackupResult, CompleteRecoveryPrune, DesiredPhase, DesiredTarget, Error,
    EstateDocument, EstateRepository, LeaseRequest, MutationContext, ObservationRequest,
    ObservedPhase, PinRecoveryPoint, PrepareRecoveryPrune, ReceiptBoundary, ReceiptRequest,
    RecordRestoreEvidence, ReleaseRecoveryPin, ScheduleBackup, SetDesired, SetRecoveryPolicy,
};
use rrd_store::{Engine, MemoryEngine, NativeEngine};

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
    for (at, request, boundary, digest) in [
        (40, "prepare-stop", ReceiptBoundary::Prepared, "b"),
        (50, "apply-stop", ReceiptBoundary::Applied, "c"),
    ] {
        repository
            .record_receipt(&ReceiptRequest {
                context: context(at, request, "stop-instance"),
                worker: id("worker-one"),
                lease_epoch: 1,
                boundary,
                evidence_sha256: digest.repeat(64),
                error: None,
            })
            .unwrap();
    }
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

fn set_policy<E: Engine>(engine: &E, at: u64, operation: &str) {
    EstateRepository::new(engine, id("estate-a"))
        .set_recovery_policy(&SetRecoveryPolicy {
            context: context(at, operation, operation),
            instance_id: id("instance-a"),
            idempotency_key: operation.into(),
            max_rpo_ms: 1_000,
            max_rto_ms: 2_000,
            minimum_recovery_points: 1,
            retention_ms: 3_000,
        })
        .unwrap();
}

fn complete_backup<E: Engine>(engine: &E, job: &str, created_at: u64, hex: char) -> String {
    let repository = EstateRepository::new(engine, id("estate-a"));
    repository
        .schedule_backup(&ScheduleBackup {
            context: context(created_at, job, job),
            instance_id: id("instance-a"),
            idempotency_key: job.into(),
            label: job.into(),
        })
        .unwrap();
    repository
        .acquire_backup_lease(&BackupLeaseRequest {
            context: context(created_at + 10, &format!("lease-{job}"), job),
            worker: id("backup-worker"),
            lease_ms: 1_000,
        })
        .unwrap();
    repository
        .record_backup_prepared(&BackupPreparedRequest {
            context: context(created_at + 20, &format!("prepare-{job}"), job),
            worker: id("backup-worker"),
            lease_epoch: 1,
            evidence_sha256: "1".repeat(64),
        })
        .unwrap();
    let backup_id = hex.to_string().repeat(64);
    repository
        .record_backup_completed(&BackupCompleteRequest {
            context: context(created_at + 30, &format!("complete-{job}"), job),
            worker: id("backup-worker"),
            lease_epoch: 1,
            result: BackupResult {
                backup_id: backup_id.clone(),
                archive_sha256: hex
                    .to_ascii_uppercase()
                    .to_ascii_lowercase()
                    .to_string()
                    .repeat(64),
                catalogue_sha256: "f".repeat(64),
                evidence_sha256: "2".repeat(64),
            },
        })
        .unwrap();
    backup_id
}

#[test]
fn policy_backup_point_and_public_posture_survive_reopen_and_replay() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("estate-native");
    {
        let engine = NativeEngine::open(&database).unwrap();
        prepare_stopped_instance(&engine);
        set_policy(&engine, 75, "set-policy");
        let backup_id = complete_backup(&engine, "backup-one", 80, '3');
        let document = EstateRepository::new(&engine, id("estate-a"))
            .load()
            .unwrap()
            .unwrap();
        let job = &document.backup_jobs["backup-one"];
        assert_eq!(job.recovery_policy.as_ref().unwrap().revision, 1);
        assert_eq!(document.recovery_points[&backup_id].policy_revision, 1);
        assert!(document.recovery_pins.values().any(|pin| {
            pin.backup_id == backup_id && pin.kind == rrd_estate::EstateRetentionPinKind::Policy
        }));
        let public = public_recovery_snapshot(&document);
        assert_eq!(public.policies.len(), 1);
        assert_eq!(public.recovery_points.len(), 1);
        assert!(!serde_json::to_string(&public).unwrap().contains("path"));
    }

    let engine = NativeEngine::open(&database).unwrap();
    let replay = EstateRepository::new(&engine, id("estate-a"))
        .set_recovery_policy(&SetRecoveryPolicy {
            context: context(75, "set-policy", "set-policy"),
            instance_id: id("instance-a"),
            idempotency_key: "set-policy".into(),
            max_rpo_ms: 1_000,
            max_rto_ms: 2_000,
            minimum_recovery_points: 1,
            retention_ms: 3_000,
        })
        .unwrap();
    assert!(replay.idempotent_replay);
    let mut drift = SetRecoveryPolicy {
        context: context(75, "set-policy", "set-policy"),
        instance_id: id("instance-a"),
        idempotency_key: "set-policy".into(),
        max_rpo_ms: 1_000,
        max_rto_ms: 2_000,
        minimum_recovery_points: 1,
        retention_ms: 3_001,
    };
    assert!(matches!(
        EstateRepository::new(&engine, id("estate-a")).set_recovery_policy(&drift),
        Err(Error::IdempotencyConflict(_))
    ));
    drift.idempotency_key = "invalid-policy".into();
    drift.context = context(120, "invalid-policy", "invalid-policy");
    drift.max_rpo_ms = 0;
    assert!(matches!(
        EstateRepository::new(&engine, id("estate-a")).set_recovery_policy(&drift),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn explicit_holds_drive_deterministic_prune_evidence() {
    let engine = MemoryEngine::new();
    prepare_stopped_instance(&engine);
    set_policy(&engine, 75, "set-policy");
    let first = complete_backup(&engine, "backup-one", 80, '3');
    let second = complete_backup(&engine, "backup-two", 200, '4');
    let third = complete_backup(&engine, "backup-three", 320, '5');
    let repository = EstateRepository::new(&engine, id("estate-a"));
    let held = repository
        .pin_recovery_point(&PinRecoveryPoint {
            context: context(400, "hold-first", "hold-first"),
            idempotency_key: "hold-first".into(),
            backup_id: first.clone(),
            expires_at: None,
        })
        .unwrap();
    let decision = retention_decision(&held.document, &id("instance-a"), 4_000).unwrap();
    assert_eq!(
        decision.retained_backup_ids,
        vec![first.clone(), third.clone()]
    );
    assert_eq!(decision.prune_candidate_backup_ids, vec![second.clone()]);

    let released = repository
        .release_recovery_pin(&ReleaseRecoveryPin {
            context: context(4_100, "release-first", "release-first"),
            idempotency_key: "release-first".into(),
            pin_id: id("hold-first"),
        })
        .unwrap();
    let decision = retention_decision(&released.document, &id("instance-a"), 4_200).unwrap();
    assert_eq!(decision.retained_backup_ids, vec![third]);
    assert_eq!(
        decision.prune_candidate_backup_ids,
        vec![first.clone(), second.clone()]
    );
    let prepare = PrepareRecoveryPrune {
        context: context(4_300, "prepare-prune", "prepare-prune"),
        idempotency_key: "prepare-prune".into(),
        instance_id: id("instance-a"),
        expected_estate_revision: decision.estate_revision,
        evaluated_at: decision.evaluated_at,
        expected_catalogue_sha256: "a".repeat(64),
        retained_backup_ids: decision.retained_backup_ids,
        prune_candidate_backup_ids: decision.prune_candidate_backup_ids,
    };
    let prepared = repository.prepare_recovery_prune(&prepare).unwrap();
    assert_eq!(prepared.document.recovery_prune_intents.len(), 1);
    let blocked = repository
        .set_recovery_policy(&SetRecoveryPolicy {
            context: context(4_350, "blocked-policy", "blocked-policy"),
            instance_id: id("instance-a"),
            idempotency_key: "blocked-policy".into(),
            max_rpo_ms: 1_000,
            max_rto_ms: 2_000,
            minimum_recovery_points: 2,
            retention_ms: 3_000,
        })
        .unwrap_err();
    assert!(blocked.to_string().contains("active recovery prune intent"));
    let complete = CompleteRecoveryPrune {
        context: context(4_400, "complete-prune", "complete-prune"),
        idempotency_key: "complete-prune".into(),
        intent_id: id("prepare-prune"),
        resulting_catalogue_sha256: "b".repeat(64),
    };
    let recorded = repository.complete_recovery_prune(&complete).unwrap();
    assert_eq!(
        recorded.document.recovery_points[&first].pruned_at,
        Some(4_400)
    );
    assert_eq!(
        recorded.document.recovery_points[&second].pruned_at,
        Some(4_400)
    );
    assert!(recorded.document.recovery_prune_intents.is_empty());
    assert!(
        repository
            .complete_recovery_prune(&complete)
            .unwrap()
            .idempotent_replay
    );
}

#[test]
fn restore_evidence_records_measured_rpo_and_rto_without_paths() {
    let engine = MemoryEngine::new();
    prepare_stopped_instance(&engine);
    set_policy(&engine, 75, "set-policy");
    let backup_id = complete_backup(&engine, "backup-one", 80, '3');
    let repository = EstateRepository::new(&engine, id("estate-a"));
    let passing = repository
        .record_restore_evidence(&RecordRestoreEvidence {
            context: context(1_000, "restore-pass", "restore-pass"),
            idempotency_key: "restore-pass".into(),
            restore_id: id("restore-pass"),
            instance_id: id("instance-a"),
            backup_id: backup_id.clone(),
            started_at: 500,
            completed_at: 1_000,
            restored_claim_sequence: 7,
            restored_runtime_cursor: 9,
            closure_sha256: "a".repeat(64),
        })
        .unwrap();
    assert!(passing.document.restore_evidence["restore-pass"].rpo_within_objective);
    assert!(passing.document.restore_evidence["restore-pass"].rto_within_objective);
    let failing = repository
        .record_restore_evidence(&RecordRestoreEvidence {
            context: context(5_000, "restore-fail", "restore-fail"),
            idempotency_key: "restore-fail".into(),
            restore_id: id("restore-fail"),
            instance_id: id("instance-a"),
            backup_id,
            started_at: 2_000,
            completed_at: 5_000,
            restored_claim_sequence: 7,
            restored_runtime_cursor: 9,
            closure_sha256: "b".repeat(64),
        })
        .unwrap();
    assert!(!failing.document.restore_evidence["restore-fail"].rpo_within_objective);
    assert!(!failing.document.restore_evidence["restore-fail"].rto_within_objective);
}

#[test]
fn legacy_backup_jobs_decode_without_recovery_policy_snapshots() {
    let engine = MemoryEngine::new();
    prepare_stopped_instance(&engine);
    let scheduled = EstateRepository::new(&engine, id("estate-a"))
        .schedule_backup(&ScheduleBackup {
            context: context(80, "backup-one", "backup-one"),
            instance_id: id("instance-a"),
            idempotency_key: "backup-one".into(),
            label: "backup-one".into(),
        })
        .unwrap();
    let mut encoded = serde_json::to_value(scheduled.document).unwrap();
    encoded["backup_jobs"]["backup-one"]
        .as_object_mut()
        .unwrap()
        .remove("recovery_policy");
    let decoded: EstateDocument = serde_json::from_value(encoded).unwrap();
    assert!(decoded.backup_jobs["backup-one"].recovery_policy.is_none());
    decoded.validate().unwrap();
}
