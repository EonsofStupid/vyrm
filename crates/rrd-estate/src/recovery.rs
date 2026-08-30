use super::{
    validate_ascii_key, validate_context, validate_id_key, validate_sha256, Error, EstateDocument,
    EstateRepository, MutationContext, Result,
};
use rrd_contract::CanonicalId;
use rrd_core::digest;
use rrd_store::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_RECOVERY_POLICIES: usize = 1_024;
pub const MAX_RECOVERY_POINTS: usize = 4_096;
pub const MAX_RECOVERY_PINS: usize = 8_192;
pub const MAX_RESTORE_EVIDENCE: usize = 4_096;
pub const MAX_RECOVERY_IDEMPOTENCY_BINDINGS: usize = 8_192;
pub const MAX_RECOVERY_PRUNE_INTENTS: usize = 128;

const MIN_OBJECTIVE_MS: u64 = 1_000;
const MAX_OBJECTIVE_MS: u64 = 365 * 24 * 60 * 60 * 1_000;
const MAX_RETENTION_MS: u64 = 10 * 365 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateRecoveryPolicy {
    pub instance_id: CanonicalId,
    pub revision: u64,
    pub max_rpo_ms: u64,
    pub max_rto_ms: u64,
    pub minimum_recovery_points: u16,
    pub retention_ms: u64,
    pub updated_at: u64,
}

impl EstateRecoveryPolicy {
    pub(crate) fn default_for(instance_id: CanonicalId, at: u64) -> Self {
        Self {
            instance_id,
            revision: 1,
            max_rpo_ms: 24 * 60 * 60 * 1_000,
            max_rto_ms: 60 * 60 * 1_000,
            minimum_recovery_points: 1,
            retention_ms: 7 * 24 * 60 * 60 * 1_000,
            updated_at: at,
        }
    }

