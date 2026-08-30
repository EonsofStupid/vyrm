use rrd_contract::CanonicalId;
use rrd_estate::{
    DesiredInstance, DesiredPhase, DesiredTarget, DriverErrorKind, DriverRequest, EstateDriver,
    EstateRepository, LocalArgument, LocalDeployment, LocalDeploymentCatalog, LocalProcessDriver,
    LocalReadiness, LocalShutdown, MutationContext, OperationKind, OperationState,
    ReconcileBoundary, ReconcileOutcome, Reconciler, SetDesired, LOCAL_DEPLOYMENT_FORMAT,
};
use rrd_store::PersistentEngine;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const DIAGNOSTIC_STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

fn id(value: &str) -> CanonicalId {
    CanonicalId::new(value).unwrap()
}

fn context(at: u64, operation: &str) -> MutationContext {
    MutationContext {
        at,
        actor: "rrd-estate".into(),
        request_id: format!("request-{operation}"),
        operation_id: id(operation),
    }
}

fn target(phase: DesiredPhase) -> DesiredTarget {
    DesiredTarget {
        phase,
        deployment_ref: id("rrd-server"),
        version: "0.1.0".into(),
        configuration_sha256: "a".repeat(64),
    }
}

fn file_sha256(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap();
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn workspace_binary(name: &str) -> PathBuf {
    let variable = format!("CARGO_BIN_EXE_{name}");
    if let Some(path) = std::env::var_os(variable) {
        return std::fs::canonicalize(path).unwrap();
    }
    let test_executable = std::env::current_exe().unwrap();
    let profile_root = test_executable
        .parent()
        .and_then(Path::parent)
        .expect("integration test executable must be under the Cargo profile directory");
    let candidate = profile_root.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    std::fs::canonicalize(&candidate).unwrap_or_else(|error| {
        panic!(
            "required workspace binary {} is not built at {}: {error}",
            name,
            candidate.display()
        )
    })
}

struct TestCatalog {
    _snapshot_directory: tempfile::TempDir,
    value: LocalDeploymentCatalog,
}

fn catalog() -> LocalDeploymentCatalog {
    static CATALOG: OnceLock<TestCatalog> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let cargo_executable = std::fs::canonicalize(env!("CARGO_BIN_EXE_rrd-server")).unwrap();
            let snapshot_directory =
                tempfile::tempdir_in(cargo_executable.parent().unwrap()).unwrap();
            let executable = snapshot_directory
                .path()
                .join(cargo_executable.file_name().unwrap());
            std::fs::hard_link(&cargo_executable, &executable).unwrap();
            let executable = std::fs::canonicalize(executable).unwrap();
            let value = LocalDeploymentCatalog {
                format: LOCAL_DEPLOYMENT_FORMAT,
                deployments: BTreeMap::from([(
                    "rrd-server".into(),
                    LocalDeployment {
                        id: id("rrd-server"),
                        version: "0.1.0".into(),
                        executable_sha256: file_sha256(&executable),
                        executable,
                        preparation_arguments: vec![
                            LocalArgument::Literal("initialize".into()),
                            LocalArgument::Literal("--root".into()),
                            LocalArgument::InstanceRoot,
                            LocalArgument::Literal("--instance".into()),
                            LocalArgument::InstanceId,
                        ],
                        arguments: vec![
                            LocalArgument::Literal("--root".into()),
                            LocalArgument::InstanceRoot,
                            LocalArgument::Literal("--bind".into()),
                            LocalArgument::Literal("127.0.0.1:0".into()),
                            LocalArgument::Literal("--ready-file".into()),
                            LocalArgument::InstancePath(PathBuf::from("RRD.READY")),
                            LocalArgument::Literal("--shutdown-request-file".into()),
                            LocalArgument::InstancePath(PathBuf::from("SHUTDOWN.REQUEST")),
                            LocalArgument::Literal("--shutdown-complete-file".into()),
                            LocalArgument::InstancePath(PathBuf::from("SHUTDOWN.COMPLETE")),
                        ],
                        environment: BTreeMap::new(),
                        readiness: LocalReadiness::File {
                            path: PathBuf::from("RRD.READY"),
                            timeout_ms: 90_000,
                        },
                        shutdown: LocalShutdown::RequestFile {
                            request: PathBuf::from("SHUTDOWN.REQUEST"),
                            complete: PathBuf::from("SHUTDOWN.COMPLETE"),
                            timeout_ms: 5_000,
                        },
                    },
                )]),
            };
            TestCatalog {
                _snapshot_directory: snapshot_directory,
                value,
            }
        })
        .value
        .clone()
}

