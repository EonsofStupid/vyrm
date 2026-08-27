use rrd_contract::CanonicalId;
use rrd_estate::{
    BackupJobState, BackupReconcileOutcome, DesiredPhase, DesiredTarget, EstateRepository,
    LeaseRequest, MutationContext, ObservationRequest, ObservedPhase, ReceiptBoundary,
    ReceiptRequest, ScheduleBackup, SetDesired,
};
use rrd_store::{verify_backup_catalogue, NativeEngine, PersistentEngine};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

fn prepare(database: &Path, state_root: &Path) {
    let engine = NativeEngine::open(database).unwrap();
    let repository = EstateRepository::new(&engine, id("estate-a"));
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
            worker: id("estate-worker"),
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
                worker: id("estate-worker"),
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
            worker: id("estate-worker"),
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
            worker: id("estate-worker"),
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
    let source = state_root.join("instances/instance-a/.rrflow/rrd");
    drop(PersistentEngine::open(&source).unwrap());
}

fn command(database: &Path, state_root: &Path, at: u64) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rrd-backup-controller"));
    command.args([
        "--db",
        database.to_str().unwrap(),
        "--authority-instance",
        "estate-control-instance",
        "--state-root",
        state_root.to_str().unwrap(),
        "--estate",
        "estate-a",
        "--worker",
        "backup-worker",
        "--lease-ms",
        "30000",
        "--at",
        &at.to_string(),
    ]);
    command
}

fn run(database: &Path, state_root: &Path, at: u64) -> BackupReconcileOutcome {
    let output = command(database, state_root, at).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run_and_kill(database: &Path, state_root: &Path, marker: &Path, at: u64, after_effect: bool) {
    let marker = marker.with_extension(format!("{at}.held"));
    let variable = if after_effect {
        "RRD_BACKUP_TEST_HOLD_AFTER_EFFECT_FILE"
    } else {
        "RRD_BACKUP_TEST_HOLD_AFTER_STEP_FILE"
    };
    let mut child = command(database, state_root, at)
        .env(variable, &marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !marker.is_file() {
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "backup controller exited before hold boundary ({status}): {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert!(
            Instant::now() < deadline,
            "backup controller did not reach hold boundary at {at}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
}

fn job_state(database: &Path) -> (BackupJobState, u64) {
    let engine = PersistentEngine::open(database).unwrap();
    let document = EstateRepository::new(&engine, id("estate-a"))
        .load()
        .unwrap()
        .unwrap();
    (
        document.backup_jobs["backup-daily"].state,
        document.revision,
    )
}

#[test]
fn backup_controller_kill_matrix_converges_after_the_archive_effect_gap() {
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("estate-authority");
    let state_root = temporary.path().join("state");
    std::fs::create_dir(&state_root).unwrap();
    std::fs::create_dir(state_root.join("instances")).unwrap();
    std::fs::create_dir(state_root.join("processes")).unwrap();
    let state_root = std::fs::canonicalize(state_root).unwrap();
    let marker = temporary.path().join("backup-controller");
    prepare(&database, &state_root);

    run_and_kill(&database, &state_root, &marker, 90, false);
    assert_eq!(job_state(&database), (BackupJobState::Leased, 9));
    run_and_kill(&database, &state_root, &marker, 100, false);
    assert_eq!(job_state(&database), (BackupJobState::Prepared, 10));
    run_and_kill(&database, &state_root, &marker, 110, true);
    assert_eq!(job_state(&database), (BackupJobState::Prepared, 10));
    let catalogue_root = state_root.join("backups/instance-a");
    let after_effect = verify_backup_catalogue(&catalogue_root).unwrap();
    assert_eq!(after_effect.revision, 1);
    assert_eq!(after_effect.backups.len(), 1);

    run_and_kill(&database, &state_root, &marker, 120, false);
    assert_eq!(job_state(&database), (BackupJobState::Succeeded, 11));
    let replay = verify_backup_catalogue(&catalogue_root).unwrap();
    assert_eq!(replay.revision, 1);
    assert_eq!(replay.backups, after_effect.backups);
    assert!(matches!(
        run(&database, &state_root, 130),
        BackupReconcileOutcome::Idle
    ));
}
