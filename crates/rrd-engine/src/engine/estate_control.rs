use super::*;
use rrd_estate::{EstateBackupDriver, EstateDriver};
use std::fs;
use std::io::Write as _;

/// A bounded estate mutation accepted by the RRD composition root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EstateAdminAction {
    Create,
    SetDesired {
        instance: CanonicalId,
        idempotency_key: String,
        phase: rrd_contract::EstateDesiredPhase,
        deployment: CanonicalId,
        version: String,
        configuration_sha256: String,
    },
    ScheduleBackup {
        instance: CanonicalId,
        idempotency_key: String,
        label: String,
    },
}

/// Public result from an engine-owned estate administration operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum EstateAdminResult {
    Mutation(rrd_contract::EstateMutationResult),
    Backup(Box<rrd_contract::EstateBackupMutationResult>),
}

pub type EstateReconcileOutcome = rrd_estate::ReconcileOutcome;
pub type EstateBackupReconcileOutcome = rrd_estate::BackupReconcileOutcome;

impl RrdEngine {
    /// Authorizes and applies one estate mutation through a single local RRD
    /// authority. Policy denial happens before the database can be created.
    #[allow(clippy::too_many_arguments)]
    pub fn administer_estate_store(
        database: &Path,
        authority_instance: CanonicalId,
        policy_path: &Path,
        key_path: &Path,
        estate_id: CanonicalId,
        at: u64,
        request_id: String,
        operation_id: CanonicalId,
        action: EstateAdminAction,
    ) -> Result<EstateAdminResult> {
        let permission = match &action {
            EstateAdminAction::Create => rrd_estate::LocalEstatePermission::Create,
            EstateAdminAction::SetDesired { .. } => rrd_estate::LocalEstatePermission::SetDesired,
            EstateAdminAction::ScheduleBackup { .. } => {
                rrd_estate::LocalEstatePermission::ScheduleBackup
            }
        };
        let policy = rrd_estate::LocalOperatorPolicy::load_json(policy_path)
            .map_err(ServiceError::Contract)?;
        let authorization = policy
            .authorize_key_file(key_path, estate_id, permission, at)
            .map_err(ServiceError::Contract)?;
        let engine = Self::open_local_authority(database, authority_instance)?;
        let repository =
            rrd_estate::EstateRepository::new(&engine.storage, authorization.estate_id);
        let context = rrd_estate::MutationContext {
            at,
            actor: authorization.operator_id.to_string(),
            request_id,
            operation_id,
        };
        match action {
            EstateAdminAction::Create => {
                let outcome = repository.create_idempotent(&context)?;
                Ok(EstateAdminResult::Mutation(
                    rrd_contract::EstateMutationResult {
                        estate: rrd_estate::public_snapshot(&outcome.document),
                        idempotent_replay: outcome.idempotent_replay,
                    },
                ))
            }
            EstateAdminAction::SetDesired {
                instance,
                idempotency_key,
                phase,
                deployment,
                version,
                configuration_sha256,
            } => {
                let phase = match phase {
                    rrd_contract::EstateDesiredPhase::Running => rrd_estate::DesiredPhase::Running,
                    rrd_contract::EstateDesiredPhase::Stopped => rrd_estate::DesiredPhase::Stopped,
                    rrd_contract::EstateDesiredPhase::Absent => rrd_estate::DesiredPhase::Absent,
                };
                let outcome = repository.set_desired(&rrd_estate::SetDesired {
                    context,
                    instance_id: instance,
                    idempotency_key,
                    target: rrd_estate::DesiredTarget {
                        phase,
                        deployment_ref: deployment,
                        version,
                        configuration_sha256,
                    },
                })?;
                Ok(EstateAdminResult::Mutation(
                    rrd_contract::EstateMutationResult {
                        estate: rrd_estate::public_snapshot(&outcome.document),
                        idempotent_replay: outcome.idempotent_replay,
                    },
                ))
            }
            EstateAdminAction::ScheduleBackup {
                instance,
                idempotency_key,
                label,
            } => {
                let outcome = repository.schedule_backup(&rrd_estate::ScheduleBackup {
                    context,
                    instance_id: instance,
                    idempotency_key,
                    label,
                })?;
                Ok(EstateAdminResult::Backup(Box::new(
                    rrd_contract::EstateBackupMutationResult {
                        estate: rrd_estate::public_snapshot(&outcome.document),
                        job: rrd_estate::public_backup_job(&outcome.job),
                        idempotent_replay: outcome.idempotent_replay,
                    },
                )))
            }
        }
    }

