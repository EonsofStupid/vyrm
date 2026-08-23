//! Persistent estate authority for RRD.
//!
//! This crate owns transport-neutral desired/observed state and durable
//! reconciliation boundaries. Connectome is a projection of these records,
//! never their source of truth.

mod backup_job;
mod local_authorization;
mod local_process;
mod reconcile;

pub use backup_job::{
    BackupIdempotencyBinding, BackupJobReceipt, BackupJobState, BackupReceiptBoundary,
    BackupScheduleOutcome, EstateBackupJob, MAX_BACKUP_IDEMPOTENCY_BINDINGS, MAX_BACKUP_JOBS,
    MAX_BACKUP_RECEIPTS_PER_JOB, ScheduleBackup, public_backup_job, public_backup_jobs,
};
pub use local_authorization::{
    LOCAL_OPERATOR_POLICY_FORMAT, LocalEstatePermission, LocalOperatorAuthorization,
    LocalOperatorPolicy,
};
pub use local_process::{
    LOCAL_DEPLOYMENT_FORMAT, LocalArgument, LocalDeployment, LocalDeploymentCatalog,
    LocalProcessDriver, LocalShutdown,
};
pub use reconcile::{
    DriverEffect, DriverError, DriverErrorKind, DriverObservation, DriverRequest, EstateDriver,
    ReconcileBoundary, ReconcileOutcome, Reconciler,
};

use rrd_contract::CanonicalId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use vyrm_core::digest;
use vyrm_store::{ControlTransition, Engine};

