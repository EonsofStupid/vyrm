use super::{
    Error, EstateDocument, EstateRepository, MutationContext, ObservedPhase, OperationLease,
    Result, encode, validate_ascii_key, validate_context, validate_error, validate_id_key,
    validate_sha256,
};
use rrd_contract::CanonicalId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use vyrm_core::digest;
use vyrm_store::{ControlTransition, Engine};

pub const MAX_BACKUP_JOBS: usize = 4_096;
pub const MAX_BACKUP_IDEMPOTENCY_BINDINGS: usize = 4_096;
pub const MAX_BACKUP_RECEIPTS_PER_JOB: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupJobState {
    Pending,
    Leased,
    Prepared,
    Succeeded,
    Failed,
}

impl BackupJobState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupReceiptBoundary {
    Prepared,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupJobReceipt {
    pub boundary: BackupReceiptBoundary,
    pub lease_epoch: u64,
    pub at: u64,
    pub evidence_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateBackupJob {
    pub id: CanonicalId,
    pub instance_id: CanonicalId,
    pub source_generation: u64,
    pub label: String,
    pub request_sha256: String,
    pub state: BackupJobState,
    pub attempts: u32,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<OperationLease>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub receipts: Vec<BackupJobReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalogue_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupIdempotencyBinding {
    pub job_id: CanonicalId,
    pub request_sha256: String,
    pub bound_at: u64,
}

#[derive(Debug, Clone)]
pub struct ScheduleBackup {
    pub context: MutationContext,
    pub instance_id: CanonicalId,
    pub idempotency_key: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct BackupScheduleOutcome {
    pub document: EstateDocument,
    pub job: EstateBackupJob,
    pub idempotent_replay: bool,
}

impl<'a, E: Engine + ?Sized> EstateRepository<'a, E> {
    pub fn schedule_backup(&self, request: &ScheduleBackup) -> Result<BackupScheduleOutcome> {
        validate_context(&request.context)?;
        validate_ascii_key(&request.idempotency_key, "backup idempotency key")?;
        validate_backup_label(&request.label)?;
        let Some(current_bytes) = self.engine.control_record(&self.key)? else {
            return Err(Error::NotFound(self.estate_id.to_string()));
        };
        let mut document = super::decode(&current_bytes)?;

        if let Some(binding) = document.backup_idempotency.get(&request.idempotency_key) {
            let job = document
                .backup_jobs
                .get(binding.job_id.as_str())
                .cloned()
                .ok_or_else(|| Error::Invalid("backup idempotency job is missing".into()))?;
            let request_sha256 =
                backup_request_sha256(&request.instance_id, job.source_generation, &request.label);
            if binding.request_sha256 != request_sha256
                || job.request_sha256 != request_sha256
                || job.id != request.context.operation_id
            {
                return Err(Error::IdempotencyConflict(request.idempotency_key.clone()));
            }
            return Ok(BackupScheduleOutcome {
                document,
                job,
                idempotent_replay: true,
            });
        }
        if document
            .backup_jobs
            .contains_key(request.context.operation_id.as_str())
        {
            return Err(Error::IdempotencyConflict(
                request.context.operation_id.to_string(),
            ));
        }
        if document.backup_jobs.len() == MAX_BACKUP_JOBS
            || document.backup_idempotency.len() == MAX_BACKUP_IDEMPOTENCY_BINDINGS
        {
            return Err(Error::Invalid(
                "estate backup job history is at its v1 bound".into(),
            ));
        }
        let instance = document
            .instances
            .get(request.instance_id.as_str())
            .ok_or_else(|| Error::NotFound(request.instance_id.to_string()))?;
        if instance.desired.phase != super::DesiredPhase::Stopped
            || instance.observed.phase != ObservedPhase::Stopped
            || instance.observed.generation != instance.desired.generation
            || instance.observed.process_id.is_some()
        {
            return Err(Error::Invalid(
                "local backup requires desired and observed stopped at the same generation".into(),
            ));
        }
        let source_generation = instance.desired.generation;
        let request_sha256 =
            backup_request_sha256(&request.instance_id, source_generation, &request.label);
        let job = EstateBackupJob {
            id: request.context.operation_id.clone(),
            instance_id: request.instance_id.clone(),
            source_generation,
            label: request.label.clone(),
            request_sha256: request_sha256.clone(),
            state: BackupJobState::Pending,
            attempts: 0,
            created_at: request.context.at,
            updated_at: request.context.at,
            lease: None,
            receipts: Vec::new(),
            backup_id: None,
            archive_sha256: None,
            catalogue_sha256: None,
            error: None,
        };
        document.backup_jobs.insert(job.id.to_string(), job.clone());
        document.backup_idempotency.insert(
            request.idempotency_key.clone(),
            BackupIdempotencyBinding {
                job_id: job.id.clone(),
                request_sha256,
                bound_at: request.context.at,
            },
        );
        let document = self.commit_backup_schedule(
            current_bytes,
            document,
            &request.context,
            "estate.backup.schedule",
        )?;
        Ok(BackupScheduleOutcome {
            document,
            job,
            idempotent_replay: false,
        })
    }

    fn commit_backup_schedule(
        &self,
        expected: Vec<u8>,
        mut document: EstateDocument,
        context: &MutationContext,
        action: &str,
    ) -> Result<EstateDocument> {
        if context.at < document.updated_at {
            return Err(Error::Invalid(
                "mutation timestamp precedes the estate revision".into(),
            ));
        }
        document.revision = document
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("estate revision overflow".into()))?;
        document.updated_at = context.at;
        document.validate()?;
        self.engine.commit_control_transition(&ControlTransition {
            key: self.key.clone(),
            expected: Some(expected),
            replacement: Some(encode(&document)?),
            at: context.at,
            actor: context.actor.clone(),
            action: action.into(),
            request_id: context.request_id.clone(),
            operation_id: context.operation_id.to_string(),
        })?;
        Ok(document)
    }
}

pub fn public_backup_jobs(document: &EstateDocument) -> rrd_contract::EstateBackupJobsSnapshot {
    rrd_contract::EstateBackupJobsSnapshot {
        estate_id: document.id.clone(),
        estate_revision: document.revision,
        jobs: document
            .backup_jobs
            .values()
            .map(public_backup_job)
            .collect(),
    }
}

pub fn public_backup_job(job: &EstateBackupJob) -> rrd_contract::EstateBackupJobSnapshot {
    rrd_contract::EstateBackupJobSnapshot {
        id: job.id.clone(),
        instance_id: job.instance_id.clone(),
        source_generation: job.source_generation,
        label: job.label.clone(),
        request_sha256: job.request_sha256.clone(),
        state: match job.state {
            BackupJobState::Pending => rrd_contract::EstateBackupJobState::Pending,
            BackupJobState::Leased => rrd_contract::EstateBackupJobState::Leased,
            BackupJobState::Prepared => rrd_contract::EstateBackupJobState::Prepared,
            BackupJobState::Succeeded => rrd_contract::EstateBackupJobState::Succeeded,
            BackupJobState::Failed => rrd_contract::EstateBackupJobState::Failed,
        },
        attempts: job.attempts,
        created_at_unix_ms: job.created_at,
        updated_at_unix_ms: job.updated_at,
        lease: job
            .lease
            .as_ref()
            .map(|lease| rrd_contract::EstateLeaseSnapshot {
                owner: lease.owner.clone(),
                epoch: lease.epoch,
                acquired_at_unix_ms: lease.acquired_at,
                expires_at_unix_ms: lease.expires_at,
            }),
        receipts: job
            .receipts
            .iter()
            .map(|receipt| rrd_contract::EstateBackupReceiptSnapshot {
                boundary: match receipt.boundary {
                    BackupReceiptBoundary::Prepared => {
                        rrd_contract::EstateBackupReceiptBoundary::Prepared
                    }
                    BackupReceiptBoundary::Completed => {
                        rrd_contract::EstateBackupReceiptBoundary::Completed
                    }
                    BackupReceiptBoundary::Failed => {
                        rrd_contract::EstateBackupReceiptBoundary::Failed
                    }
                },
                lease_epoch: receipt.lease_epoch,
                at_unix_ms: receipt.at,
                evidence_sha256: receipt.evidence_sha256.clone(),
            })
            .collect(),
        backup_id: job.backup_id.clone(),
        archive_sha256: job.archive_sha256.clone(),
        catalogue_sha256: job.catalogue_sha256.clone(),
        error: job.error.clone(),
    }
}

pub(crate) fn validate_backup_state(document: &EstateDocument) -> Result<()> {
    if document.backup_jobs.len() > MAX_BACKUP_JOBS
        || document.backup_idempotency.len() > MAX_BACKUP_IDEMPOTENCY_BINDINGS
    {
        return Err(Error::Invalid(
            "estate backup job cardinality exceeded".into(),
        ));
    }
    for (key, job) in &document.backup_jobs {
        validate_id_key(key, &job.id)?;
        validate_backup_label(&job.label)?;
        validate_sha256(&job.request_sha256, "backup request")?;
        if job.source_generation == 0
            || job.created_at == 0
            || job.updated_at < job.created_at
            || !document.instances.contains_key(job.instance_id.as_str())
        {
            return Err(Error::Invalid(format!("backup job {} is invalid", job.id)));
        }
        if job.receipts.len() > MAX_BACKUP_RECEIPTS_PER_JOB {
            return Err(Error::Invalid("backup receipt limit exceeded".into()));
        }
        for receipt in &job.receipts {
            if receipt.lease_epoch == 0 || receipt.at == 0 {
                return Err(Error::Invalid("backup receipt is invalid".into()));
            }
            validate_sha256(&receipt.evidence_sha256, "backup receipt")?;
        }
        validate_error(job.error.as_deref())?;
        validate_backup_result(job)?;
    }
    for (key, binding) in &document.backup_idempotency {
        validate_ascii_key(key, "backup idempotency key")?;
        validate_sha256(&binding.request_sha256, "backup idempotency request")?;
        if binding.bound_at == 0 {
            return Err(Error::Invalid(
                "backup idempotency binding time is invalid".into(),
            ));
        }
        let job = document
            .backup_jobs
            .get(binding.job_id.as_str())
            .ok_or_else(|| Error::Invalid(format!("backup idempotency key {key} is orphaned")))?;
        if job.request_sha256 != binding.request_sha256 {
            return Err(Error::Invalid(format!(
                "backup idempotency key {key} request digest diverged"
            )));
        }
    }
    Ok(())
}

fn validate_backup_result(job: &EstateBackupJob) -> Result<()> {
    let result_count = [
        job.backup_id.as_deref(),
        job.archive_sha256.as_deref(),
        job.catalogue_sha256.as_deref(),
    ]
    .iter()
    .filter(|value| value.is_some())
    .count();
    if result_count != 0 && result_count != 3 {
        return Err(Error::Invalid(
            "backup result identity is only partially populated".into(),
        ));
    }
    for (name, value) in [
        ("backup identity", job.backup_id.as_deref()),
        ("backup archive", job.archive_sha256.as_deref()),
        ("backup catalogue", job.catalogue_sha256.as_deref()),
    ] {
        if let Some(value) = value {
            validate_sha256(value, name)?;
        }
    }
    match job.state {
        BackupJobState::Pending => {
            if job.attempts != 0 || job.lease.is_some() || !job.receipts.is_empty() {
                return Err(Error::Invalid(
                    "pending backup job has execution state".into(),
                ));
            }
        }
        BackupJobState::Leased | BackupJobState::Prepared => {
            if job.attempts == 0 || job.lease.is_none() {
                return Err(Error::Invalid(
                    "active backup job lacks a lease attempt".into(),
                ));
            }
        }
        BackupJobState::Succeeded => {
            if result_count != 3 || job.error.is_some() {
                return Err(Error::Invalid(
                    "succeeded backup job lacks its result".into(),
                ));
            }
        }
        BackupJobState::Failed => {
            if job.error.is_none() {
                return Err(Error::Invalid("failed backup job lacks an error".into()));
            }
        }
    }
    Ok(())
}

fn validate_backup_label(label: &str) -> Result<()> {
    if label.is_empty()
        || label.len() > 128
        || label.trim() != label
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(Error::Invalid(
            "backup label must be 1-128 ASCII alphanumeric, '.', '-', or '_' characters".into(),
        ));
    }
    Ok(())
}

fn backup_request_sha256(instance_id: &CanonicalId, source_generation: u64, label: &str) -> String {
    digest::sha256_hex(
        &serde_json::to_vec(&(
            "rrd-estate-backup-create-v1",
            instance_id,
            source_generation,
            label,
        ))
        .expect("backup request fields serialize"),
    )
}

pub(crate) fn empty_backup_jobs() -> BTreeMap<String, EstateBackupJob> {
    BTreeMap::new()
}

pub(crate) fn empty_backup_idempotency() -> BTreeMap<String, BackupIdempotencyBinding> {
    BTreeMap::new()
}