    /// Advances one estate reconciliation boundary. The outward executable
    /// supplies coordinates only; catalog, driver, repository, and store
    /// construction remain inside `RrdEngine`.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_estate_store(
        database: &Path,
        authority_instance: CanonicalId,
        state_root: &Path,
        catalog_path: &Path,
        estate_id: CanonicalId,
        worker: CanonicalId,
        lease_ms: u64,
        at: u64,
        hold_after_effect: Option<&Path>,
    ) -> Result<EstateReconcileOutcome> {
        let catalog = rrd_estate::LocalDeploymentCatalog::load_json(catalog_path)
            .map_err(ServiceError::Contract)?;
        let driver = rrd_estate::LocalProcessDriver::new(state_root, catalog)
            .map_err(ServiceError::Contract)?;
        let engine = Self::open_local_authority(database, authority_instance)?;
        let mut reconciler = rrd_estate::Reconciler::new(
            &engine.storage,
            estate_id,
            worker,
            lease_ms,
            EffectHoldDriver {
                inner: driver,
                marker: hold_after_effect.map(Path::to_path_buf),
            },
        )?;
        reconciler.step(at).map_err(Into::into)
    }

    /// Advances one estate backup boundary with an engine-owned local backup
    /// driver. The driver may open a managed instance only as another
    /// `RrdEngine`; no physical store handle escapes the composition root.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_estate_backup_store(
        database: &Path,
        authority_instance: CanonicalId,
        state_root: &Path,
        estate_id: CanonicalId,
        worker: CanonicalId,
        lease_ms: u64,
        at: u64,
        hold_after_effect: Option<&Path>,
    ) -> Result<EstateBackupReconcileOutcome> {
        let driver = EngineLocalBackupDriver::new(state_root).map_err(ServiceError::Contract)?;
        let engine = Self::open_local_authority(database, authority_instance)?;
        let mut reconciler = rrd_estate::BackupReconciler::new(
            &engine.storage,
            estate_id,
            worker,
            lease_ms,
            BackupEffectHoldDriver {
                inner: driver,
                marker: hold_after_effect.map(Path::to_path_buf),
            },
        )?;
        reconciler.step(at).map_err(Into::into)
    }
}

struct EffectHoldDriver {
    inner: rrd_estate::LocalProcessDriver,
    marker: Option<PathBuf>,
}

impl EstateDriver for EffectHoldDriver {
    fn apply(
        &mut self,
        request: &rrd_estate::DriverRequest,
    ) -> std::result::Result<rrd_estate::DriverEffect, rrd_estate::DriverError> {
        let effect = self.inner.apply(request)?;
        maybe_test_hold(self.marker.as_deref(), &effect.evidence_sha256).map_err(|error| {
            rrd_estate::DriverError::retryable(error.to_string(), effect.evidence_sha256.clone())
        })?;
        Ok(effect)
    }

    fn observe(
        &mut self,
        request: &rrd_estate::DriverRequest,
    ) -> std::result::Result<rrd_estate::DriverObservation, rrd_estate::DriverError> {
        self.inner.observe(request)
    }
}

struct BackupEffectHoldDriver {
    inner: EngineLocalBackupDriver,
    marker: Option<PathBuf>,
}

impl EstateBackupDriver for BackupEffectHoldDriver {
    fn create(
        &mut self,
        request: &rrd_estate::BackupDriverRequest,
    ) -> std::result::Result<rrd_estate::BackupResult, rrd_estate::DriverError> {
        let result = self.inner.create(request)?;
        maybe_test_hold(self.marker.as_deref(), &result.backup_id).map_err(|error| {
            rrd_estate::DriverError::retryable(error.to_string(), result.evidence_sha256.clone())
        })?;
        Ok(result)
    }
}

/// Engine-owned implementation of the fixed local estate backup layout.
struct EngineLocalBackupDriver {
    state_root: PathBuf,
}

impl EngineLocalBackupDriver {
    fn new(state_root: impl AsRef<Path>) -> std::result::Result<Self, String> {
        let supplied = state_root.as_ref();
        if !supplied.is_absolute() {
            return Err("local backup state root must be absolute".into());
        }
        let state_root = fs::canonicalize(supplied)
            .map_err(|error| format!("cannot resolve local backup state root: {error}"))?;
        if !state_root.is_dir() {
            return Err("local backup state root must be a directory".into());
        }
        ensure_direct_directory(&state_root, "instances")?;
        ensure_direct_directory(&state_root, "processes")?;
        ensure_direct_directory(&state_root, "backups")?;
        Ok(Self { state_root })
    }

    fn source_path(&self, request: &rrd_estate::BackupDriverRequest) -> PathBuf {
        self.state_root
            .join("instances")
            .join(request.instance_id.as_str())
            .join(".rrflow/rrd")
    }

    fn catalogue_path(&self, request: &rrd_estate::BackupDriverRequest) -> PathBuf {
        self.state_root
            .join("backups")
            .join(request.instance_id.as_str())
    }