    fn validate(&self) -> Result<()> {
        if self.revision == 0
            || !(MIN_OBJECTIVE_MS..=MAX_OBJECTIVE_MS).contains(&self.max_rpo_ms)
            || !(MIN_OBJECTIVE_MS..=MAX_OBJECTIVE_MS).contains(&self.max_rto_ms)
            || !(1..=1_024).contains(&self.minimum_recovery_points)
            || self.retention_ms < self.max_rpo_ms
            || self.retention_ms > MAX_RETENTION_MS
            || self.updated_at == 0
        {
            return Err(Error::Invalid(format!(
                "recovery policy for {} is invalid",
                self.instance_id
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateRecoveryPoint {
    pub backup_id: String,
    pub instance_id: CanonicalId,
    pub backup_job_id: CanonicalId,
    pub source_generation: u64,
    pub archive_sha256: String,
    pub catalogue_sha256: String,
    pub source_cut_at: u64,
    pub completed_at: u64,
    pub policy_revision: u64,
    pub max_rpo_ms: u64,
    pub max_rto_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pruned_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstateRetentionPinKind {
    Policy,
    ExplicitHold,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateRetentionPin {
    pub id: CanonicalId,
    pub backup_id: String,
    pub kind: EstateRetentionPinKind,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_at: Option<u64>,
}

impl EstateRetentionPin {
    pub fn is_live(&self, at: u64) -> bool {
        self.released_at.is_none() && self.expires_at.is_none_or(|expires| expires > at)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateRestoreEvidence {
    pub restore_id: CanonicalId,
    pub instance_id: CanonicalId,
    pub backup_id: String,
    pub started_at: u64,
    pub completed_at: u64,
    pub duration_ms: u64,
    pub recovery_point_age_ms: u64,
    pub restored_claim_sequence: u64,
    pub restored_runtime_cursor: u64,
    pub closure_sha256: String,
    pub policy_revision: u64,
    pub rpo_within_objective: bool,
    pub rto_within_objective: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryIdempotencyBinding {
    pub operation_id: CanonicalId,
    pub request_sha256: String,
    pub bound_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstateRecoveryPruneIntent {
    pub id: CanonicalId,
    pub instance_id: CanonicalId,
    pub based_on_estate_revision: u64,
    pub evaluated_at: u64,
    pub expected_catalogue_sha256: String,
    pub retained_backup_ids: Vec<String>,
    pub prune_candidate_backup_ids: Vec<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct SetRecoveryPolicy {
    pub context: MutationContext,
    pub instance_id: CanonicalId,
    pub idempotency_key: String,
    pub max_rpo_ms: u64,
    pub max_rto_ms: u64,
    pub minimum_recovery_points: u16,
    pub retention_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PinRecoveryPoint {
    pub context: MutationContext,
    pub idempotency_key: String,
    pub backup_id: String,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ReleaseRecoveryPin {
    pub context: MutationContext,
    pub idempotency_key: String,
    pub pin_id: CanonicalId,
}

#[derive(Debug, Clone)]
pub struct RecordRestoreEvidence {
    pub context: MutationContext,
    pub idempotency_key: String,
    pub restore_id: CanonicalId,
    pub instance_id: CanonicalId,
    pub backup_id: String,
    pub started_at: u64,
    pub completed_at: u64,
    pub restored_claim_sequence: u64,
    pub restored_runtime_cursor: u64,
    pub closure_sha256: String,
}

#[derive(Debug, Clone)]
pub struct PrepareRecoveryPrune {
    pub context: MutationContext,
    pub idempotency_key: String,
    pub instance_id: CanonicalId,
    pub expected_estate_revision: u64,
    pub evaluated_at: u64,
    pub expected_catalogue_sha256: String,
    pub retained_backup_ids: Vec<String>,
    pub prune_candidate_backup_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CompleteRecoveryPrune {
    pub context: MutationContext,
    pub idempotency_key: String,
    pub intent_id: CanonicalId,
    pub resulting_catalogue_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryMutationOutcome {
    pub document: EstateDocument,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstateRetentionDecision {
    pub estate_revision: u64,
    pub instance_id: CanonicalId,
    pub evaluated_at: u64,
    pub retained_backup_ids: Vec<String>,
    pub prune_candidate_backup_ids: Vec<String>,
}

impl<'a, E: Engine + ?Sized> EstateRepository<'a, E> {
    pub fn set_recovery_policy(
        &self,
        request: &SetRecoveryPolicy,
    ) -> Result<RecoveryMutationOutcome> {
        let candidate = EstateRecoveryPolicy {
            instance_id: request.instance_id.clone(),
            revision: 1,
            max_rpo_ms: request.max_rpo_ms,
            max_rto_ms: request.max_rto_ms,
            minimum_recovery_points: request.minimum_recovery_points,
            retention_ms: request.retention_ms,
            updated_at: request.context.at,
        };
        candidate.validate()?;
        let request_sha256 = recovery_request_sha256(&(
            "recovery-policy-v1",
            &request.instance_id,
            request.max_rpo_ms,
            request.max_rto_ms,
            request.minimum_recovery_points,
            request.retention_ms,
        ));
        self.recovery_mutation(
            &request.context,
            &request.idempotency_key,
            request_sha256,
            "estate.recovery.policy.set",
            |document| {
                if !document
                    .instances
                    .contains_key(request.instance_id.as_str())
                {
                    return Err(Error::NotFound(request.instance_id.to_string()));
                }
                deny_active_recovery_prune(document, &request.instance_id)?;
                let revision = document
                    .recovery_policies
                    .get(request.instance_id.as_str())
                    .map(|policy| {
                        policy.revision.checked_add(1).ok_or_else(|| {
                            Error::Invalid("recovery policy revision overflow".into())
                        })
                    })
                    .transpose()?
                    .unwrap_or(1);
                let mut policy = candidate.clone();
                policy.revision = revision;
                document
                    .recovery_policies
                    .insert(request.instance_id.to_string(), policy);
                Ok(())
            },
        )
    }

    pub fn pin_recovery_point(
        &self,
        request: &PinRecoveryPoint,
    ) -> Result<RecoveryMutationOutcome> {
        validate_sha256(&request.backup_id, "recovery point")?;
        if request
            .expires_at
            .is_some_and(|expires| expires <= request.context.at)
        {
            return Err(Error::Invalid(
                "recovery pin expiry must be after creation".into(),
            ));
        }
        let request_sha256 =
            recovery_request_sha256(&("recovery-pin-v1", &request.backup_id, request.expires_at));
        self.recovery_mutation(
            &request.context,
            &request.idempotency_key,
            request_sha256,
            "estate.recovery.pin.created",
            |document| {
                let point = document
                    .recovery_points
                    .get(&request.backup_id)
                    .ok_or_else(|| Error::NotFound(request.backup_id.clone()))?;
                let instance_id = point.instance_id.clone();
                if point.pruned_at.is_some() {
                    return Err(Error::Invalid(
                        "pruned recovery point cannot be pinned".into(),
                    ));
                }
                deny_active_recovery_prune(document, &instance_id)?;
                let pin = EstateRetentionPin {
                    id: request.context.operation_id.clone(),
                    backup_id: request.backup_id.clone(),
                    kind: EstateRetentionPinKind::ExplicitHold,
                    created_at: request.context.at,
                    expires_at: request.expires_at,
                    released_at: None,
                };
                if document.recovery_pins.contains_key(pin.id.as_str()) {
                    return Err(Error::IdempotencyConflict(pin.id.to_string()));
                }
                document.recovery_pins.insert(pin.id.to_string(), pin);
                Ok(())
            },
        )
    }

    pub fn release_recovery_pin(
        &self,
        request: &ReleaseRecoveryPin,
    ) -> Result<RecoveryMutationOutcome> {
        let request_sha256 = recovery_request_sha256(&("recovery-pin-release-v1", &request.pin_id));
        self.recovery_mutation(
            &request.context,
            &request.idempotency_key,
            request_sha256,
            "estate.recovery.pin.released",
            |document| {
                let instance_id = document
                    .recovery_pins
                    .get(request.pin_id.as_str())
                    .and_then(|pin| document.recovery_points.get(&pin.backup_id))
                    .map(|point| point.instance_id.clone())
                    .ok_or_else(|| Error::NotFound(request.pin_id.to_string()))?;
                deny_active_recovery_prune(document, &instance_id)?;
                let pin = document
                    .recovery_pins
                    .get_mut(request.pin_id.as_str())
                    .ok_or_else(|| Error::NotFound(request.pin_id.to_string()))?;
                if pin.kind == EstateRetentionPinKind::Policy {
                    return Err(Error::Invalid(
                        "policy recovery pins expire through policy, not manual release".into(),
                    ));
                }
                if pin.released_at.is_some() {
                    return Err(Error::Invalid("recovery pin is already released".into()));
                }
                pin.released_at = Some(request.context.at);
                Ok(())
            },
        )
    }

    pub fn record_restore_evidence(
        &self,
        request: &RecordRestoreEvidence,
    ) -> Result<RecoveryMutationOutcome> {
        validate_sha256(&request.backup_id, "restore recovery point")?;
        validate_sha256(&request.closure_sha256, "restore closure")?;
        if request.started_at == 0
            || request.completed_at < request.started_at
            || request.context.at < request.completed_at
        {
            return Err(Error::Invalid("restore evidence time is invalid".into()));
        }
        let request_sha256 = recovery_request_sha256(&(
            "restore-evidence-v1",
            &request.restore_id,
            &request.instance_id,
            &request.backup_id,
            request.started_at,
            request.completed_at,
            request.restored_claim_sequence,
            request.restored_runtime_cursor,
            &request.closure_sha256,
        ));
        self.recovery_mutation(
            &request.context,
            &request.idempotency_key,
            request_sha256,
            "estate.recovery.restore.verified",
            |document| {
                let point = document
                    .recovery_points
                    .get(&request.backup_id)
                    .ok_or_else(|| Error::NotFound(request.backup_id.clone()))?;
                let point_instance = point.instance_id.clone();
                if point.instance_id != request.instance_id || point.pruned_at.is_some() {
                    return Err(Error::Invalid(
                        "restore recovery point is unavailable for the instance".into(),
                    ));
                }
                deny_active_recovery_prune(document, &point_instance)?;
                let duration_ms = request.completed_at - request.started_at;
                let recovery_point_age_ms = request
                    .started_at
                    .checked_sub(point.source_cut_at)
                    .ok_or_else(|| Error::Invalid("restore predates its recovery point".into()))?;
                let evidence = EstateRestoreEvidence {
                    restore_id: request.restore_id.clone(),
                    instance_id: request.instance_id.clone(),
                    backup_id: request.backup_id.clone(),
                    started_at: request.started_at,
                    completed_at: request.completed_at,
                    duration_ms,
                    recovery_point_age_ms,
                    restored_claim_sequence: request.restored_claim_sequence,
                    restored_runtime_cursor: request.restored_runtime_cursor,
                    closure_sha256: request.closure_sha256.clone(),
                    policy_revision: point.policy_revision,
                    rpo_within_objective: recovery_point_age_ms <= point.max_rpo_ms,
                    rto_within_objective: duration_ms <= point.max_rto_ms,
                };
                if document
                    .restore_evidence
                    .contains_key(request.restore_id.as_str())
                {
                    return Err(Error::IdempotencyConflict(request.restore_id.to_string()));
                }
                document
                    .restore_evidence
                    .insert(request.restore_id.to_string(), evidence);
                Ok(())
            },
        )
    }

    pub fn prepare_recovery_prune(
        &self,
        request: &PrepareRecoveryPrune,
    ) -> Result<RecoveryMutationOutcome> {
        validate_ordered_backup_ids(&request.retained_backup_ids, "retained recovery point")?;
        validate_ordered_backup_ids(
            &request.prune_candidate_backup_ids,
            "recovery prune candidate",
        )?;
        validate_sha256(
            &request.expected_catalogue_sha256,
            "recovery prune catalogue",
        )?;
        if request.prune_candidate_backup_ids.is_empty()
            || request.expected_estate_revision == 0
            || request.evaluated_at == 0
            || request.evaluated_at > request.context.at
        {
            return Err(Error::Invalid("recovery prune intent is invalid".into()));
        }
        let request_sha256 = recovery_request_sha256(&(
            "recovery-prune-prepare-v1",
            &request.instance_id,
            request.expected_estate_revision,
            request.evaluated_at,
            &request.expected_catalogue_sha256,
            &request.retained_backup_ids,
            &request.prune_candidate_backup_ids,
        ));
        self.recovery_mutation(
            &request.context,
            &request.idempotency_key,
            request_sha256,
            "estate.recovery.prune.prepared",
            |document| {
                if document.revision != request.expected_estate_revision {
                    return Err(Error::Invalid(
                        "recovery prune estate revision is stale".into(),
                    ));
                }
                deny_active_recovery_prune(document, &request.instance_id)?;
                let decision =
                    retention_decision(document, &request.instance_id, request.evaluated_at)?;
                if decision.retained_backup_ids != request.retained_backup_ids
                    || decision.prune_candidate_backup_ids != request.prune_candidate_backup_ids
                {
                    return Err(Error::Invalid(
                        "recovery prune decision no longer matches estate policy".into(),
                    ));
                }
                if document.recovery_prune_intents.len() == MAX_RECOVERY_PRUNE_INTENTS {
                    return Err(Error::Invalid(
                        "recovery prune intent history is at its v1 bound".into(),
                    ));
                }
                let intent = EstateRecoveryPruneIntent {
                    id: request.context.operation_id.clone(),
                    instance_id: request.instance_id.clone(),
                    based_on_estate_revision: request.expected_estate_revision,
                    evaluated_at: request.evaluated_at,
                    expected_catalogue_sha256: request.expected_catalogue_sha256.clone(),
                    retained_backup_ids: request.retained_backup_ids.clone(),
                    prune_candidate_backup_ids: request.prune_candidate_backup_ids.clone(),
                    created_at: request.context.at,
                };
                document
                    .recovery_prune_intents
                    .insert(intent.id.to_string(), intent);
                Ok(())
            },
        )
    }

    pub fn complete_recovery_prune(
        &self,
        request: &CompleteRecoveryPrune,
    ) -> Result<RecoveryMutationOutcome> {
        validate_sha256(
            &request.resulting_catalogue_sha256,
            "completed recovery prune catalogue",
        )?;
        let request_sha256 = recovery_request_sha256(&(
            "recovery-prune-complete-v1",
            &request.intent_id,
            &request.resulting_catalogue_sha256,
        ));
        self.recovery_mutation(
            &request.context,
            &request.idempotency_key,
            request_sha256,
            "estate.recovery.prune.completed",
            |document| {
                let intent = document
                    .recovery_prune_intents
                    .get(request.intent_id.as_str())
                    .cloned()
                    .ok_or_else(|| Error::NotFound(request.intent_id.to_string()))?;
                if request.context.at < intent.created_at
                    || request.resulting_catalogue_sha256 == intent.expected_catalogue_sha256
                {
                    return Err(Error::Invalid(
                        "completed recovery prune evidence is invalid".into(),
                    ));
                }
                for backup_id in &intent.prune_candidate_backup_ids {
                    let point = document
                        .recovery_points
                        .get_mut(backup_id)
                        .ok_or_else(|| Error::NotFound(backup_id.clone()))?;
                    point.pruned_at = Some(request.context.at);
                }
                document
                    .recovery_prune_intents
                    .remove(request.intent_id.as_str());
                Ok(())
            },
        )
    }

    fn recovery_mutation(
        &self,
        context: &MutationContext,
        idempotency_key: &str,
        request_sha256: String,
        action: &str,
        mutate: impl FnOnce(&mut EstateDocument) -> Result<()>,
    ) -> Result<RecoveryMutationOutcome> {
        validate_context(context)?;
        validate_ascii_key(idempotency_key, "recovery idempotency key")?;
        let Some(current_bytes) = self.engine.control_record(&self.key)? else {
            return Err(Error::NotFound(self.estate_id.to_string()));
        };
        let mut document = super::decode(&current_bytes)?;
        if let Some(binding) = document.recovery_idempotency.get(idempotency_key) {
            if binding.operation_id != context.operation_id
                || binding.request_sha256 != request_sha256
            {
                return Err(Error::IdempotencyConflict(idempotency_key.into()));
            }
            return Ok(RecoveryMutationOutcome {
                document,
                idempotent_replay: true,
            });
        }
        if document
            .recovery_idempotency
            .values()
            .any(|binding| binding.operation_id == context.operation_id)
            || document
                .operations
                .contains_key(context.operation_id.as_str())
            || document
                .backup_jobs
                .contains_key(context.operation_id.as_str())
        {
            return Err(Error::IdempotencyConflict(context.operation_id.to_string()));
        }
        mutate(&mut document)?;
        document.recovery_idempotency.insert(
            idempotency_key.into(),
            RecoveryIdempotencyBinding {
                operation_id: context.operation_id.clone(),
                request_sha256,
                bound_at: context.at,
            },
        );
        let document = self.commit(current_bytes, document, context, action)?;
        Ok(RecoveryMutationOutcome {
            document,
            idempotent_replay: false,
        })
    }
}

pub fn retention_decision(
    document: &EstateDocument,
    instance_id: &CanonicalId,
    at: u64,
) -> Result<EstateRetentionDecision> {
    if at == 0 {
        return Err(Error::Invalid(
            "retention evaluation time must be non-zero".into(),
        ));
    }
    let policy = document
        .recovery_policies
        .get(instance_id.as_str())
        .ok_or_else(|| Error::NotFound(instance_id.to_string()))?;
    let mut points = document
        .recovery_points
        .values()
        .filter(|point| point.instance_id == *instance_id && point.pruned_at.is_none())
        .collect::<Vec<_>>();
    points.sort_by(|left, right| {
        (right.source_cut_at, &right.backup_id).cmp(&(left.source_cut_at, &left.backup_id))
    });
    let mut retained = document
        .recovery_pins
        .values()
        .filter(|pin| pin.is_live(at))
        .map(|pin| pin.backup_id.clone())
        .collect::<BTreeSet<_>>();
    retained.extend(
        points
            .iter()
            .take(usize::from(policy.minimum_recovery_points))
            .map(|point| point.backup_id.clone()),
    );
    if let Some(newest) = points.first() {
        retained.insert(newest.backup_id.clone());
    }
    let available = points
        .iter()
        .map(|point| point.backup_id.as_str())
        .collect::<BTreeSet<_>>();
    retained.retain(|backup_id| available.contains(backup_id.as_str()));
    let mut retained_backup_ids = retained.iter().cloned().collect::<Vec<_>>();
    retained_backup_ids.sort();
    let mut prune_candidate_backup_ids = points
        .iter()
        .filter(|point| !retained.contains(&point.backup_id))
        .map(|point| point.backup_id.clone())
        .collect::<Vec<_>>();
    prune_candidate_backup_ids.sort();
    Ok(EstateRetentionDecision {
        estate_revision: document.revision,
        instance_id: instance_id.clone(),
        evaluated_at: at,
        retained_backup_ids,
        prune_candidate_backup_ids,
    })
}

pub fn public_recovery_snapshot(document: &EstateDocument) -> rrd_contract::EstateRecoverySnapshot {
    rrd_contract::EstateRecoverySnapshot {
        estate_id: document.id.clone(),
        estate_revision: document.revision,
        policies: document
            .recovery_policies
            .values()
            .map(|policy| rrd_contract::EstateRecoveryPolicySnapshot {
                instance_id: policy.instance_id.clone(),
                revision: policy.revision,
                max_rpo_ms: policy.max_rpo_ms,
                max_rto_ms: policy.max_rto_ms,
                minimum_recovery_points: policy.minimum_recovery_points,
                retention_ms: policy.retention_ms,
                updated_at_unix_ms: policy.updated_at,
            })
            .collect(),
        recovery_points: document
            .recovery_points
            .values()
            .map(|point| rrd_contract::EstateRecoveryPointSnapshot {
                backup_sha256: point.backup_id.clone(),
                instance_id: point.instance_id.clone(),
                backup_job_id: point.backup_job_id.clone(),
                source_generation: point.source_generation,
                archive_sha256: point.archive_sha256.clone(),
                catalogue_sha256: point.catalogue_sha256.clone(),
                source_cut_at_unix_ms: point.source_cut_at,
                completed_at_unix_ms: point.completed_at,
                policy_revision: point.policy_revision,
                max_rpo_ms: point.max_rpo_ms,
                max_rto_ms: point.max_rto_ms,
                pruned_at_unix_ms: point.pruned_at,
            })
            .collect(),
        retention_pins: document
            .recovery_pins
            .values()
            .map(|pin| rrd_contract::EstateRetentionPinSnapshot {
                id: pin.id.clone(),
                backup_sha256: pin.backup_id.clone(),
                kind: match pin.kind {
                    EstateRetentionPinKind::Policy => {
                        rrd_contract::EstateRetentionPinKindSnapshot::Policy
                    }
                    EstateRetentionPinKind::ExplicitHold => {
                        rrd_contract::EstateRetentionPinKindSnapshot::ExplicitHold
                    }
                },
                created_at_unix_ms: pin.created_at,
                expires_at_unix_ms: pin.expires_at,
                released_at_unix_ms: pin.released_at,
            })
            .collect(),
        restore_evidence: document
            .restore_evidence
            .values()
            .map(|evidence| rrd_contract::EstateRestoreEvidenceSnapshot {
                restore_id: evidence.restore_id.clone(),
                instance_id: evidence.instance_id.clone(),
                backup_sha256: evidence.backup_id.clone(),
                started_at_unix_ms: evidence.started_at,
                completed_at_unix_ms: evidence.completed_at,
                duration_ms: evidence.duration_ms,
                recovery_point_age_ms: evidence.recovery_point_age_ms,
                restored_claim_sequence: evidence.restored_claim_sequence,
                restored_runtime_cursor: evidence.restored_runtime_cursor,
                closure_sha256: evidence.closure_sha256.clone(),
                policy_revision: evidence.policy_revision,
                rpo_within_objective: evidence.rpo_within_objective,
                rto_within_objective: evidence.rto_within_objective,
            })
            .collect(),
        prune_intents: document
            .recovery_prune_intents
            .values()
            .map(|intent| rrd_contract::EstateRecoveryPruneIntentSnapshot {
                id: intent.id.clone(),
                instance_id: intent.instance_id.clone(),
                based_on_estate_revision: intent.based_on_estate_revision,
                evaluated_at_unix_ms: intent.evaluated_at,
                expected_catalogue_sha256: intent.expected_catalogue_sha256.clone(),
                retained_backup_ids: intent.retained_backup_ids.clone(),
                prune_candidate_backup_ids: intent.prune_candidate_backup_ids.clone(),
                created_at_unix_ms: intent.created_at,
            })
            .collect(),
    }
}

pub fn public_retention_decision(
    decision: &EstateRetentionDecision,
) -> rrd_contract::EstateRetentionDecisionSnapshot {
    rrd_contract::EstateRetentionDecisionSnapshot {
        estate_revision: decision.estate_revision,
        instance_id: decision.instance_id.clone(),
        evaluated_at_unix_ms: decision.evaluated_at,
        retained_backup_ids: decision.retained_backup_ids.clone(),
        prune_candidate_backup_ids: decision.prune_candidate_backup_ids.clone(),
    }
}

pub(crate) fn ensure_recovery_policy(
    document: &mut EstateDocument,
    instance_id: &CanonicalId,
    at: u64,
) -> Result<EstateRecoveryPolicy> {
    if let Some(policy) = document.recovery_policies.get(instance_id.as_str()) {
        return Ok(policy.clone());
    }
    if document.recovery_policies.len() == MAX_RECOVERY_POLICIES {
        return Err(Error::Invalid("recovery policy limit exceeded".into()));
    }
    let policy = EstateRecoveryPolicy::default_for(instance_id.clone(), at);
    document
        .recovery_policies
        .insert(instance_id.to_string(), policy.clone());
    Ok(policy)
}

pub(crate) fn deny_active_recovery_prune(
    document: &EstateDocument,
    instance_id: &CanonicalId,
) -> Result<()> {
    if document
        .recovery_prune_intents
        .values()
        .any(|intent| intent.instance_id == *instance_id)
    {
        return Err(Error::Invalid(format!(
            "instance {instance_id} has an active recovery prune intent"
        )));
    }
    Ok(())
}

pub(crate) fn record_completed_recovery_point(
    document: &mut EstateDocument,
    job_id: &CanonicalId,
    completed_at: u64,
) -> Result<()> {
    let job = document
        .backup_jobs
        .get(job_id.as_str())
        .ok_or_else(|| Error::NotFound(job_id.to_string()))?;
    let backup_id = job
        .backup_id
        .clone()
        .ok_or_else(|| Error::Invalid("completed backup has no identity".into()))?;
    let archive_sha256 = job
        .archive_sha256
        .clone()
        .ok_or_else(|| Error::Invalid("completed backup has no archive".into()))?;
    let catalogue_sha256 = job
        .catalogue_sha256
        .clone()
        .ok_or_else(|| Error::Invalid("completed backup has no catalogue".into()))?;
    let policy = job
        .recovery_policy
        .as_ref()
        .ok_or_else(|| Error::Invalid("legacy backup job has no recovery policy binding".into()))?;
    let point = EstateRecoveryPoint {
        backup_id: backup_id.clone(),
        instance_id: job.instance_id.clone(),
        backup_job_id: job.id.clone(),
        source_generation: job.source_generation,
        archive_sha256,
        catalogue_sha256,
        source_cut_at: job.created_at,
        completed_at,
        policy_revision: policy.revision,
        max_rpo_ms: policy.max_rpo_ms,
        max_rto_ms: policy.max_rto_ms,
        pruned_at: None,
    };
    if let Some(existing) = document.recovery_points.get(&backup_id) {
        if existing != &point {
            return Err(Error::Invalid(
                "backup identity recovery point conflict".into(),
            ));
        }
        return Ok(());
    }
    if document.recovery_points.len() == MAX_RECOVERY_POINTS
        || document.recovery_pins.len() == MAX_RECOVERY_PINS
    {
        return Err(Error::Invalid("recovery history limit exceeded".into()));
    }
    let pin_id = CanonicalId::new(format!("policy-{backup_id}"))
        .map_err(|error| Error::Invalid(error.to_string()))?;
    let expires_at = completed_at
        .checked_add(policy.retention_ms)
        .ok_or_else(|| Error::Invalid("recovery retention expiry overflow".into()))?;
    document.recovery_points.insert(backup_id.clone(), point);
    document.recovery_pins.insert(
        pin_id.to_string(),
        EstateRetentionPin {
            id: pin_id,
            backup_id,
            kind: EstateRetentionPinKind::Policy,
            created_at: completed_at,
            expires_at: Some(expires_at),
            released_at: None,
        },
    );
    Ok(())
}

pub(crate) fn validate_recovery_state(document: &EstateDocument) -> Result<()> {
    if document.recovery_policies.len() > MAX_RECOVERY_POLICIES
        || document.recovery_points.len() > MAX_RECOVERY_POINTS
        || document.recovery_pins.len() > MAX_RECOVERY_PINS
        || document.restore_evidence.len() > MAX_RESTORE_EVIDENCE
        || document.recovery_idempotency.len() > MAX_RECOVERY_IDEMPOTENCY_BINDINGS
        || document.recovery_prune_intents.len() > MAX_RECOVERY_PRUNE_INTENTS
    {
        return Err(Error::Invalid(
            "estate recovery cardinality exceeded".into(),
        ));
    }
    for (key, policy) in &document.recovery_policies {
        validate_id_key(key, &policy.instance_id)?;
        policy.validate()?;
        if !document.instances.contains_key(key) {
            return Err(Error::Invalid(format!(
                "recovery policy {key} names an unknown instance"
            )));
        }
    }
    for (key, point) in &document.recovery_points {
        if key != &point.backup_id {
            return Err(Error::Invalid("recovery point key is invalid".into()));
        }
        validate_sha256(&point.backup_id, "recovery point")?;
        validate_sha256(&point.archive_sha256, "recovery archive")?;
        validate_sha256(&point.catalogue_sha256, "recovery catalogue")?;
        let job = document
            .backup_jobs
            .get(point.backup_job_id.as_str())
            .ok_or_else(|| Error::Invalid(format!("recovery point {key} has no backup job")))?;
        let policy = job
            .recovery_policy
            .as_ref()
            .ok_or_else(|| Error::Invalid(format!("recovery point {key} has no policy binding")))?;
        if point.source_generation == 0
            || point.source_cut_at == 0
            || point.completed_at < point.source_cut_at
            || point.policy_revision == 0
            || point.max_rpo_ms == 0
            || point.max_rto_ms == 0
            || !document.instances.contains_key(point.instance_id.as_str())
            || job.state != super::BackupJobState::Succeeded
            || job.backup_id.as_deref() != Some(point.backup_id.as_str())
            || job.instance_id != point.instance_id
            || job.source_generation != point.source_generation
            || job.created_at != point.source_cut_at
            || job.updated_at != point.completed_at
            || job.archive_sha256.as_deref() != Some(point.archive_sha256.as_str())
            || job.catalogue_sha256.as_deref() != Some(point.catalogue_sha256.as_str())
            || policy.revision != point.policy_revision
            || policy.max_rpo_ms != point.max_rpo_ms
            || policy.max_rto_ms != point.max_rto_ms
            || point.pruned_at.is_some_and(|at| at < point.completed_at)
        {
            return Err(Error::Invalid(format!("recovery point {key} is invalid")));
        }
    }
    for job in document.backup_jobs.values().filter(|job| {
        job.state == super::BackupJobState::Succeeded && job.recovery_policy.is_some()
    }) {
        let backup_id = job
            .backup_id
            .as_deref()
            .ok_or_else(|| Error::Invalid("succeeded backup has no identity".into()))?;
        if !document.recovery_points.contains_key(backup_id) {
            return Err(Error::Invalid(format!(
                "policy-bound backup {} has no recovery point",
                job.id
            )));
        }
    }
    for (key, pin) in &document.recovery_pins {
        validate_id_key(key, &pin.id)?;
        validate_sha256(&pin.backup_id, "recovery pin point")?;
        if pin.created_at == 0
            || pin.expires_at.is_some_and(|at| at <= pin.created_at)
            || pin.released_at.is_some_and(|at| at < pin.created_at)
            || !document.recovery_points.contains_key(&pin.backup_id)
            || document
                .recovery_points
                .get(&pin.backup_id)
                .and_then(|point| point.pruned_at)
                .is_some_and(|pruned_at| pin.is_live(pruned_at))
        {
            return Err(Error::Invalid(format!("recovery pin {key} is invalid")));
        }
    }
    for point in document.recovery_points.values() {
        let policy_pins = document
            .recovery_pins
            .values()
            .filter(|pin| {
                pin.backup_id == point.backup_id && pin.kind == EstateRetentionPinKind::Policy
            })
            .collect::<Vec<_>>();
        let job = &document.backup_jobs[point.backup_job_id.as_str()];
        let policy = job
            .recovery_policy
            .as_ref()
            .expect("recovery point validation established its policy");
        let expected_expiry = point
            .completed_at
            .checked_add(policy.retention_ms)
            .ok_or_else(|| Error::Invalid("recovery policy expiry overflow".into()))?;
        if policy_pins.len() != 1
            || policy_pins[0].created_at != point.completed_at
            || policy_pins[0].expires_at != Some(expected_expiry)
            || policy_pins[0].released_at.is_some()
        {
            return Err(Error::Invalid(format!(
                "recovery point {} must have exactly one policy pin",
                point.backup_id
            )));
        }
    }
    for (key, evidence) in &document.restore_evidence {
        validate_id_key(key, &evidence.restore_id)?;
        validate_sha256(&evidence.backup_id, "restore recovery point")?;
        validate_sha256(&evidence.closure_sha256, "restore closure")?;
        let point = document
            .recovery_points
            .get(&evidence.backup_id)
            .ok_or_else(|| Error::Invalid(format!("restore evidence {key} has no point")))?;
        if evidence.started_at == 0
            || evidence.completed_at < evidence.started_at
            || evidence.duration_ms != evidence.completed_at - evidence.started_at
            || evidence.policy_revision == 0
            || evidence.instance_id != point.instance_id
            || evidence.recovery_point_age_ms
                != evidence.started_at.saturating_sub(point.source_cut_at)
            || evidence.policy_revision != point.policy_revision
            || evidence.rpo_within_objective != (evidence.recovery_point_age_ms <= point.max_rpo_ms)
            || evidence.rto_within_objective != (evidence.duration_ms <= point.max_rto_ms)
            || point
                .pruned_at
                .is_some_and(|pruned_at| evidence.completed_at > pruned_at)
        {
            return Err(Error::Invalid(format!("restore evidence {key} is invalid")));
        }
    }
    for (key, binding) in &document.recovery_idempotency {
        validate_ascii_key(key, "recovery idempotency key")?;
        validate_sha256(&binding.request_sha256, "recovery idempotency request")?;
        if binding.bound_at == 0 {
            return Err(Error::Invalid(
                "recovery idempotency binding time is invalid".into(),
            ));
        }
    }
    let mut prune_instances = BTreeSet::new();
    for (key, intent) in &document.recovery_prune_intents {
        validate_id_key(key, &intent.id)?;
        validate_sha256(
            &intent.expected_catalogue_sha256,
            "recovery prune catalogue",
        )?;
        validate_ordered_backup_ids(&intent.retained_backup_ids, "retained recovery point")?;
        validate_ordered_backup_ids(
            &intent.prune_candidate_backup_ids,
            "recovery prune candidate",
        )?;
        if intent.based_on_estate_revision == 0
            || intent.evaluated_at == 0
            || intent.created_at < intent.evaluated_at
            || intent.prune_candidate_backup_ids.is_empty()
            || !document.instances.contains_key(intent.instance_id.as_str())
            || !prune_instances.insert(intent.instance_id.as_str())
            || intent
                .retained_backup_ids
                .iter()
                .any(|backup_id| !document.recovery_points.contains_key(backup_id))
            || intent
                .prune_candidate_backup_ids
                .iter()
                .any(|backup_id| !document.recovery_points.contains_key(backup_id))
        {
            return Err(Error::Invalid(format!(
                "recovery prune intent {key} is invalid"
            )));
        }
    }
    Ok(())
}

fn recovery_request_sha256(value: &impl Serialize) -> String {
    digest::sha256_hex(&serde_json::to_vec(value).expect("validated recovery request serializes"))
}

fn validate_ordered_backup_ids(values: &[String], name: &str) -> Result<()> {
    let mut previous: Option<&str> = None;
    for value in values {
        validate_sha256(value, name)?;
        if previous.is_some_and(|prior| prior >= value.as_str()) {
            return Err(Error::Invalid(format!(
                "{name} identities are not uniquely ordered"
            )));
        }
        previous = Some(value);
    }
    Ok(())
}

pub(crate) fn empty_recovery_policies() -> BTreeMap<String, EstateRecoveryPolicy> {
    BTreeMap::new()
}

pub(crate) fn empty_recovery_points() -> BTreeMap<String, EstateRecoveryPoint> {
    BTreeMap::new()
}

pub(crate) fn empty_recovery_pins() -> BTreeMap<String, EstateRetentionPin> {
    BTreeMap::new()
}

pub(crate) fn empty_restore_evidence() -> BTreeMap<String, EstateRestoreEvidence> {
    BTreeMap::new()
}

pub(crate) fn empty_recovery_idempotency() -> BTreeMap<String, RecoveryIdempotencyBinding> {
    BTreeMap::new()
}

pub(crate) fn empty_recovery_prune_intents() -> BTreeMap<String, EstateRecoveryPruneIntent> {
    BTreeMap::new()
}
