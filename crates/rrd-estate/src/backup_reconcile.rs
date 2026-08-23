use crate::{
    BackupCompleteRequest, BackupFailRequest, BackupJobState, BackupLeaseRequest,
    BackupPreparedRequest, BackupResult, DesiredPhase, DriverError, DriverErrorKind, Error,
    EstateBackupJob, EstateDocument, EstateRepository, MutationContext, ObservedPhase, Result,
};
use rrd_contract::CanonicalId;
use serde::{Deserialize, Serialize};
use vyrm_core::digest;
use vyrm_store::Engine;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupDriverRequest {
    pub estate_id: CanonicalId,
    pub job_id: CanonicalId,
    pub instance_id: CanonicalId,
    pub source_generation: u64,
    pub label: String,
    pub created_at: u64,
}

impl BackupDriverRequest {
    pub fn sha256(&self) -> String {
        digest::sha256_hex(
            &serde_json::to_vec(self).expect("validated backup driver request serializes"),
        )
    }
}

pub trait EstateBackupDriver {
    fn create(
        &mut self,
        request: &BackupDriverRequest,
    ) -> std::result::Result<BackupResult, DriverError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupReconcileBoundary {
    LeaseAcquired,
    Prepared,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum BackupReconcileOutcome {
    Idle,
    WaitingForLease {
        job_id: CanonicalId,
        expires_at: u64,
    },
    Deferred {
        job_id: CanonicalId,
        message: String,
    },
    Advanced {
        job_id: CanonicalId,
        boundary: BackupReconcileBoundary,
        revision: u64,
    },
}

pub struct BackupReconciler<'a, E: Engine + ?Sized, D: EstateBackupDriver> {
    repository: EstateRepository<'a, E>,
    estate_id: CanonicalId,
    worker: CanonicalId,
    lease_ms: u64,
    driver: D,
}

impl<'a, E: Engine + ?Sized, D: EstateBackupDriver> BackupReconciler<'a, E, D> {
    pub fn new(
        engine: &'a E,
        estate_id: CanonicalId,
        worker: CanonicalId,
        lease_ms: u64,
        driver: D,
    ) -> Result<Self> {
        if !(1_000..=3_600_000).contains(&lease_ms) {
            return Err(Error::Invalid(
                "backup reconciler lease_ms must be in 1000..=3600000".into(),
            ));
        }
        Ok(Self {
            repository: EstateRepository::new(engine, estate_id.clone()),
            estate_id,
            worker,
            lease_ms,
            driver,
        })
    }

    pub fn driver(&self) -> &D {
        &self.driver
    }

    pub fn driver_mut(&mut self) -> &mut D {
        &mut self.driver
    }

    pub fn into_driver(self) -> D {
        self.driver
    }