fn step(database: &Path, state_root: &Path, at: u64) -> ReconcileOutcome {
    let engine = PersistentEngine::open(database).unwrap();
    let driver = LocalProcessDriver::new(state_root, catalog()).unwrap();
    let mut reconciler =
        Reconciler::new(&engine, id("estate-a"), id("worker-one"), 30_000, driver).unwrap();
    reconciler.step(at).unwrap()
}

fn assert_boundary(outcome: ReconcileOutcome, expected: ReconcileBoundary) {
    let observed = format!("{outcome:?}");
    assert!(
        matches!(
            outcome,
            ReconcileOutcome::Advanced { boundary, .. } if boundary == expected
        ),
        "expected {expected:?}, observed {observed}"
    );
}

fn process_pid(state_root: &Path) -> Option<u32> {
    let path = state_root.join("processes/project-a.json");
    std::fs::read(path)
        .ok()
        .map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).unwrap())
        .and_then(|record| record["pid"].as_u64())
        .map(|pid| u32::try_from(pid).unwrap())
}

fn process_exists(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::nothing().without_tasks(),
    );
    system.process(Pid::from_u32(pid)).is_some()
}

fn wait_for_file_text(path: &Path, expected: &str) -> bool {
    let deadline = Instant::now() + DIAGNOSTIC_STARTUP_TIMEOUT;
    loop {
        if std::fs::read_to_string(path).is_ok_and(|text| text.contains(expected)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn run_controller_and_kill(
    database: &Path,
    state_root: &Path,
    catalog_path: &Path,
    marker: &Path,
    at: u64,
    after_effect: bool,
) {
    let marker = marker.with_extension(format!("{at}.held"));
    let hold_variable = if after_effect {
        "RRD_ESTATE_TEST_HOLD_AFTER_EFFECT_FILE"
    } else {
        "RRD_ESTATE_TEST_HOLD_AFTER_STEP_FILE"
    };
    let deadline = Instant::now() + Duration::from_secs(90);
    'retry: loop {
        let _ = std::fs::remove_file(&marker);
        let _ = std::fs::remove_file(marker.with_extension("new"));
        let mut child = Command::new(workspace_binary("rrd-estate-controller"))
            .args([
                "--db",
                database.to_str().unwrap(),
                "--authority-instance",
                "estate-control-instance",
                "--state-root",
                state_root.to_str().unwrap(),
                "--catalog",
                catalog_path.to_str().unwrap(),
                "--estate",
                "estate-a",
                "--worker",
                "worker-one",
                "--lease-ms",
                "30000",
                "--at",
                &at.to_string(),
            ])
            .env(hold_variable, &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        while !marker.is_file() {
            if let Some(status) = child.try_wait().unwrap() {
                let mut output = String::new();
                let mut error = String::new();
                std::io::Read::read_to_string(child.stdout.as_mut().unwrap(), &mut output).unwrap();
                std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut error).unwrap();
                let deferred = status.success()
                    && serde_json::from_str::<ReconcileOutcome>(output.trim())
                        .is_ok_and(|outcome| matches!(outcome, ReconcileOutcome::Deferred { .. }));
                if deferred && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(10));
                    continue 'retry;
                }
                let engine = PersistentEngine::open(database).unwrap();
                let repository = EstateRepository::new(&engine, id("estate-a"));
                let document = repository.load().unwrap().unwrap();
                let operation_failures = document
                    .operations
                    .values()
                    .filter_map(|operation| {
                        operation
                            .error
                            .as_ref()
                            .map(|error| format!("{}: {error}", operation.id))
                    })
                    .collect::<Vec<_>>();
                let process_record =
                    std::fs::read_to_string(state_root.join("processes/project-a.json")).ok();
                panic!(
                    "estate controller exited before hold marker at {at} ({status}); stdout={output:?}; stderr={error:?}; operation_failures={operation_failures:?}; process_record={process_record:?}"
                );
            }
            assert!(
                Instant::now() < deadline,
                "estate controller did not reach hold boundary at {at}"
            );
            thread::sleep(Duration::from_millis(10));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        return;
    }
}

fn operation_state(database: &Path, operation: &str) -> (OperationState, u64) {
    let engine = PersistentEngine::open(database).unwrap();
    let repository = EstateRepository::new(&engine, id("estate-a"));
    let document = repository.load().unwrap().unwrap();
    (
        document.operation(&id(operation)).unwrap().state,
        document.revision,
    )
}

struct ProcessCleanup {
    state_root: PathBuf,
}

impl Drop for ProcessCleanup {
    fn drop(&mut self) {
        if let Some(pid) = process_pid(&self.state_root) {
            let mut system = System::new();
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
                true,
                ProcessRefreshKind::nothing().without_tasks(),
            );
            if let Some(process) = system.process(Pid::from_u32(pid)) {
                let _ = process.kill();
                let _ = process.wait();
            }
        }
    }
}

