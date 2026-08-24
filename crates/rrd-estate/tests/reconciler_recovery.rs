use rrd_contract::CanonicalId;
use rrd_estate::{
    DesiredPhase, DesiredTarget, DriverEffect, DriverError, DriverObservation, DriverRequest,
    EstateDriver, EstateRepository, MutationContext, ObservedPhase, OperationState,
    ReconcileBoundary, ReconcileOutcome, Reconciler, SetDesired,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use vyrm_core::digest;
use rrd_store::{Engine, NativeEngine};

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

fn desired(at: u64, operation: &str, version: &str) -> SetDesired {
    SetDesired {
        context: context(at, &format!("request-{operation}"), operation),
        instance_id: id("project-a"),
        idempotency_key: format!("key-{operation}"),
        target: DesiredTarget {
            phase: DesiredPhase::Running,
            deployment_ref: id("local-rrd"),
            version: version.into(),
            configuration_sha256: "a".repeat(64),
        },
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FakeWorldState {
    effects: BTreeMap<String, u32>,
    lost_acknowledgement: bool,
    versions: BTreeMap<String, String>,
}

struct DurableFakeDriver {
    path: PathBuf,
    inject_lost_acknowledgement: bool,
}

impl DurableFakeDriver {
    fn new(path: &Path, inject_lost_acknowledgement: bool) -> Self {
        Self {
            path: path.to_path_buf(),
            inject_lost_acknowledgement,
        }
    }

    fn load(&self) -> FakeWorldState {
        std::fs::read(&self.path)
            .ok()
            .map(|bytes| serde_json::from_slice(&bytes).unwrap())
            .unwrap_or_default()
    }

    fn store(&self, state: &FakeWorldState) {
        std::fs::write(&self.path, serde_json::to_vec(state).unwrap()).unwrap();
    }
}

impl EstateDriver for DurableFakeDriver {
    fn apply(&mut self, request: &DriverRequest) -> std::result::Result<DriverEffect, DriverError> {
        let mut world = self.load();
        let operation = request.operation_id.to_string();
        if !world.effects.contains_key(&operation) {
            world.effects.insert(operation.clone(), 1);
            world.versions.insert(
                request.instance_id.to_string(),
                request.desired.version.clone(),
            );
            if self.inject_lost_acknowledgement && !world.lost_acknowledgement {
                world.lost_acknowledgement = true;
                self.store(&world);
                return Err(DriverError::retryable(
                    "effect committed but acknowledgement was lost",
                    digest::sha256_hex(format!("lost:{operation}").as_bytes()),
                ));
            }
            self.store(&world);
        }
        Ok(DriverEffect {
            evidence_sha256: digest::sha256_hex(format!("effect:{operation}").as_bytes()),
        })
    }

    fn observe(
        &mut self,
        request: &DriverRequest,
    ) -> std::result::Result<DriverObservation, DriverError> {
        let world = self.load();
        let version = world.versions.get(request.instance_id.as_str()).cloned();
        let phase = if version.is_some() {
            ObservedPhase::Running
        } else {
            ObservedPhase::Absent
        };
        Ok(DriverObservation {
            phase,
            version,
            process_id: Some(42),
            evidence_sha256: digest::sha256_hex(
                format!("observe:{}:{phase:?}", request.operation_id).as_bytes(),
            ),
            error: None,
        })
    }
}

fn run_one_step(
    database: &Path,
    world: &Path,
    worker: &str,
    at: u64,
    inject_lost_acknowledgement: bool,
) -> ReconcileOutcome {
    let engine = NativeEngine::open(database).unwrap();
    let mut reconciler = Reconciler::new(
        &engine,
        id("estate-a"),
        id(worker),
        1_000,
        DurableFakeDriver::new(world, inject_lost_acknowledgement),
    )
    .unwrap();
    reconciler.step(at).unwrap()
}

fn assert_boundary(outcome: ReconcileOutcome, expected: ReconcileBoundary) {
    assert!(matches!(
        outcome,
        ReconcileOutcome::Advanced { boundary, .. } if boundary == expected
    ));
}

#[test]
fn reopen_between_every_boundary_converges_and_deduplicates_a_lost_ack() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("estate-native");
    let world = directory.path().join("fake-world.json");
    {
        let engine = NativeEngine::open(&database).unwrap();
        let repository = EstateRepository::new(&engine, id("estate-a"));
        repository
            .create(&context(10, "create-estate", "create-estate"))
            .unwrap();
        repository
            .set_desired(&desired(20, "deploy-project-a", "0.1.0"))
            .unwrap();
    }

    assert_boundary(
        run_one_step(&database, &world, "worker-one", 30, true),
        ReconcileBoundary::LeaseAcquired,
    );
    assert_boundary(
        run_one_step(&database, &world, "worker-one", 40, true),
        ReconcileBoundary::Prepared,
    );
    assert!(matches!(
        run_one_step(&database, &world, "worker-one", 50, true),
        ReconcileOutcome::Deferred { .. }
    ));
    assert_boundary(
        run_one_step(&database, &world, "worker-one", 60, true),
        ReconcileBoundary::Applied,
    );
    assert_boundary(
        run_one_step(&database, &world, "worker-one", 70, true),
        ReconcileBoundary::Observed,
    );
    assert_boundary(
        run_one_step(&database, &world, "worker-one", 80, true),
        ReconcileBoundary::Completed,
    );
    assert_eq!(
        run_one_step(&database, &world, "worker-one", 90, true),
        ReconcileOutcome::Idle
    );

    let fake_world: FakeWorldState =
        serde_json::from_slice(&std::fs::read(&world).unwrap()).unwrap();
    assert_eq!(fake_world.effects.get("deploy-project-a"), Some(&1));

    let reopened = NativeEngine::open(&database).unwrap();
    let repository = EstateRepository::new(&reopened, id("estate-a"));
    let document = repository.load().unwrap().unwrap();
    let operation = document.operation(&id("deploy-project-a")).unwrap();
    assert_eq!(operation.state, OperationState::Succeeded);
    assert_eq!(operation.attempts, 1);
    assert_eq!(operation.receipts.len(), 3);
    assert_eq!(document.revision, 7);
    let journal = reopened.control_journal_since(0, 20).unwrap();
    assert_eq!(journal.len(), 7);
    assert!(journal.iter().all(|entry| entry.verify()));
}

#[test]
fn takeover_waits_for_expiry_and_preserves_the_prepared_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("estate-native");
    let world = directory.path().join("fake-world.json");
    {
        let engine = NativeEngine::open(&database).unwrap();
        let repository = EstateRepository::new(&engine, id("estate-a"));
        repository
            .create(&context(10, "create-estate", "create-estate"))
            .unwrap();
        repository
            .set_desired(&desired(20, "deploy-project-a", "0.1.0"))
            .unwrap();
    }
    assert_boundary(
        run_one_step(&database, &world, "worker-one", 30, false),
        ReconcileBoundary::LeaseAcquired,
    );
    assert_boundary(
        run_one_step(&database, &world, "worker-one", 40, false),
        ReconcileBoundary::Prepared,
    );
    assert!(matches!(
        run_one_step(&database, &world, "worker-two", 100, false),
        ReconcileOutcome::WaitingForLease {
            expires_at: 1_030,
            ..
        }
    ));
    assert_boundary(
        run_one_step(&database, &world, "worker-two", 1_030, false),
        ReconcileBoundary::LeaseAcquired,
    );

    let engine = NativeEngine::open(&database).unwrap();
    let repository = EstateRepository::new(&engine, id("estate-a"));
    let document = repository.load().unwrap().unwrap();
    let operation = document.operation(&id("deploy-project-a")).unwrap();
    assert_eq!(operation.state, OperationState::Prepared);
    assert_eq!(operation.attempts, 2);
    assert_eq!(operation.lease.as_ref().unwrap().epoch, 2);
    drop(repository);
    drop(engine);

    assert_boundary(
        run_one_step(&database, &world, "worker-two", 1_040, false),
        ReconcileBoundary::Applied,
    );
    let fake_world: FakeWorldState =
        serde_json::from_slice(&std::fs::read(&world).unwrap()).unwrap();
    assert_eq!(fake_world.effects.get("deploy-project-a"), Some(&1));
}

#[test]
fn a_new_desired_generation_supersedes_unfinished_work() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("estate-native");
    let world = directory.path().join("fake-world.json");
    {
        let engine = NativeEngine::open(&database).unwrap();
        let repository = EstateRepository::new(&engine, id("estate-a"));
        repository
            .create(&context(10, "create-estate", "create-estate"))
            .unwrap();
        repository
            .set_desired(&desired(20, "deploy-v1", "0.1.0"))
            .unwrap();
        repository
            .set_desired(&desired(30, "deploy-v2", "0.2.0"))
            .unwrap();
        let document = repository.load().unwrap().unwrap();
        assert_eq!(
            document.operation(&id("deploy-v1")).unwrap().state,
            OperationState::Superseded
        );
    }

    assert!(matches!(
        run_one_step(&database, &world, "worker-one", 40, false),
        ReconcileOutcome::Advanced { operation_id, boundary: ReconcileBoundary::LeaseAcquired, .. }
            if operation_id == id("deploy-v2")
    ));
}