pub const ESTATE_FORMAT: u16 = 1;
pub const MAX_INSTANCES: usize = 1_024;
pub const MAX_OPERATIONS: usize = 4_096;
pub const MAX_IDEMPOTENCY_BINDINGS: usize = 4_096;
pub const MAX_RECEIPTS_PER_OPERATION: usize = 64;
const MAX_CREATE_REPLAY_JOURNAL_ENTRIES: usize = 65_536;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Store(vyrm_store::Error),
    Invalid(String),
    NotFound(String),
    AlreadyExists(String),
    IdempotencyConflict(String),
    LeaseBusy(String),
    StaleLease(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "estate storage failed: {error}"),
            Self::Invalid(message) => write!(formatter, "invalid estate state: {message}"),
            Self::NotFound(id) => write!(formatter, "estate resource not found: {id}"),
            Self::AlreadyExists(id) => write!(formatter, "estate already exists: {id}"),
            Self::IdempotencyConflict(key) => {
                write!(formatter, "estate idempotency key was rebound: {key}")
            }
            Self::LeaseBusy(operation) => {
                write!(formatter, "estate operation lease is held: {operation}")
            }
            Self::StaleLease(operation) => {
                write!(formatter, "estate operation lease is stale: {operation}")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<vyrm_store::Error> for Error {
    fn from(error: vyrm_store::Error) -> Self {
        Self::Store(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredPhase {
    Running,
    Stopped,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedPhase {
    Unknown,
    Provisioning,
    Starting,
    Running,
    Stopping,
    Stopped,
    Deleting,
    Absent,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Provision,
    Start,
    Stop,
    Restart,
    Upgrade,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Pending,
    Leased,
    Prepared,
    Applied,
    Succeeded,
    Failed,
    Superseded,
}

impl OperationState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Superseded)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptBoundary {
    Prepared,
    Applied,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityClass {
    Unknown,
    Active,
    Idle,
    Stale,
    Neglected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityPolicy {
    pub idle_after_ms: u64,
    pub stale_after_ms: u64,
    pub neglected_after_ms: u64,
}

impl Default for ActivityPolicy {
    fn default() -> Self {
        Self {
            idle_after_ms: 5 * 60 * 1_000,
            stale_after_ms: 30 * 60 * 1_000,
            neglected_after_ms: 7 * 24 * 60 * 60 * 1_000,
        }
    }
}

impl ActivityPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.idle_after_ms == 0
            || self.idle_after_ms >= self.stale_after_ms
            || self.stale_after_ms >= self.neglected_after_ms
        {
            return Err(Error::Invalid(
                "activity thresholds must be non-zero and strictly increasing".into(),
            ));
        }
        Ok(())
    }

    pub fn classify(&self, last_activity_at: Option<u64>, evaluated_at: u64) -> ActivityClass {
        let Some(last_activity_at) = last_activity_at else {
            return ActivityClass::Unknown;
        };
        let elapsed = evaluated_at.saturating_sub(last_activity_at);
        if elapsed <= self.idle_after_ms {
            ActivityClass::Active
        } else if elapsed <= self.stale_after_ms {
            ActivityClass::Idle
        } else if elapsed <= self.neglected_after_ms {
            ActivityClass::Stale
        } else {
            ActivityClass::Neglected
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesiredInstance {
    pub generation: u64,
    pub phase: DesiredPhase,
    pub deployment_ref: CanonicalId,
    pub version: String,
    pub configuration_sha256: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedInstance {
    pub generation: u64,
    pub phase: ObservedPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
    pub observed_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceActivity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_meaningful_runtime_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_at: Option<u64>,
    pub class: ActivityClass,
    pub evaluated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedInstance {
    pub id: CanonicalId,
    pub desired: DesiredInstance,
    pub observed: ObservedInstance,
    pub activity: InstanceActivity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationLease {
    pub owner: CanonicalId,
    pub epoch: u64,
    pub acquired_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationReceipt {
    pub boundary: ReceiptBoundary,
    pub lease_epoch: u64,
    pub at: u64,
    pub evidence_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateOperation {
    pub id: CanonicalId,
    pub instance_id: CanonicalId,
    pub kind: OperationKind,
    pub desired_generation: u64,
    pub request_sha256: String,
    pub state: OperationState,
    pub attempts: u32,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<OperationLease>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub receipts: Vec<OperationReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdempotencyBinding {
    pub operation_id: CanonicalId,
    pub request_sha256: String,
    pub bound_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateDocument {
    pub format: u16,
    pub id: CanonicalId,
    pub revision: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub activity_policy: ActivityPolicy,
    pub instances: BTreeMap<String, ManagedInstance>,
    pub operations: BTreeMap<String, EstateOperation>,
    pub idempotency: BTreeMap<String, IdempotencyBinding>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub backup_jobs: BTreeMap<String, EstateBackupJob>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub backup_idempotency: BTreeMap<String, BackupIdempotencyBinding>,
}

impl EstateDocument {
    pub fn new(id: CanonicalId, at: u64) -> Result<Self> {
        require_time(at)?;
        Ok(Self {
            format: ESTATE_FORMAT,
            id,
            revision: 1,
            created_at: at,
            updated_at: at,
            activity_policy: ActivityPolicy::default(),
            instances: BTreeMap::new(),
            operations: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            backup_jobs: backup_job::empty_backup_jobs(),
            backup_idempotency: backup_job::empty_backup_idempotency(),
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.format != ESTATE_FORMAT || self.revision == 0 {
            return Err(Error::Invalid(
                "unsupported estate format or revision".into(),
            ));
        }
        require_time(self.created_at)?;
        require_time(self.updated_at)?;
        if self.updated_at < self.created_at {
            return Err(Error::Invalid(
                "estate updated_at precedes created_at".into(),
            ));
        }
        self.activity_policy.validate()?;
        if self.instances.len() > MAX_INSTANCES
            || self.operations.len() > MAX_OPERATIONS
            || self.idempotency.len() > MAX_IDEMPOTENCY_BINDINGS
        {
            return Err(Error::Invalid(
                "estate aggregate cardinality exceeded".into(),
            ));
        }
        for (key, instance) in &self.instances {
            validate_id_key(key, &instance.id)?;
            validate_desired(&instance.desired)?;
            validate_observed(&instance.observed, instance.desired.generation)?;
            require_time(instance.activity.evaluated_at)?;
            if instance.activity.class
                != self.activity_policy.classify(
                    instance.activity.last_meaningful_runtime_at,
                    instance.activity.evaluated_at,
                )
            {
                return Err(Error::Invalid(format!(
                    "instance {} activity classification is inconsistent",
                    instance.id
                )));
            }
        }
        for (key, operation) in &self.operations {
            validate_id_key(key, &operation.id)?;
            let instance = self
                .instances
                .get(operation.instance_id.as_str())
                .ok_or_else(|| {
                    Error::Invalid(format!(
                        "operation {} names an unknown instance",
                        operation.id
                    ))
                })?;
            if operation.desired_generation == 0
                || operation.desired_generation > instance.desired.generation
            {
                return Err(Error::Invalid(format!(
                    "operation {} has an invalid desired generation",
                    operation.id
                )));
            }
            validate_sha256(&operation.request_sha256, "operation request")?;
            require_time(operation.created_at)?;
            require_time(operation.updated_at)?;
            if operation.updated_at < operation.created_at {
                return Err(Error::Invalid(format!(
                    "operation {} updated_at precedes created_at",
                    operation.id
                )));
            }
            validate_error(operation.error.as_deref())?;
            if let Some(lease) = &operation.lease {
                if lease.epoch == 0
                    || lease.acquired_at == 0
                    || lease.expires_at <= lease.acquired_at
                {
                    return Err(Error::Invalid(format!(
                        "operation {} has an invalid lease",
                        operation.id
                    )));
                }
            }
            if operation.receipts.len() > MAX_RECEIPTS_PER_OPERATION {
                return Err(Error::Invalid("operation receipt limit exceeded".into()));
            }
            for receipt in &operation.receipts {
                if receipt.lease_epoch == 0 || receipt.at == 0 {
                    return Err(Error::Invalid("operation receipt is invalid".into()));
                }
                validate_sha256(&receipt.evidence_sha256, "operation receipt")?;
            }
        }
        for (key, binding) in &self.idempotency {
            validate_ascii_key(key, "idempotency key")?;
            validate_sha256(&binding.request_sha256, "idempotency request")?;
            require_time(binding.bound_at)?;
            if !self.operations.contains_key(binding.operation_id.as_str()) {
                return Err(Error::Invalid(format!(
                    "idempotency key {key} names an unknown operation"
                )));
            }
        }
        backup_job::validate_backup_state(self)?;
        Ok(())
    }

    pub fn instance(&self, id: &CanonicalId) -> Option<&ManagedInstance> {
        self.instances.get(id.as_str())
    }

    pub fn operation(&self, id: &CanonicalId) -> Option<&EstateOperation> {
        self.operations.get(id.as_str())
    }
}

pub fn public_snapshot(document: &EstateDocument) -> rrd_contract::EstateSnapshot {
    rrd_contract::EstateSnapshot {
        format_version: document.format,
        id: document.id.clone(),
        revision: document.revision,
        created_at_unix_ms: document.created_at,
        updated_at_unix_ms: document.updated_at,
        activity_policy: rrd_contract::EstateActivityPolicySnapshot {
            idle_after_ms: document.activity_policy.idle_after_ms,
            stale_after_ms: document.activity_policy.stale_after_ms,
            neglected_after_ms: document.activity_policy.neglected_after_ms,
        },
        instances: document
            .instances
            .values()
            .map(|instance| rrd_contract::EstateInstanceSnapshot {
                id: instance.id.clone(),
                desired: rrd_contract::EstateDesiredSnapshot {
                    generation: instance.desired.generation,
                    phase: match instance.desired.phase {
                        DesiredPhase::Running => rrd_contract::EstateDesiredPhase::Running,
                        DesiredPhase::Stopped => rrd_contract::EstateDesiredPhase::Stopped,
                        DesiredPhase::Absent => rrd_contract::EstateDesiredPhase::Absent,
                    },
                    deployment_ref: instance.desired.deployment_ref.clone(),
                    version: instance.desired.version.clone(),
                    configuration_sha256: instance.desired.configuration_sha256.clone(),
                    updated_at_unix_ms: instance.desired.updated_at,
                },
                observed: rrd_contract::EstateObservedSnapshot {
                    generation: instance.observed.generation,
                    phase: public_observed_phase(instance.observed.phase),
                    version: instance.observed.version.clone(),
                    process_id: instance.observed.process_id,
                    observed_at_unix_ms: instance.observed.observed_at,
                    evidence_sha256: instance.observed.evidence_sha256.clone(),
                    error: instance.observed.error.clone(),
                },
                activity: rrd_contract::EstateActivitySnapshot {
                    last_meaningful_runtime_at_unix_ms: instance
                        .activity
                        .last_meaningful_runtime_at,
                    last_heartbeat_at_unix_ms: instance.activity.last_heartbeat_at,
                    class: match instance.activity.class {
                        ActivityClass::Unknown => rrd_contract::EstateActivityClass::Unknown,
                        ActivityClass::Active => rrd_contract::EstateActivityClass::Active,
                        ActivityClass::Idle => rrd_contract::EstateActivityClass::Idle,
                        ActivityClass::Stale => rrd_contract::EstateActivityClass::Stale,
                        ActivityClass::Neglected => rrd_contract::EstateActivityClass::Neglected,
                    },
                    evaluated_at_unix_ms: instance.activity.evaluated_at,
                },
            })
            .collect(),
        operations: document
            .operations
            .values()
            .map(|operation| rrd_contract::EstateOperationSnapshot {
                id: operation.id.clone(),
                instance_id: operation.instance_id.clone(),
                kind: match operation.kind {
                    OperationKind::Provision => rrd_contract::EstateOperationKind::Provision,
                    OperationKind::Start => rrd_contract::EstateOperationKind::Start,
                    OperationKind::Stop => rrd_contract::EstateOperationKind::Stop,
                    OperationKind::Restart => rrd_contract::EstateOperationKind::Restart,
                    OperationKind::Upgrade => rrd_contract::EstateOperationKind::Upgrade,
                    OperationKind::Delete => rrd_contract::EstateOperationKind::Delete,
                },
                desired_generation: operation.desired_generation,
                request_sha256: operation.request_sha256.clone(),
                state: match operation.state {
                    OperationState::Pending => rrd_contract::EstateOperationState::Pending,
                    OperationState::Leased => rrd_contract::EstateOperationState::Leased,
                    OperationState::Prepared => rrd_contract::EstateOperationState::Prepared,
                    OperationState::Applied => rrd_contract::EstateOperationState::Applied,
                    OperationState::Succeeded => rrd_contract::EstateOperationState::Succeeded,
                    OperationState::Failed => rrd_contract::EstateOperationState::Failed,
                    OperationState::Superseded => rrd_contract::EstateOperationState::Superseded,
                },
                attempts: operation.attempts,
                created_at_unix_ms: operation.created_at,
                updated_at_unix_ms: operation.updated_at,
                lease: operation
                    .lease
                    .as_ref()
                    .map(|lease| rrd_contract::EstateLeaseSnapshot {
                        owner: lease.owner.clone(),
                        epoch: lease.epoch,
                        acquired_at_unix_ms: lease.acquired_at,
                        expires_at_unix_ms: lease.expires_at,
                    }),
                receipts: operation
                    .receipts
                    .iter()
                    .map(|receipt| rrd_contract::EstateReceiptSnapshot {
                        boundary: match receipt.boundary {
                            ReceiptBoundary::Prepared => {
                                rrd_contract::EstateReceiptBoundary::Prepared
                            }
                            ReceiptBoundary::Applied => {
                                rrd_contract::EstateReceiptBoundary::Applied
                            }
                            ReceiptBoundary::Completed => {
                                rrd_contract::EstateReceiptBoundary::Completed
                            }
                            ReceiptBoundary::Failed => rrd_contract::EstateReceiptBoundary::Failed,
                        },
                        lease_epoch: receipt.lease_epoch,
                        at_unix_ms: receipt.at,
                        evidence_sha256: receipt.evidence_sha256.clone(),
                    })
                    .collect(),
                error: operation.error.clone(),
            })
            .collect(),
        idempotency_binding_count: u32::try_from(document.idempotency.len())
            .expect("bounded estate idempotency count fits u32"),
    }
}

fn public_observed_phase(phase: ObservedPhase) -> rrd_contract::EstateObservedPhase {
    match phase {
        ObservedPhase::Unknown => rrd_contract::EstateObservedPhase::Unknown,
        ObservedPhase::Provisioning => rrd_contract::EstateObservedPhase::Provisioning,
        ObservedPhase::Starting => rrd_contract::EstateObservedPhase::Starting,
        ObservedPhase::Running => rrd_contract::EstateObservedPhase::Running,
        ObservedPhase::Stopping => rrd_contract::EstateObservedPhase::Stopping,
        ObservedPhase::Stopped => rrd_contract::EstateObservedPhase::Stopped,
        ObservedPhase::Deleting => rrd_contract::EstateObservedPhase::Deleting,
        ObservedPhase::Absent => rrd_contract::EstateObservedPhase::Absent,
        ObservedPhase::Failed => rrd_contract::EstateObservedPhase::Failed,
    }
}

#[derive(Debug, Clone)]
pub struct MutationContext {
    pub at: u64,
    pub actor: String,
    pub request_id: String,
    pub operation_id: CanonicalId,
}

#[derive(Debug, Clone)]
pub struct DesiredTarget {
    pub phase: DesiredPhase,
    pub deployment_ref: CanonicalId,
    pub version: String,
    pub configuration_sha256: String,
}

#[derive(Debug, Clone)]
pub struct SetDesired {
    pub context: MutationContext,
    pub instance_id: CanonicalId,
    pub idempotency_key: String,
    pub target: DesiredTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredOutcome {
    pub document: EstateDocument,
    pub operation: EstateOperation,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOutcome {
    pub document: EstateDocument,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone)]
pub struct LeaseRequest {
    pub context: MutationContext,
    pub worker: CanonicalId,
    pub lease_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ReceiptRequest {
    pub context: MutationContext,
    pub worker: CanonicalId,
    pub lease_epoch: u64,
    pub boundary: ReceiptBoundary,
    pub evidence_sha256: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ObservationRequest {
    pub context: MutationContext,
    pub worker: CanonicalId,
    pub lease_epoch: u64,
    pub phase: ObservedPhase,
    pub version: Option<String>,
    pub process_id: Option<u32>,
    pub evidence_sha256: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ActivityEvidence {
    pub context: MutationContext,
    pub instance_id: CanonicalId,
    pub meaningful_runtime_at: Option<u64>,
    pub heartbeat_at: Option<u64>,
}

pub struct EstateRepository<'a, E: Engine + ?Sized> {
    engine: &'a E,
    estate_id: CanonicalId,
    key: String,
}

impl<'a, E: Engine + ?Sized> EstateRepository<'a, E> {
    pub fn new(engine: &'a E, estate_id: CanonicalId) -> Self {
        let key = format!("server/state/estate/{estate_id}/document");
        Self {
            engine,
            estate_id,
            key,
        }
    }

    pub fn create(&self, context: &MutationContext) -> Result<EstateDocument> {
        self.create_idempotent(context)
            .map(|outcome| outcome.document)
    }

    pub fn create_idempotent(&self, context: &MutationContext) -> Result<CreateOutcome> {
        validate_context(context)?;
        let document = EstateDocument::new(self.estate_id.clone(), context.at)?;
        let replacement = encode(&document)?;
        let transition = ControlTransition {
            key: self.key.clone(),
            expected: None,
            replacement: Some(replacement),
            at: context.at,
            actor: context.actor.clone(),
            action: "estate.create".into(),
            request_id: context.request_id.clone(),
            operation_id: context.operation_id.to_string(),
        };
        match self.engine.commit_control_transition(&transition) {
            Ok(_) => Ok(CreateOutcome {
                document,
                idempotent_replay: false,
            }),
            Err(vyrm_store::Error::ControlConflict(_)) => {
                let existing = self.load()?.ok_or_else(|| {
                    Error::Invalid("estate create conflicted without state".into())
                })?;
                let mut after = 0;
                let mut scanned = 0;
                loop {
                    let page = self.engine.control_journal_since(after, 1_024)?;
                    scanned += page.len();
                    if page.iter().any(|entry| {
                        entry.key == self.key
                            && entry.action == "estate.create"
                            && entry.actor == context.actor
                            && entry.request_id == context.request_id
                            && entry.operation_id == context.operation_id.as_str()
                            && entry.at == context.at
                    }) {
                        return Ok(CreateOutcome {
                            document: existing,
                            idempotent_replay: true,
                        });
                    }
                    let Some(last) = page.last() else {
                        break;
                    };
                    after = last.sequence;
                    if page.len() < 1_024 {
                        break;
                    }
                    if scanned >= MAX_CREATE_REPLAY_JOURNAL_ENTRIES {
                        return Err(Error::Invalid(
                            "estate create replay is outside the bounded journal window".into(),
                        ));
                    }
                }
                Err(Error::AlreadyExists(self.estate_id.to_string()))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn load(&self) -> Result<Option<EstateDocument>> {
        self.engine
            .control_record(&self.key)?
            .map(|bytes| decode(&bytes))
            .transpose()
    }

    pub fn set_desired(&self, request: &SetDesired) -> Result<DesiredOutcome> {
        validate_context(&request.context)?;
        validate_ascii_key(&request.idempotency_key, "idempotency key")?;
        validate_target(&request.target)?;
        let request_sha256 = desired_request_sha256(request);
        let Some(current_bytes) = self.engine.control_record(&self.key)? else {
            return Err(Error::NotFound(self.estate_id.to_string()));
        };
        let mut document = decode(&current_bytes)?;

        if let Some(binding) = document.idempotency.get(&request.idempotency_key) {
            if binding.request_sha256 != request_sha256
                || binding.operation_id != request.context.operation_id
            {
                return Err(Error::IdempotencyConflict(request.idempotency_key.clone()));
            }
            let operation = document
                .operation(&binding.operation_id)
                .cloned()
                .ok_or_else(|| Error::Invalid("idempotency operation is missing".into()))?;
            return Ok(DesiredOutcome {
                document,
                operation,
                idempotent_replay: true,
            });
        }
        if document
            .operations
            .contains_key(request.context.operation_id.as_str())
        {
            return Err(Error::IdempotencyConflict(
                request.context.operation_id.to_string(),
            ));
        }
        if document.idempotency.len() == MAX_IDEMPOTENCY_BINDINGS
            || document.operations.len() == MAX_OPERATIONS
        {
            return Err(Error::Invalid("estate history is at its v1 bound".into()));
        }

        let previous = document.instances.get(request.instance_id.as_str());
        if previous.is_some_and(|instance| target_matches(&instance.desired, &request.target)) {
            return Err(Error::Invalid("desired target is unchanged".into()));
        }
        if previous.is_none() && document.instances.len() == MAX_INSTANCES {
            return Err(Error::Invalid("estate instance limit exceeded".into()));
        }
        let generation = previous
            .map(|instance| instance.desired.generation)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("desired generation overflow".into()))?;
        let kind = operation_kind(previous, &request.target);
        let desired = DesiredInstance {
            generation,
            phase: request.target.phase,
            deployment_ref: request.target.deployment_ref.clone(),
            version: request.target.version.clone(),
            configuration_sha256: request.target.configuration_sha256.clone(),
            updated_at: request.context.at,
        };
        let managed = previous.cloned().map_or_else(
            || ManagedInstance {
                id: request.instance_id.clone(),
                desired: desired.clone(),
                observed: ObservedInstance {
                    generation: 0,
                    phase: ObservedPhase::Unknown,
                    version: None,
                    process_id: None,
                    observed_at: request.context.at,
                    evidence_sha256: None,
                    error: None,
                },
                activity: InstanceActivity {
                    last_meaningful_runtime_at: None,
                    last_heartbeat_at: None,
                    class: ActivityClass::Unknown,
                    evaluated_at: request.context.at,
                },
            },
            |mut instance| {
                instance.desired = desired.clone();
                instance
            },
        );
        document
            .instances
            .insert(request.instance_id.to_string(), managed);
        for previous_operation in document.operations.values_mut().filter(|operation| {
            operation.instance_id == request.instance_id && !operation.state.is_terminal()
        }) {
            previous_operation.state = OperationState::Superseded;
            previous_operation.updated_at = request.context.at;
            previous_operation.error = Some(format!(
                "superseded by desired generation {generation} operation {}",
                request.context.operation_id
            ));
        }
        let operation = EstateOperation {
            id: request.context.operation_id.clone(),
            instance_id: request.instance_id.clone(),
            kind,
            desired_generation: generation,
            request_sha256: request_sha256.clone(),
            state: OperationState::Pending,
            attempts: 0,
            created_at: request.context.at,
            updated_at: request.context.at,
            lease: None,
            receipts: Vec::new(),
            error: None,
        };
        document
            .operations
            .insert(request.context.operation_id.to_string(), operation.clone());
        document.idempotency.insert(
            request.idempotency_key.clone(),
            IdempotencyBinding {
                operation_id: request.context.operation_id.clone(),
                request_sha256,
                bound_at: request.context.at,
            },
        );
        self.commit(
            current_bytes,
            document,
            &request.context,
            "estate.desired.set",
        )
        .map(|document| DesiredOutcome {
            document,
            operation,
            idempotent_replay: false,
        })
    }

    pub fn acquire_lease(&self, request: &LeaseRequest) -> Result<EstateDocument> {
        validate_context(&request.context)?;
        if !(1_000..=3_600_000).contains(&request.lease_ms) {
            return Err(Error::Invalid("lease_ms must be in 1000..=3600000".into()));
        }
        self.update(&request.context, "estate.operation.lease", |document| {
            let operation = operation_mut(document, &request.context.operation_id)?;
            if operation.state.is_terminal() {
                return Err(Error::Invalid("terminal operation cannot be leased".into()));
            }
            if operation.lease.as_ref().is_some_and(|lease| {
                lease.expires_at > request.context.at && lease.owner != request.worker
            }) {
                return Err(Error::LeaseBusy(operation.id.to_string()));
            }
            if operation.lease.as_ref().is_some_and(|lease| {
                lease.expires_at > request.context.at && lease.owner == request.worker
            }) {
                return Ok(());
            }
            let epoch = operation
                .lease
                .as_ref()
                .map(|lease| lease.epoch)
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| Error::Invalid("lease epoch overflow".into()))?;
            let expires_at = request
                .context
                .at
                .checked_add(request.lease_ms)
                .ok_or_else(|| Error::Invalid("lease expiry overflow".into()))?;
            operation.attempts = operation
                .attempts
                .checked_add(1)
                .ok_or_else(|| Error::Invalid("operation attempts overflow".into()))?;
            if operation.state == OperationState::Pending {
                operation.state = OperationState::Leased;
            }
            operation.updated_at = request.context.at;
            operation.lease = Some(OperationLease {
                owner: request.worker.clone(),
                epoch,
                acquired_at: request.context.at,
                expires_at,
            });
            Ok(())
        })
    }

    pub fn record_receipt(&self, request: &ReceiptRequest) -> Result<EstateDocument> {
        validate_context(&request.context)?;
        validate_sha256(&request.evidence_sha256, "receipt evidence")?;
        validate_error(request.error.as_deref())?;
        self.update(&request.context, "estate.operation.receipt", |document| {
            let operation = operation_mut(document, &request.context.operation_id)?;
            let target = match request.boundary {
                ReceiptBoundary::Prepared => OperationState::Prepared,
                ReceiptBoundary::Applied => OperationState::Applied,
                ReceiptBoundary::Completed => OperationState::Succeeded,
                ReceiptBoundary::Failed => OperationState::Failed,
            };
            if let Some(existing) = operation.receipts.iter().find(|receipt| {
                receipt.boundary == request.boundary && receipt.lease_epoch == request.lease_epoch
            }) {
                if existing.evidence_sha256 == request.evidence_sha256 {
                    return Ok(());
                }
                return Err(Error::Invalid("receipt boundary was rebound".into()));
            }
            validate_lease(
                operation,
                &request.worker,
                request.lease_epoch,
                request.context.at,
            )?;
            if operation.receipts.len() == MAX_RECEIPTS_PER_OPERATION {
                return Err(Error::Invalid("operation receipt limit exceeded".into()));
            }
            validate_state_advance(operation.state, target)?;
            operation.receipts.push(OperationReceipt {
                boundary: request.boundary,
                lease_epoch: request.lease_epoch,
                at: request.context.at,
                evidence_sha256: request.evidence_sha256.clone(),
            });
            operation.state = target;
            operation.updated_at = request.context.at;
            operation.error = request.error.clone();
            Ok(())
        })
    }

    pub fn record_observation(&self, request: &ObservationRequest) -> Result<EstateDocument> {
        validate_context(&request.context)?;
        validate_sha256(&request.evidence_sha256, "observation evidence")?;
        validate_version(request.version.as_deref())?;
        validate_error(request.error.as_deref())?;
        self.update(&request.context, "estate.instance.observe", |document| {
            let operation = document
                .operation(&request.context.operation_id)
                .cloned()
                .ok_or_else(|| Error::NotFound(request.context.operation_id.to_string()))?;
            validate_lease(
                &operation,
                &request.worker,
                request.lease_epoch,
                request.context.at,
            )?;
            if !matches!(
                operation.state,
                OperationState::Prepared | OperationState::Applied
            ) {
                return Err(Error::Invalid(
                    "observation requires a prepared or applied operation".into(),
                ));
            }
            let instance = document
                .instances
                .get_mut(operation.instance_id.as_str())
                .ok_or_else(|| Error::NotFound(operation.instance_id.to_string()))?;
            instance.observed = ObservedInstance {
                generation: operation.desired_generation,
                phase: request.phase,
                version: request.version.clone(),
                process_id: request.process_id,
                observed_at: request.context.at,
                evidence_sha256: Some(request.evidence_sha256.clone()),
                error: request.error.clone(),
            };
            Ok(())
        })
    }

    pub fn record_activity(&self, evidence: &ActivityEvidence) -> Result<EstateDocument> {
        validate_context(&evidence.context)?;
        self.update(&evidence.context, "estate.instance.activity", |document| {
            let instance = document
                .instances
                .get_mut(evidence.instance_id.as_str())
                .ok_or_else(|| Error::NotFound(evidence.instance_id.to_string()))?;
            instance.activity.last_meaningful_runtime_at = max_option(
                instance.activity.last_meaningful_runtime_at,
                evidence.meaningful_runtime_at,
            );
            instance.activity.last_heartbeat_at =
                max_option(instance.activity.last_heartbeat_at, evidence.heartbeat_at);
            instance.activity.class = document.activity_policy.classify(
                instance.activity.last_meaningful_runtime_at,
                evidence.context.at,
            );
            instance.activity.evaluated_at = evidence.context.at;
            Ok(())
        })
    }

    pub fn refresh_activity(&self, context: &MutationContext) -> Result<EstateDocument> {
        validate_context(context)?;
        self.update(context, "estate.activity.refresh", |document| {
            for instance in document.instances.values_mut() {
                instance.activity.class = document
                    .activity_policy
                    .classify(instance.activity.last_meaningful_runtime_at, context.at);
                instance.activity.evaluated_at = context.at;
            }
            Ok(())
        })
    }

    fn update(
        &self,
        context: &MutationContext,
        action: &str,
        mutate: impl FnOnce(&mut EstateDocument) -> Result<()>,
    ) -> Result<EstateDocument> {
        let Some(current_bytes) = self.engine.control_record(&self.key)? else {
            return Err(Error::NotFound(self.estate_id.to_string()));
        };
        let mut document = decode(&current_bytes)?;
        mutate(&mut document)?;
        self.commit(current_bytes, document, context, action)
    }

    fn commit(
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
        let replacement = encode(&document)?;
        self.engine.commit_control_transition(&ControlTransition {
            key: self.key.clone(),
            expected: Some(expected),
            replacement: Some(replacement),
            at: context.at,
            actor: context.actor.clone(),
            action: action.into(),
            request_id: context.request_id.clone(),
            operation_id: context.operation_id.to_string(),
        })?;
        Ok(document)
    }
}

fn desired_request_sha256(request: &SetDesired) -> String {
    let bytes = serde_json::to_vec(&(
        &request.instance_id,
        request.target.phase,
        &request.target.deployment_ref,
        &request.target.version,
        &request.target.configuration_sha256,
    ))
    .expect("desired request fields serialize");
    digest::sha256_hex(&bytes)
}

fn operation_kind(previous: Option<&ManagedInstance>, target: &DesiredTarget) -> OperationKind {
    if target.phase == DesiredPhase::Absent {
        return OperationKind::Delete;
    }
    let Some(previous) = previous else {
        return OperationKind::Provision;
    };
    if previous.desired.version != target.version
        || previous.desired.configuration_sha256 != target.configuration_sha256
        || previous.desired.deployment_ref != target.deployment_ref
    {
        return OperationKind::Upgrade;
    }
    match target.phase {
        DesiredPhase::Running if previous.desired.phase == DesiredPhase::Running => {
            OperationKind::Restart
        }
        DesiredPhase::Running => OperationKind::Start,
        DesiredPhase::Stopped => OperationKind::Stop,
        DesiredPhase::Absent => OperationKind::Delete,
    }
}

fn target_matches(desired: &DesiredInstance, target: &DesiredTarget) -> bool {
    desired.phase == target.phase
        && desired.deployment_ref == target.deployment_ref
        && desired.version == target.version
        && desired.configuration_sha256 == target.configuration_sha256
}

fn operation_mut<'a>(
    document: &'a mut EstateDocument,
    id: &CanonicalId,
) -> Result<&'a mut EstateOperation> {
    document
        .operations
        .get_mut(id.as_str())
        .ok_or_else(|| Error::NotFound(id.to_string()))
}

fn validate_lease(
    operation: &EstateOperation,
    worker: &CanonicalId,
    epoch: u64,
    at: u64,
) -> Result<()> {
    let lease = operation
        .lease
        .as_ref()
        .ok_or_else(|| Error::StaleLease(operation.id.to_string()))?;
    if lease.owner != *worker || lease.epoch != epoch || lease.expires_at <= at {
        return Err(Error::StaleLease(operation.id.to_string()));
    }
    Ok(())
}

fn validate_state_advance(current: OperationState, target: OperationState) -> Result<()> {
    let valid = matches!(
        (current, target),
        (OperationState::Leased, OperationState::Prepared)
            | (OperationState::Prepared, OperationState::Applied)
            | (OperationState::Applied, OperationState::Succeeded)
            | (OperationState::Leased, OperationState::Failed)
            | (OperationState::Prepared, OperationState::Failed)
            | (OperationState::Applied, OperationState::Failed)
    );
    if !valid {
        return Err(Error::Invalid(format!(
            "operation state cannot advance from {current:?} to {target:?}"
        )));
    }
    Ok(())
}

fn validate_context(context: &MutationContext) -> Result<()> {
    require_time(context.at)?;
    validate_ascii_key(&context.actor, "actor")?;
    validate_ascii_key(&context.request_id, "request id")?;
    Ok(())
}

fn validate_target(target: &DesiredTarget) -> Result<()> {
    validate_version(Some(&target.version))?;
    validate_sha256(&target.configuration_sha256, "configuration")
}

fn validate_desired(desired: &DesiredInstance) -> Result<()> {
    if desired.generation == 0 {
        return Err(Error::Invalid("desired generation must be non-zero".into()));
    }
    require_time(desired.updated_at)?;
    validate_version(Some(&desired.version))?;
    validate_sha256(&desired.configuration_sha256, "configuration")
}

fn validate_observed(observed: &ObservedInstance, desired_generation: u64) -> Result<()> {
    if observed.generation > desired_generation {
        return Err(Error::Invalid(
            "observed generation exceeds desired generation".into(),
        ));
    }
    require_time(observed.observed_at)?;
    validate_version(observed.version.as_deref())?;
    if let Some(digest) = &observed.evidence_sha256 {
        validate_sha256(digest, "observation evidence")?;
    }
    validate_error(observed.error.as_deref())
}

fn validate_version(version: Option<&str>) -> Result<()> {
    if version.is_some_and(|value| {
        value.is_empty()
            || value.len() > 256
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && byte != b'\\')
    }) {
        return Err(Error::Invalid("version is invalid".into()));
    }
    Ok(())
}

fn validate_error(error: Option<&str>) -> Result<()> {
    if error.is_some_and(|value| value.len() > 4_096 || value.as_bytes().contains(&0)) {
        return Err(Error::Invalid("operation error is invalid".into()));
    }
    Ok(())
}

fn validate_id_key(key: &str, id: &CanonicalId) -> Result<()> {
    if key != id.as_str() {
        return Err(Error::Invalid(format!(
            "map key {key} does not match id {id}"
        )));
    }
    Ok(())
}

fn validate_ascii_key(value: &str, name: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(Error::Invalid(format!("{name} is invalid")));
    }
    Ok(())
}

fn validate_sha256(value: &str, name: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(Error::Invalid(format!("{name} SHA-256 is invalid")));
    }
    Ok(())
}

fn require_time(at: u64) -> Result<()> {
    if at == 0 {
        return Err(Error::Invalid("timestamp must be non-zero".into()));
    }
    Ok(())
}

fn max_option(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    }
}

fn encode(document: &EstateDocument) -> Result<Vec<u8>> {
    document.validate()?;
    serde_json::to_vec(document).map_err(|error| Error::Invalid(error.to_string()))
}

fn decode(bytes: &[u8]) -> Result<EstateDocument> {
    let document: EstateDocument =
        serde_json::from_slice(bytes).map_err(|error| Error::Invalid(error.to_string()))?;
    document.validate()?;
    Ok(document)
}