#[test]
fn real_rrd_child_survives_controller_reopen_and_stops_without_data_deletion() {
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("estate-authority");
    let state_root = temporary.path().join("local-processes");
    std::fs::create_dir(&state_root).unwrap();
    let state_root = std::fs::canonicalize(state_root).unwrap();
    let _cleanup = ProcessCleanup {
        state_root: state_root.clone(),
    };
    {
        let engine = PersistentEngine::open(&database).unwrap();
        let repository = EstateRepository::new(&engine, id("estate-a"));
        repository.create(&context(10, "create-estate")).unwrap();
        repository
            .set_desired(&SetDesired {
                context: context(20, "start-project-a"),
                instance_id: id("project-a"),
                idempotency_key: "start-project-a".into(),
                target: target(DesiredPhase::Running),
            })
            .unwrap();
    }

    assert_boundary(
        step(&database, &state_root, 30),
        ReconcileBoundary::LeaseAcquired,
    );
    assert_boundary(
        step(&database, &state_root, 40),
        ReconcileBoundary::Prepared,
    );
    assert_boundary(step(&database, &state_root, 50), ReconcileBoundary::Applied);
    let started_pid = process_pid(&state_root).expect("driver persisted process identity");
    assert!(process_exists(started_pid));

    let replay_driver = LocalProcessDriver::new(&state_root, catalog()).unwrap();
    let request = rrd_estate::DriverRequest {
        estate_id: id("estate-a"),
        instance_id: id("project-a"),
        operation_id: id("start-project-a"),
        kind: rrd_estate::OperationKind::Provision,
        desired: rrd_estate::DesiredInstance {
            generation: 1,
            phase: DesiredPhase::Running,
            deployment_ref: id("rrd-server"),
            version: "0.1.0".into(),
            configuration_sha256: "a".repeat(64),
            updated_at: 20,
        },
    };
    let mut replay_driver = replay_driver;
    rrd_estate::EstateDriver::apply(&mut replay_driver, &request).unwrap();
    assert_eq!(process_pid(&state_root), Some(started_pid));
    let instance_root = state_root.join("instances/project-a");
    assert!(instance_root.join("RRD.PROCESS.STDOUT.LOG").is_file());
    let readiness: serde_json::Value = serde_json::from_slice(
        &std::fs::read(instance_root.join("RRD.READY")).expect("driver waits for readiness"),
    )
    .unwrap();
    assert_eq!(readiness["status"], "ready");
    assert!(readiness["url"]
        .as_str()
        .is_some_and(|url| url.starts_with("http://127.0.0.1:")));
    assert!(
        wait_for_file_text(
            &instance_root.join("RRD.PROCESS.STDERR.LOG"),
            "rrd-server: http://127.0.0.1:"
        ),
        "the per-instance diagnostic log must retain the server startup boundary"
    );

    assert_boundary(
        step(&database, &state_root, 60),
        ReconcileBoundary::Observed,
    );
    assert_boundary(
        step(&database, &state_root, 70),
        ReconcileBoundary::Completed,
    );
    {
        let engine = PersistentEngine::open(&database).unwrap();
        let repository = EstateRepository::new(&engine, id("estate-a"));
        repository
            .set_desired(&SetDesired {
                context: context(80, "stop-project-a"),
                instance_id: id("project-a"),
                idempotency_key: "stop-project-a".into(),
                target: target(DesiredPhase::Stopped),
            })
            .unwrap();
    }
    assert_boundary(
        step(&database, &state_root, 90),
        ReconcileBoundary::LeaseAcquired,
    );
    assert_boundary(
        step(&database, &state_root, 100),
        ReconcileBoundary::Prepared,
    );
    assert_boundary(
        step(&database, &state_root, 110),
        ReconcileBoundary::Applied,
    );
    assert!(!process_exists(started_pid));
    assert!(
        state_root
            .join("instances/project-a/SHUTDOWN.COMPLETE")
            .is_file(),
        "managed RRD child must confirm graceful shutdown"
    );
    assert!(state_root.join("instances/project-a/.rrflow/rrd").is_dir());
    assert_boundary(
        step(&database, &state_root, 120),
        ReconcileBoundary::Observed,
    );
    assert_boundary(
        step(&database, &state_root, 130),
        ReconcileBoundary::Completed,
    );
    assert_eq!(process_pid(&state_root), None);
}

