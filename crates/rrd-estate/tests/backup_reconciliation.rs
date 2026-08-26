use rrd_contract::CanonicalId;
use rrd_core::digest;
use rrd_estate::{
    BackupCompleteRequest, BackupDriverRequest, BackupJobState, BackupLeaseRequest,
    BackupReconcileBoundary, BackupReconcileOutcome, BackupReconciler, BackupResult, DesiredPhase,
    DesiredTarget, DriverError, Error, EstateBackupDriver, EstateRepository, LeaseRequest,
    LocalEstateBackupDriver, MutationContext, ObservationRequest, ObservedPhase, ReceiptBoundary,
    ReceiptRequest, ScheduleBackup, SetDesired,
};
use rrd_store::{verify_backup_catalogue, Engine, NativeEngine, PersistentEngine};
use std::fs;
use std::path::{Path, PathBuf};

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

fn prepare_backup_job<E: Engine>(engine: &E) {
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
    for (at, request, boundary, evidence) in [
        (40, "prepare-stop", ReceiptBoundary::Prepared, "b"),
        (50, "apply-stop", ReceiptBoundary::Applied, "c"),
    ] {
        repository
            .record_receipt(&ReceiptRequest {
                context: context(at, request, "stop-instance"),
                worker: id("worker-one"),
                lease_epoch: 1,
                boundary,
                evidence_sha256: evidence.repeat(64),
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
    repository
        .schedule_backup(&ScheduleBackup {
            context: context(80, "backup-daily", "backup-daily"),
            instance_id: id("instance-a"),
            idempotency_key: "backup-daily".into(),
            label: "daily.0001".into(),
        })
        .unwrap();
}

fn assert_advanced(outcome: BackupReconcileOutcome, boundary: BackupReconcileBoundary) {
    assert!(matches!(
        outcome,
        BackupReconcileOutcome::Advanced {
            boundary: actual,
            ..
        } if actual == boundary
    ));
}

struct LostAckDriver {
    marker: PathBuf,
}

impl LostAckDriver {
    fn result(request: &BackupDriverRequest) -> BackupResult {
        BackupResult {
            backup_id: digest::sha256_hex(format!("backup:{}", request.job_id).as_bytes()),
            archive_sha256: digest::sha256_hex(b"archive"),
            catalogue_sha256: digest::sha256_hex(b"catalogue"),
            evidence_sha256: digest::sha256_hex(b"completion"),
        }
    }
}

impl EstateBackupDriver for LostAckDriver {
    fn create(
        &mut self,
        request: &BackupDriverRequest,
    ) -> std::result::Result<BackupResult, DriverError> {
        if self.marker.exists() {
            return Ok(Self::result(request));
        }
        fs::write(&self.marker, request.sha256()).unwrap();
        Err(DriverError::retryable(
            "effect committed before acknowledgement",
            digest::sha256_hex(b"lost-ack"),
        ))
    }
}

#[test]
fn prepared_job_replays_one_durable_effect_after_reopen() {
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("estate-native");
    let marker = temporary.path().join("effect.marker");
    {
        let engine = NativeEngine::open(&database).unwrap();
        prepare_backup_job(&engine);
        let mut reconciler = BackupReconciler::new(
            &engine,
            id("estate-a"),
            id("backup-worker"),
            1_000,
            LostAckDriver {
                marker: marker.clone(),
            },
        )
        .unwrap();
        assert_advanced(
            reconciler.step(90).unwrap(),
            BackupReconcileBoundary::LeaseAcquired,
        );
        assert_advanced(
            reconciler.step(100).unwrap(),
            BackupReconcileBoundary::Prepared,
        );
        assert!(matches!(
            reconciler.step(110).unwrap(),
            BackupReconcileOutcome::Deferred { .. }
        ));
        assert!(marker.is_file());
        let document = EstateRepository::new(&engine, id("estate-a"))
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(
            document.backup_jobs["backup-daily"].state,
            BackupJobState::Prepared
        );
    }

    let engine = NativeEngine::open(&database).unwrap();
    let mut reconciler = BackupReconciler::new(
        &engine,
        id("estate-a"),
        id("backup-worker"),
        1_000,
        LostAckDriver {
            marker: marker.clone(),
        },
    )
    .unwrap();
    assert_advanced(
        reconciler.step(120).unwrap(),
        BackupReconcileBoundary::Completed,
    );
    assert!(matches!(
        reconciler.step(130).unwrap(),
        BackupReconcileOutcome::Idle
    ));
    assert_eq!(fs::read_to_string(marker).unwrap().len(), 64);
    let job = &EstateRepository::new(&engine, id("estate-a"))
        .load()
        .unwrap()
        .unwrap()
        .backup_jobs["backup-daily"];
    assert_eq!(job.state, BackupJobState::Succeeded);
    assert_eq!(job.attempts, 1);
}

#[test]
fn expired_prepared_lease_is_taken_over_and_stale_worker_is_fenced() {
    let engine = rrd_store::MemoryEngine::new();
    prepare_backup_job(&engine);
    let repository = EstateRepository::new(&engine, id("estate-a"));
    repository
        .acquire_backup_lease(&BackupLeaseRequest {
            context: context(90, "lease-backup-one", "backup-daily"),
            worker: id("worker-one"),
            lease_ms: 1_000,
        })
        .unwrap();
    repository
        .record_backup_prepared(&rrd_estate::BackupPreparedRequest {
            context: context(100, "prepare-backup-one", "backup-daily"),
            worker: id("worker-one"),
            lease_epoch: 1,
            evidence_sha256: "f".repeat(64),
        })
        .unwrap();
    let takeover = repository
        .acquire_backup_lease(&BackupLeaseRequest {
            context: context(1_100, "lease-backup-two", "backup-daily"),
            worker: id("worker-two"),
            lease_ms: 1_000,
        })
        .unwrap();
    let job = &takeover.backup_jobs["backup-daily"];
    assert_eq!(job.state, BackupJobState::Prepared);
    assert_eq!(job.attempts, 2);
    assert_eq!(job.lease.as_ref().unwrap().epoch, 2);

    let result = BackupResult {
        backup_id: "1".repeat(64),
        archive_sha256: "2".repeat(64),
        catalogue_sha256: "3".repeat(64),
        evidence_sha256: "4".repeat(64),
    };
    let stale = repository
        .record_backup_completed(&BackupCompleteRequest {
            context: context(1_101, "complete-stale", "backup-daily"),
            worker: id("worker-one"),
            lease_epoch: 1,
            result: result.clone(),
        })
        .unwrap_err();
    assert!(matches!(stale, Error::StaleLease(_)));
    let completed = repository
        .record_backup_completed(&BackupCompleteRequest {
            context: context(1_102, "complete-new", "backup-daily"),
            worker: id("worker-two"),
            lease_epoch: 2,
            result,
        })
        .unwrap();
    assert_eq!(
        completed.backup_jobs["backup-daily"].state,
        BackupJobState::Succeeded
    );
}

fn local_request(instance: &str, job: &str) -> BackupDriverRequest {
    BackupDriverRequest {
        estate_id: id("estate-a"),
        job_id: id(job),
        instance_id: id(instance),
        source_generation: 1,
        label: "daily.0001".into(),
        created_at: 100,
    }
}

fn create_state_root(parent: &Path) -> PathBuf {
    let root = parent.join("state");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("instances")).unwrap();
    fs::create_dir(root.join("processes")).unwrap();
    fs::canonicalize(root).unwrap()
}

#[test]
fn local_driver_creates_one_authenticated_catalogue_entry_on_replay() {
    let temporary = tempfile::tempdir().unwrap();
    let state_root = create_state_root(temporary.path());
    let source = state_root.join("instances/instance-a/.rrflow/rrd");
    let engine = PersistentEngine::open(&source).unwrap();
    drop(engine);
    let request = local_request("instance-a", "backup-daily");
    let mut driver = LocalEstateBackupDriver::new(&state_root).unwrap();

    let first = driver.create(&request).unwrap();
    let replay = driver.create(&request).unwrap();
    assert_eq!(replay, first);
    let catalogue = verify_backup_catalogue(&driver.catalogue_path(&request)).unwrap();
    assert_eq!(catalogue.revision, 1);
    assert_eq!(catalogue.backups.len(), 1);
    assert_eq!(catalogue.backups[0].backup_id, first.backup_id);
}

#[test]
fn local_driver_denies_process_record_before_catalogue_creation() {
    let temporary = tempfile::tempdir().unwrap();
    let state_root = create_state_root(temporary.path());
    let source = state_root.join("instances/instance-a/.rrflow/rrd");
    let engine = PersistentEngine::open(&source).unwrap();
    drop(engine);
    fs::write(state_root.join("processes/instance-a.json"), b"retained").unwrap();
    let request = local_request("instance-a", "backup-daily");
    let mut driver = LocalEstateBackupDriver::new(&state_root).unwrap();

    let error = driver.create(&request).unwrap_err();
    assert_eq!(error.kind, rrd_estate::DriverErrorKind::Permanent);
    assert!(error.message.contains("retained process record"));
    assert!(!driver.catalogue_path(&request).exists());
}
