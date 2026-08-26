use crate::{
    DesiredInstance, DesiredPhase, Error, EstateDocument, EstateOperation, EstateRepository,
    LeaseRequest, MutationContext, ObservedPhase, OperationKind, OperationState, ReceiptBoundary,
    ReceiptRequest, Result,
};
use rrd_contract::CanonicalId;
use rrd_core::digest;
use rrd_store::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverRequest {
    pub estate_id: CanonicalId,
    pub instance_id: CanonicalId,
    pub operation_id: CanonicalId,
    pub kind: OperationKind,
    pub desired: DesiredInstance,
}

impl DriverRequest {
    pub fn sha256(&self) -> String {
        digest::sha256_hex(
            &serde_json::to_vec(self).expect("validated driver request fields serialize"),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverEffect {
    pub evidence_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverObservation {
    pub phase: ObservedPhase,
    pub version: Option<String>,
    pub process_id: Option<u32>,
    pub evidence_sha256: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverErrorKind {
    Retryable,
    Permanent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverError {
    pub kind: DriverErrorKind,
    pub message: String,
    pub evidence_sha256: String,
}

impl DriverError {
    pub fn retryable(message: impl Into<String>, evidence_sha256: impl Into<String>) -> Self {
        Self {
            kind: DriverErrorKind::Retryable,
            message: message.into(),
            evidence_sha256: evidence_sha256.into(),
        }
    }

    pub fn permanent(message: impl Into<String>, evidence_sha256: impl Into<String>) -> Self {
        Self {
            kind: DriverErrorKind::Permanent,
            message: message.into(),
            evidence_sha256: evidence_sha256.into(),
        }
    }
}

/// Typed effect boundary. Implementations must bind every external mutation to
/// `request.operation_id` and make a retry of that ID converge without
/// repeating a destructive effect.
pub trait EstateDriver {
    fn apply(&mut self, request: &DriverRequest) -> std::result::Result<DriverEffect, DriverError>;

    fn observe(
        &mut self,
        request: &DriverRequest,
    ) -> std::result::Result<DriverObservation, DriverError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileBoundary {
    LeaseAcquired,
    Prepared,
    Applied,
    Observed,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ReconcileOutcome {
    Idle,
    WaitingForLease {
        operation_id: CanonicalId,
        expires_at: u64,
    },
    Deferred {
        operation_id: CanonicalId,
        message: String,
    },
    Advanced {
        operation_id: CanonicalId,
        boundary: ReconcileBoundary,
        revision: u64,
    },
}

pub struct Reconciler<'a, E: Engine + ?Sized, D: EstateDriver> {
    repository: EstateRepository<'a, E>,
    estate_id: CanonicalId,
    worker: CanonicalId,
    lease_ms: u64,
    driver: D,
}

impl<'a, E: Engine + ?Sized, D: EstateDriver> Reconciler<'a, E, D> {
    pub fn new(
        engine: &'a E,
        estate_id: CanonicalId,
        worker: CanonicalId,
        lease_ms: u64,
        driver: D,
    ) -> Result<Self> {
        if !(1_000..=3_600_000).contains(&lease_ms) {
            return Err(Error::Invalid(
                "reconciler lease_ms must be in 1000..=3600000".into(),
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

    /// Advances at most one durable boundary. Calling code can stop, crash or
    /// reopen between any two successful outcomes without losing the state
    /// needed to resume.
    pub fn step(&mut self, at: u64) -> Result<ReconcileOutcome> {
        if at == 0 {
            return Err(Error::Invalid(
                "reconcile timestamp must be non-zero".into(),
            ));
        }
        let document = self
            .repository
            .load()?
            .ok_or_else(|| Error::NotFound(self.estate_id.to_string()))?;
        if at < document.updated_at {
            return Err(Error::Invalid(
                "reconcile timestamp precedes estate authority".into(),
            ));
        }
        let Some(operation) = select_operation(&document, &self.worker, at) else {
            if let Some(waiting) = oldest_open_operation(&document) {
                let expires_at = waiting
                    .lease
                    .as_ref()
                    .map(|lease| lease.expires_at)
                    .unwrap_or(at);
                return Ok(ReconcileOutcome::WaitingForLease {
                    operation_id: waiting.id.clone(),
                    expires_at,
                });
            }
            return Ok(ReconcileOutcome::Idle);
        };

        if operation
            .lease
            .as_ref()
            .is_none_or(|lease| lease.expires_at <= at)
        {
            let document = self.repository.acquire_lease(&LeaseRequest {
                context: self.context(at, document.revision, "lease", &operation.id),
                worker: self.worker.clone(),
                lease_ms: self.lease_ms,
            })?;
            return Ok(advanced(
                operation.id,
                ReconcileBoundary::LeaseAcquired,
                document.revision,
            ));
        }

        let lease = operation
            .lease
            .as_ref()
            .ok_or_else(|| Error::Invalid("selected operation has no lease".into()))?;
        if lease.owner != self.worker {
            return Ok(ReconcileOutcome::WaitingForLease {
                operation_id: operation.id.clone(),
                expires_at: lease.expires_at,
            });
        }
        let request = driver_request(&document, &operation)?;
        match operation.state {
            OperationState::Pending => Err(Error::Invalid(
                "pending operation unexpectedly has an active lease".into(),
            )),
            OperationState::Leased => {
                let document = self.repository.record_receipt(&ReceiptRequest {
                    context: self.context(at, document.revision, "prepared", &operation.id),
                    worker: self.worker.clone(),
                    lease_epoch: lease.epoch,
                    boundary: ReceiptBoundary::Prepared,
                    evidence_sha256: request.sha256(),
                    error: None,
                })?;
                Ok(advanced(
                    operation.id,
                    ReconcileBoundary::Prepared,
                    document.revision,
                ))
            }
            OperationState::Prepared => match self.driver.apply(&request) {
                Ok(effect) => {
                    validate_driver_digest(&effect.evidence_sha256)?;
                    let document = self.repository.record_receipt(&ReceiptRequest {
                        context: self.context(at, document.revision, "applied", &operation.id),
                        worker: self.worker.clone(),
                        lease_epoch: lease.epoch,
                        boundary: ReceiptBoundary::Applied,
                        evidence_sha256: effect.evidence_sha256,
                        error: None,
                    })?;
                    Ok(advanced(
                        operation.id,
                        ReconcileBoundary::Applied,
                        document.revision,
                    ))
                }
                Err(error) => {
                    self.handle_driver_error(&document, &operation, lease.epoch, error, at)
                }
            },
            OperationState::Applied => {
                let instance = document
                    .instance(&operation.instance_id)
                    .ok_or_else(|| Error::NotFound(operation.instance_id.to_string()))?;
                if instance.observed.generation == operation.desired_generation
                    && instance.observed.phase == ObservedPhase::Failed
                {
                    return self.fail_from_observation(
                        &document,
                        &operation,
                        lease.epoch,
                        instance.observed.evidence_sha256.clone(),
                        instance.observed.error.clone(),
                        at,
                    );
                }
                if observation_converged(&operation, &instance.desired, &instance.observed) {
                    let evidence = instance.observed.evidence_sha256.clone().ok_or_else(|| {
                        Error::Invalid("converged observation has no evidence digest".into())
                    })?;
                    let document = self.repository.record_receipt(&ReceiptRequest {
                        context: self.context(at, document.revision, "completed", &operation.id),
                        worker: self.worker.clone(),
                        lease_epoch: lease.epoch,
                        boundary: ReceiptBoundary::Completed,
                        evidence_sha256: evidence,
                        error: None,
                    })?;
                    return Ok(advanced(
                        operation.id,
                        ReconcileBoundary::Completed,
                        document.revision,
                    ));
                }
                match self.driver.observe(&request) {
                    Ok(observation) => {
                        validate_driver_digest(&observation.evidence_sha256)?;
                        let document =
                            self.repository
                                .record_observation(&crate::ObservationRequest {
                                    context: self.context(
                                        at,
                                        document.revision,
                                        "observed",
                                        &operation.id,
                                    ),
                                    worker: self.worker.clone(),
                                    lease_epoch: lease.epoch,
                                    phase: observation.phase,
                                    version: observation.version,
                                    process_id: observation.process_id,
                                    evidence_sha256: observation.evidence_sha256,
                                    error: observation.error,
                                })?;
                        Ok(advanced(
                            operation.id,
                            ReconcileBoundary::Observed,
                            document.revision,
                        ))
                    }
                    Err(error) => {
                        self.handle_driver_error(&document, &operation, lease.epoch, error, at)
                    }
                }
            }
            OperationState::Succeeded | OperationState::Failed | OperationState::Superseded => {
                Ok(ReconcileOutcome::Idle)
            }
        }
    }

    fn handle_driver_error(
        &self,
        document: &EstateDocument,
        operation: &EstateOperation,
        lease_epoch: u64,
        error: DriverError,
        at: u64,
    ) -> Result<ReconcileOutcome> {
        validate_driver_digest(&error.evidence_sha256)?;
        match error.kind {
            DriverErrorKind::Retryable => Ok(ReconcileOutcome::Deferred {
                operation_id: operation.id.clone(),
                message: error.message,
            }),
            DriverErrorKind::Permanent => {
                let updated = self.repository.record_receipt(&ReceiptRequest {
                    context: self.context(at, document.revision, "failed", &operation.id),
                    worker: self.worker.clone(),
                    lease_epoch,
                    boundary: ReceiptBoundary::Failed,
                    evidence_sha256: error.evidence_sha256,
                    error: Some(error.message),
                })?;
                Ok(advanced(
                    operation.id.clone(),
                    ReconcileBoundary::Failed,
                    updated.revision,
                ))
            }
        }
    }

    fn fail_from_observation(
        &self,
        document: &EstateDocument,
        operation: &EstateOperation,
        lease_epoch: u64,
        evidence: Option<String>,
        message: Option<String>,
        at: u64,
    ) -> Result<ReconcileOutcome> {
        let evidence = evidence
            .ok_or_else(|| Error::Invalid("failed observation has no evidence digest".into()))?;
        self.handle_driver_error(
            document,
            operation,
            lease_epoch,
            DriverError::permanent(
                message.unwrap_or_else(|| "driver observed a failed instance".into()),
                evidence,
            ),
            at,
        )
    }

    fn context(
        &self,
        at: u64,
        revision: u64,
        boundary: &str,
        operation_id: &CanonicalId,
    ) -> MutationContext {
        MutationContext {
            at,
            actor: self.worker.to_string(),
            request_id: format!("reconcile-{boundary}-{revision}"),
            operation_id: operation_id.clone(),
        }
    }
}

fn select_operation(
    document: &EstateDocument,
    worker: &CanonicalId,
    at: u64,
) -> Option<EstateOperation> {
    document
        .operations
        .values()
        .filter(|operation| !operation.state.is_terminal())
        .filter(|operation| {
            operation
                .lease
                .as_ref()
                .is_none_or(|lease| lease.expires_at <= at || lease.owner == *worker)
        })
        .min_by_key(|operation| (operation.created_at, operation.id.as_str()))
        .cloned()
}

fn oldest_open_operation(document: &EstateDocument) -> Option<&EstateOperation> {
    document
        .operations
        .values()
        .filter(|operation| !operation.state.is_terminal())
        .min_by_key(|operation| (operation.created_at, operation.id.as_str()))
}

fn driver_request(document: &EstateDocument, operation: &EstateOperation) -> Result<DriverRequest> {
    let instance = document
        .instance(&operation.instance_id)
        .ok_or_else(|| Error::NotFound(operation.instance_id.to_string()))?;
    if instance.desired.generation != operation.desired_generation {
        return Err(Error::Invalid(format!(
            "operation {} was superseded by desired generation {}",
            operation.id, instance.desired.generation
        )));
    }
    Ok(DriverRequest {
        estate_id: document.id.clone(),
        instance_id: operation.instance_id.clone(),
        operation_id: operation.id.clone(),
        kind: operation.kind,
        desired: instance.desired.clone(),
    })
}

fn observation_converged(
    operation: &EstateOperation,
    desired: &DesiredInstance,
    observed: &crate::ObservedInstance,
) -> bool {
    if observed.generation != operation.desired_generation {
        return false;
    }
    matches!(
        (desired.phase, observed.phase),
        (DesiredPhase::Running, ObservedPhase::Running)
            | (DesiredPhase::Stopped, ObservedPhase::Stopped)
            | (DesiredPhase::Absent, ObservedPhase::Absent)
    )
}

fn validate_driver_digest(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(Error::Invalid("driver evidence SHA-256 is invalid".into()));
    }
    Ok(())
}

fn advanced(
    operation_id: CanonicalId,
    boundary: ReconcileBoundary,
    revision: u64,
) -> ReconcileOutcome {
    ReconcileOutcome::Advanced {
        operation_id,
        boundary,
        revision,
    }
}