#[test]
fn local_driver_refuses_to_signal_a_reused_or_forged_pid_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let state_root = temporary.path().join("local-processes");
    std::fs::create_dir(&state_root).unwrap();
    let state_root = std::fs::canonicalize(state_root).unwrap();
    let driver = LocalProcessDriver::new(&state_root, catalog()).unwrap();
    let record_path = driver.process_record_path(&id("project-a"));
    std::fs::create_dir_all(record_path.parent().unwrap()).unwrap();
    let current_pid = std::process::id();
    let current_executable = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(current_pid)]),
        true,
        ProcessRefreshKind::nothing().without_tasks(),
    );
    let current_start = system
        .process(Pid::from_u32(current_pid))
        .unwrap()
        .start_time();
    std::fs::write(
        &record_path,
        serde_json::to_vec(&serde_json::json!({
            "format": 1,
            "instance_id": "project-a",
            "operation_id": "old-operation",
            "deployment_ref": "rrd-server",
            "version": "0.1.0",
            "configuration_sha256": "a".repeat(64),
            "executable": current_executable,
            "executable_sha256": "b".repeat(64),
            "pid": current_pid,
            "process_start_time_unix_s": current_start.saturating_add(1)
        }))
        .unwrap(),
    )
    .unwrap();
    let request = DriverRequest {
        estate_id: id("estate-a"),
        instance_id: id("project-a"),
        operation_id: id("stop-project-a"),
        kind: OperationKind::Stop,
        desired: DesiredInstance {
            generation: 2,
            phase: DesiredPhase::Stopped,
            deployment_ref: id("rrd-server"),
            version: "0.1.0".into(),
            configuration_sha256: "a".repeat(64),
            updated_at: 20,
        },
    };
    let mut driver = driver;
    let error = driver.apply(&request).unwrap_err();
    assert_eq!(error.kind, DriverErrorKind::Permanent);
    assert!(error.message.contains("refusing to signal"));
    assert!(process_exists(current_pid));
    assert!(record_path.exists());
}

#[test]
fn graceful_timeout_reauthenticates_then_uses_the_bounded_kill_fallback() {
    let temporary = tempfile::tempdir().unwrap();
    let state_root = temporary.path().join("local-processes");
    std::fs::create_dir(&state_root).unwrap();
    let state_root = std::fs::canonicalize(state_root).unwrap();
    let _cleanup = ProcessCleanup {
        state_root: state_root.clone(),
    };
    let mut fallback_catalog = catalog();
    let deployment = fallback_catalog.deployments.get_mut("rrd-server").unwrap();
    deployment.shutdown = LocalShutdown::RequestFile {
        request: PathBuf::from("SHUTDOWN.REQUEST"),
        complete: PathBuf::from("SHUTDOWN.COMPLETE"),
        timeout_ms: 100,
    };
    let watched_request = deployment
        .arguments
        .iter_mut()
        .find(|argument| {
            matches!(
                argument,
                LocalArgument::InstancePath(path) if path == Path::new("SHUTDOWN.REQUEST")
            )
        })
        .unwrap();
    *watched_request = LocalArgument::InstancePath(PathBuf::from("IGNORED.REQUEST"));
    let mut driver = LocalProcessDriver::new(&state_root, fallback_catalog).unwrap();
    let start = DriverRequest {
        estate_id: id("estate-a"),
        instance_id: id("project-a"),
        operation_id: id("start-fallback"),
        kind: OperationKind::Provision,
        desired: DesiredInstance {
            generation: 1,
            phase: DesiredPhase::Running,
            deployment_ref: id("rrd-server"),
            version: "0.1.0".into(),
            configuration_sha256: "a".repeat(64),
            updated_at: 10,
        },
    };
    driver.apply(&start).unwrap();
    let started_pid = process_pid(&state_root).unwrap();
    assert!(process_exists(started_pid));

    let mut stop = start;
    stop.operation_id = id("stop-fallback");
    stop.kind = OperationKind::Stop;
    stop.desired.generation = 2;
    stop.desired.phase = DesiredPhase::Stopped;
    stop.desired.updated_at = 20;
    driver.apply(&stop).unwrap();

    assert!(!process_exists(started_pid));
    assert_eq!(process_pid(&state_root), None);
    assert!(
        !state_root
            .join("instances/project-a/SHUTDOWN.COMPLETE")
            .exists(),
        "a forced fallback must not fabricate graceful completion"
    );
    assert!(state_root.join("instances/project-a/.rrflow/rrd").is_dir());
}