    fn create_inner(
        &self,
        request: &rrd_estate::BackupDriverRequest,
    ) -> std::result::Result<rrd_estate::BackupResult, rrd_estate::DriverError> {
        let process_record = self
            .state_root
            .join("processes")
            .join(format!("{}.json", request.instance_id));
        match fs::symlink_metadata(&process_record) {
            Ok(_) => {
                return Err(permanent(
                    request,
                    "backup denied because the instance has a retained process record",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(retryable(
                    request,
                    format!("cannot inspect the instance process record: {error}"),
                ));
            }
        }

        let source = self.source_path(request);
        let canonical_source = fs::canonicalize(&source).map_err(|error| {
            permanent(
                request,
                format!("cannot resolve the fixed instance data root: {error}"),
            )
        })?;
        if canonical_source != source || !canonical_source.is_dir() {
            return Err(permanent(
                request,
                "fixed instance data root is not a direct canonical directory",
            ));
        }

        let catalogue = self.catalogue_path(request);
        create_direct_directory(&catalogue).map_err(|error| {
            retryable(
                request,
                format!("cannot create the fixed backup catalogue root: {error}"),
            )
        })?;
        let canonical_catalogue = fs::canonicalize(&catalogue).map_err(|error| {
            retryable(
                request,
                format!("cannot resolve the fixed backup catalogue root: {error}"),
            )
        })?;
        if canonical_catalogue != catalogue || !canonical_catalogue.is_dir() {
            return Err(permanent(
                request,
                "fixed backup catalogue root is not a direct canonical directory",
            ));
        }

        RrdEngine::verify_backup_catalogue(&canonical_catalogue).map_err(|error| {
            permanent(
                request,
                format!("backup catalogue authentication failed: {error}"),
            )
        })?;
        let source_engine = RrdEngine::open(
            &canonical_source,
            request.instance_id.clone(),
            [0_u8; TOKEN_KEY_BYTES],
        )
        .map_err(|error| permanent(request, format!("instance engine open failed: {error}")))?;
        let entry = source_engine
            .create_logical_backup(&canonical_catalogue, &request.label, request.created_at)
            .map_err(|error| {
                retryable(request, format!("logical backup creation failed: {error}"))
            })?;
        let verified =
            RrdEngine::verify_backup_catalogue(&canonical_catalogue).map_err(|error| {
                permanent(
                    request,
                    format!("created backup verification failed: {error}"),
                )
            })?;
        let retained = verified
            .backups
            .iter()
            .find(|candidate| candidate.backup_id == entry.backup_id)
            .ok_or_else(|| permanent(request, "created backup is absent from its catalogue"))?;
        if retained != &entry {
            return Err(permanent(
                request,
                "created backup entry diverges from its authenticated catalogue",
            ));
        }
        let evidence_sha256 = digest::sha256_hex(
            &serde_json::to_vec(&(
                "rrd-local-estate-backup-v1",
                request,
                &entry.backup_id,
                &entry.archive.archive_sha256,
                &verified.catalogue_sha256,
            ))
            .expect("validated local backup evidence serializes"),
        );
        Ok(rrd_estate::BackupResult {
            backup_id: entry.backup_id.clone(),
            archive_sha256: entry.archive.archive_sha256.clone(),
            catalogue_sha256: verified.catalogue_sha256,
            evidence_sha256,
        })
    }
}

impl EstateBackupDriver for EngineLocalBackupDriver {
    fn create(
        &mut self,
        request: &rrd_estate::BackupDriverRequest,
    ) -> std::result::Result<rrd_estate::BackupResult, rrd_estate::DriverError> {
        self.create_inner(request)
    }
}

fn ensure_direct_directory(root: &Path, child: &str) -> std::result::Result<(), String> {
    let path = root.join(child);
    create_direct_directory(&path)
        .map_err(|error| format!("cannot create local {child} root: {error}"))?;
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("cannot resolve local {child} root: {error}"))?;
    if canonical != path || !canonical.is_dir() {
        return Err(format!("local {child} root must be a direct directory"));
    }
    Ok(())
}

fn create_direct_directory(path: &Path) -> std::io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(std::io::Error::other("path is not a direct directory"));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn permanent(
    request: &rrd_estate::BackupDriverRequest,
    message: impl Into<String>,
) -> rrd_estate::DriverError {
    let message = message.into();
    rrd_estate::DriverError::permanent(
        message.clone(),
        digest::sha256_hex(format!("backup-permanent:{}:{message}", request.job_id).as_bytes()),
    )
}

fn retryable(
    request: &rrd_estate::BackupDriverRequest,
    message: impl Into<String>,
) -> rrd_estate::DriverError {
    let message = message.into();
    rrd_estate::DriverError::retryable(
        message.clone(),
        digest::sha256_hex(format!("backup-retryable:{}:{message}", request.job_id).as_bytes()),
    )
}

#[cfg(debug_assertions)]
fn maybe_test_hold(marker: Option<&Path>, contents: &str) -> std::io::Result<()> {
    let Some(path) = marker else {
        return Ok(());
    };
    let temporary = path.with_extension("new");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    loop {
        std::thread::park_timeout(Duration::from_secs(60));
    }
}

#[cfg(not(debug_assertions))]
fn maybe_test_hold(marker: Option<&Path>, _contents: &str) -> std::io::Result<()> {
    if marker.is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "test hold failpoints are unavailable in release builds",
        ));
    }
    Ok(())
}