    pub fn step(&mut self, at: u64) -> Result<BackupReconcileOutcome> {
        if at == 0 {
            return Err(Error::Invalid(
                "backup reconcile timestamp must be non-zero".into(),
            ));
        }
        let document = self
            .repository
            .load()?
            .ok_or_else(|| Error::NotFound(self.estate_id.to_string()))?;
        if at < document.updated_at {
            return Err(Error::Invalid(
                "backup reconcile timestamp precedes estate authority".into(),
            ));
        }
        let Some(job) = select_backup_job(&document, &self.worker, at) else {
            if let Some(waiting) = oldest_open_backup_job(&document) {
                return Ok(BackupReconcileOutcome::WaitingForLease {
                    job_id: waiting.id.clone(),
                    expires_at: waiting.lease.as_ref().map_or(at, |lease| lease.expires_at),
                });
            }
            return Ok(BackupReconcileOutcome::Idle);
        };

        if job
            .lease
            .as_ref()
            .is_none_or(|lease| lease.expires_at <= at)
        {
            let updated = self.repository.acquire_backup_lease(&BackupLeaseRequest {
                context: self.context(at, document.revision, "lease", &job.id),
                worker: self.worker.clone(),
                lease_ms: self.lease_ms,
            })?;
            return Ok(advanced(
                job.id,
                BackupReconcileBoundary::LeaseAcquired,
                updated.revision,
            ));
        }
        let lease = job
            .lease
            .as_ref()
            .ok_or_else(|| Error::Invalid("selected backup job has no lease".into()))?;
        if lease.owner != self.worker {
            return Ok(BackupReconcileOutcome::WaitingForLease {
                job_id: job.id,
                expires_at: lease.expires_at,
            });
        }
        let request = match backup_driver_request(&document, &job) {
            Ok(request) => request,
            Err(error) => {
                let message = error.to_string();
                let evidence_sha256 = digest::sha256_hex(
                    format!("backup-precondition:{}:{message}", job.id).as_bytes(),
                );
                let updated = self.repository.record_backup_failed(&BackupFailRequest {
                    context: self.context(at, document.revision, "failed", &job.id),
                    worker: self.worker.clone(),
                    lease_epoch: lease.epoch,
                    evidence_sha256,
                    error: message,
                })?;
                return Ok(advanced(
                    job.id,
                    BackupReconcileBoundary::Failed,
                    updated.revision,
                ));
            }
        };
        match job.state {
            BackupJobState::Pending => Err(Error::Invalid(
                "pending backup job unexpectedly has an active lease".into(),
            )),
            BackupJobState::Leased => {
                let updated = self
                    .repository
                    .record_backup_prepared(&BackupPreparedRequest {
                        context: self.context(at, document.revision, "prepared", &job.id),
                        worker: self.worker.clone(),
                        lease_epoch: lease.epoch,
                        evidence_sha256: request.sha256(),
                    })?;
                Ok(advanced(
                    job.id,
                    BackupReconcileBoundary::Prepared,
                    updated.revision,
                ))
            }
            BackupJobState::Prepared => match self.driver.create(&request) {
                Ok(result) => {
                    validate_backup_result(&result)?;
                    let updated =
                        self.repository
                            .record_backup_completed(&BackupCompleteRequest {
                                context: self.context(at, document.revision, "completed", &job.id),
                                worker: self.worker.clone(),
                                lease_epoch: lease.epoch,
                                result,
                            })?;
                    Ok(advanced(
                        job.id,
                        BackupReconcileBoundary::Completed,
                        updated.revision,
                    ))
                }
                Err(error) => {
                    super::validate_sha256(&error.evidence_sha256, "backup driver evidence")?;
                    match error.kind {
                        DriverErrorKind::Retryable => Ok(BackupReconcileOutcome::Deferred {
                            job_id: job.id,
                            message: error.message,
                        }),
                        DriverErrorKind::Permanent => {
                            let updated =
                                self.repository.record_backup_failed(&BackupFailRequest {
                                    context: self.context(at, document.revision, "failed", &job.id),
                                    worker: self.worker.clone(),
                                    lease_epoch: lease.epoch,
                                    evidence_sha256: error.evidence_sha256,
                                    error: error.message,
                                })?;
                            Ok(advanced(
                                job.id,
                                BackupReconcileBoundary::Failed,
                                updated.revision,
                            ))
                        }
                    }
                }
            },
            BackupJobState::Succeeded | BackupJobState::Failed => Ok(BackupReconcileOutcome::Idle),
        }
    }

    fn context(
        &self,
        at: u64,
        revision: u64,
        boundary: &str,
        job: &CanonicalId,
    ) -> MutationContext {
        MutationContext {
            at,
            actor: "rrd-backup-reconciler".into(),
            request_id: format!("backup-reconcile-{boundary}-{revision}"),
            operation_id: job.clone(),
        }
    }
}

fn select_backup_job(
    document: &EstateDocument,
    worker: &CanonicalId,
    at: u64,
) -> Option<EstateBackupJob> {
    document
        .backup_jobs
        .values()
        .filter(|job| !job.state.is_terminal())
        .filter(|job| {
            job.lease
                .as_ref()
                .is_none_or(|lease| lease.expires_at <= at || lease.owner == *worker)
        })
        .min_by_key(|job| (job.created_at, job.id.as_str()))
        .cloned()
}

fn oldest_open_backup_job(document: &EstateDocument) -> Option<&EstateBackupJob> {
    document
        .backup_jobs
        .values()
        .filter(|job| !job.state.is_terminal())
        .min_by_key(|job| (job.created_at, job.id.as_str()))
}

fn backup_driver_request(
    document: &EstateDocument,
    job: &EstateBackupJob,
) -> Result<BackupDriverRequest> {
    let instance = document
        .instance(&job.instance_id)
        .ok_or_else(|| Error::NotFound(job.instance_id.to_string()))?;
    if instance.desired.generation != job.source_generation
        || instance.observed.generation != job.source_generation
        || instance.desired.phase != DesiredPhase::Stopped
        || instance.observed.phase != ObservedPhase::Stopped
        || instance.observed.process_id.is_some()
    {
        return Err(Error::Invalid(format!(
            "backup job {} source instance is no longer quiesced at generation {}",
            job.id, job.source_generation
        )));
    }
    Ok(BackupDriverRequest {
        estate_id: document.id.clone(),
        job_id: job.id.clone(),
        instance_id: job.instance_id.clone(),
        source_generation: job.source_generation,
        label: job.label.clone(),
        created_at: job.created_at,
    })
}

fn validate_backup_result(result: &BackupResult) -> Result<()> {
    super::validate_sha256(&result.backup_id, "backup identity")?;
    super::validate_sha256(&result.archive_sha256, "backup archive")?;
    super::validate_sha256(&result.catalogue_sha256, "backup catalogue")?;
    super::validate_sha256(&result.evidence_sha256, "backup evidence")
}

fn advanced(
    job_id: CanonicalId,
    boundary: BackupReconcileBoundary,
    revision: u64,
) -> BackupReconcileOutcome {
    BackupReconcileOutcome::Advanced {
        job_id,
        boundary,
        revision,
    }
}