#[test]
fn controller_process_kill_matrix_converges_across_start_and_stop_effect_gaps() {
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("estate-authority");
    let state_root = temporary.path().join("local-processes");
    std::fs::create_dir(&state_root).unwrap();
    let state_root = std::fs::canonicalize(state_root).unwrap();
    let catalog_path = temporary.path().join("deployments.json");
    std::fs::write(&catalog_path, serde_json::to_vec(&catalog()).unwrap()).unwrap();
    let marker = temporary.path().join("controller-held");
    let _cleanup = ProcessCleanup {
        state_root: state_root.clone(),
    };
    {
        let engine = PersistentEngine::open(&database).unwrap();
        let repository = EstateRepository::new(&engine, id("estate-a"));
        repository.create(&context(10, "create-estate")).unwrap();
        repository
            .set_desired(&SetDesired {
                context: context(20, "start-project-a"),
                instance_id: id("project-a"),
                idempotency_key: "start-project-a".into(),
                target: target(DesiredPhase::Running),
            })
            .unwrap();
    }

    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 30, false);
    assert_eq!(
        operation_state(&database, "start-project-a"),
        (OperationState::Leased, 3)
    );
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 40, false);
    assert_eq!(
        operation_state(&database, "start-project-a"),
        (OperationState::Prepared, 4)
    );
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 50, true);
    assert_eq!(
        operation_state(&database, "start-project-a"),
        (OperationState::Prepared, 4)
    );
    let started_pid = process_pid(&state_root).expect("effect gap retained child identity");
    assert!(process_exists(started_pid));
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 60, false);
    assert_eq!(
        operation_state(&database, "start-project-a"),
        (OperationState::Applied, 5)
    );
    assert_eq!(process_pid(&state_root), Some(started_pid));
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 70, false);
    assert_eq!(
        operation_state(&database, "start-project-a"),
        (OperationState::Applied, 6)
    );
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 80, false);
    assert_eq!(
        operation_state(&database, "start-project-a"),
        (OperationState::Succeeded, 7)
    );
    assert!(
        wait_for_file_text(
            &state_root.join("instances/project-a/RRD.PROCESS.STDERR.LOG"),
            "rrd-server: http://127.0.0.1:"
        ),
        "the effect-gap fixture must observe server readiness before requesting shutdown"
    );

    {
        let engine = PersistentEngine::open(&database).unwrap();
        let repository = EstateRepository::new(&engine, id("estate-a"));
        repository
            .set_desired(&SetDesired {
                context: context(90, "stop-project-a"),
                instance_id: id("project-a"),
                idempotency_key: "stop-project-a".into(),
                target: target(DesiredPhase::Stopped),
            })
            .unwrap();
    }
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 100, false);
    assert_eq!(
        operation_state(&database, "stop-project-a"),
        (OperationState::Leased, 9)
    );
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 110, false);
    assert_eq!(
        operation_state(&database, "stop-project-a"),
        (OperationState::Prepared, 10)
    );
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 120, true);
    assert_eq!(
        operation_state(&database, "stop-project-a"),
        (OperationState::Prepared, 10)
    );
    assert!(!process_exists(started_pid));
    assert_eq!(process_pid(&state_root), None);
    assert!(
        state_root
            .join("instances/project-a/SHUTDOWN.COMPLETE")
            .is_file(),
        "effect-gap stop must complete through the graceful protocol"
    );
    assert!(state_root.join("instances/project-a/.rrflow/rrd").is_dir());
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 130, false);
    assert_eq!(
        operation_state(&database, "stop-project-a"),
        (OperationState::Applied, 11)
    );
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 140, false);
    assert_eq!(
        operation_state(&database, "stop-project-a"),
        (OperationState::Applied, 12)
    );
    run_controller_and_kill(&database, &state_root, &catalog_path, &marker, 150, false);
    assert_eq!(
        operation_state(&database, "stop-project-a"),
        (OperationState::Succeeded, 13)
    );
}
