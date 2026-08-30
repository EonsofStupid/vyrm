//! Governed, replayable context maintenance for RRFlow instances.
//!
//! Maintenance changes a rebuildable AI context projection. It never deletes
//! the claim log, typed runtime log, source repository, or authenticated backup
//! that protects the source cut.

use rrd_contract::CanonicalId;
use rrd_core::{
    digest, RuntimeCommit, RuntimeEvent, RuntimeEventSchema, RuntimeMutation, RuntimeProperties,
    RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry,
    RuntimeType, RuntimeValue, RuntimeValueType, ScopeId,
};
use rrd_store::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAINTENANCE_FORMAT_VERSION: u16 = 1;
pub const DEFAULT_TARGET_REDUCTION_BPS: u16 = 6_500;
pub const MIN_TARGET_REDUCTION_BPS: u16 = 5_000;
pub const MAX_TARGET_REDUCTION_BPS: u16 = 7_500;
const RUN_TYPE: &str = "context_maintenance_run";
const EVENT_TYPE: &str = "context_maintenance_event";
const PROJECTION_TYPE: &str = "context_maintenance_projection";
const ACTIVE_PROJECTION_ID: &str = "active";
const REPLAY_PAGE: usize = 1_024;
const MAX_TEXT_BYTES: usize = 16 * 1024;

pub type Result<T> = std::result::Result<T, MaintenanceError>;

#[derive(Debug, thiserror::Error)]
pub enum MaintenanceError {
    #[error("maintenance contract: {0}")]
    Contract(String),
    #[error("maintenance storage: {0}")]
    Storage(#[from] rrd_store::Error),
    #[error("maintenance encoding: {0}")]
    Encoding(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceStage {
    Inventory,
    Protect,
    Propose,
    Review,
    Validate,
    Apply,
    Observe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceStatus {
    Active,
    ReadyToApply,
    Applied,
    Completed,
    RolledBack,
    Cancelled,
}

impl MaintenanceStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::RolledBack | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryClass {
    ActiveGoalsConstraints,
    CanonicalDecisions,
    CompletedAttempts,
    SupersededPlans,
    DuplicateObservations,
    FailureLessons,
    ProviderEnvelopes,
    ToolOutputs,
}

impl HistoryClass {
    pub const ALL: [Self; 8] = [
        Self::ActiveGoalsConstraints,
        Self::CanonicalDecisions,
        Self::CompletedAttempts,
        Self::SupersededPlans,
        Self::DuplicateObservations,
        Self::FailureLessons,
        Self::ProviderEnvelopes,
        Self::ToolOutputs,
    ];

    pub const fn default_disposition(self) -> ContextDisposition {
        match self {
            Self::ActiveGoalsConstraints | Self::CanonicalDecisions => ContextDisposition::Hot,
            Self::CompletedAttempts | Self::FailureLessons => ContextDisposition::Warm,
            Self::SupersededPlans
            | Self::DuplicateObservations
            | Self::ProviderEnvelopes
            | Self::ToolOutputs => ContextDisposition::Cold,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextDisposition {
    Hot,
    Warm,
    Cold,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryClassInventory {
    pub class: HistoryClass,
    pub items: u64,
    pub source_bytes: u64,
    pub estimated_tokens: u64,
    pub derived_from: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceInventory {
    pub claim_sequence: u64,
    pub runtime_cursor: u64,
    pub source_bytes: u64,
    pub estimated_tokens: u64,
    pub classes: Vec<HistoryClassInventory>,
}

impl MaintenanceInventory {
    pub fn validate(&self) -> Result<()> {
        exact_classes(self.classes.iter().map(|entry| entry.class))?;
        let mut bytes = 0_u64;
        let mut tokens = 0_u64;
        for entry in &self.classes {
            text_field("inventory.derived_from", &entry.derived_from, false)?;
            bytes = bytes
                .checked_add(entry.source_bytes)
                .ok_or_else(|| contract("inventory source byte count overflow"))?;
            tokens = tokens
                .checked_add(entry.estimated_tokens)
                .ok_or_else(|| contract("inventory token count overflow"))?;
        }
        if bytes != self.source_bytes || tokens != self.estimated_tokens {
            return Err(contract("inventory totals do not match class totals"));
        }
        if self.estimated_tokens == 0 {
            return Err(contract(
                "maintenance inventory must contain estimated context tokens",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceDecision {
    pub class: HistoryClass,
    pub proposed: ContextDisposition,
    pub selected: ContextDisposition,
    pub rationale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_note: Option<String>,
}

impl MaintenanceDecision {
    fn validate(&self) -> Result<()> {
        text_field("decision.rationale", &self.rationale, false)?;
        if let Some(note) = &self.operator_note {
            text_field("decision.operator_note", note, false)?;
        }
        if self.proposed != self.selected
            && self
                .operator_note
                .as_deref()
                .is_none_or(|note| note.trim().is_empty())
        {
            return Err(contract("every operator override requires a retained note"));
        }
        match self.class {
            HistoryClass::ActiveGoalsConstraints if self.selected != ContextDisposition::Hot => {
                Err(contract("active goals and constraints must remain hot"))
            }
            HistoryClass::CanonicalDecisions | HistoryClass::FailureLessons
                if self.selected == ContextDisposition::Cold =>
            {
                Err(contract(
                    "canonical decisions and failure lessons may not become cold",
                ))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionPlan {
    pub target_reduction_bps: u16,
    pub estimated_reduction_bps: u16,
    pub source_tokens: u64,
    pub hot_tokens: u64,
    pub warm_tokens: u64,
    pub cold_tokens: u64,
    pub decisions: Vec<MaintenanceDecision>,
    pub projection_sha256: String,
}

impl ProjectionPlan {
    pub fn build(
        inventory: &MaintenanceInventory,
        target_reduction_bps: u16,
        decisions: Vec<MaintenanceDecision>,
    ) -> Result<Self> {
        inventory.validate()?;
        target(target_reduction_bps)?;
        validate_decisions(&decisions)?;
        let estimates = inventory
            .classes
            .iter()
            .map(|entry| (entry.class, entry.estimated_tokens))
            .collect::<BTreeMap<_, _>>();
        let mut hot_tokens = 0_u64;
        let mut warm_tokens = 0_u64;
        let mut cold_tokens = 0_u64;
        for decision in &decisions {
            let tokens = estimates[&decision.class];
            let destination = match decision.selected {
                ContextDisposition::Hot => &mut hot_tokens,
                ContextDisposition::Warm => &mut warm_tokens,
                ContextDisposition::Cold => &mut cold_tokens,
            };
            *destination = destination
                .checked_add(tokens)
                .ok_or_else(|| contract("projection token count overflow"))?;
        }
        let removed = inventory
            .estimated_tokens
            .checked_sub(hot_tokens)
            .ok_or_else(|| contract("hot token count exceeds source inventory"))?;
        let estimated_reduction_bps = u16::try_from(
            removed
                .checked_mul(10_000)
                .ok_or_else(|| contract("projection reduction overflow"))?
                / inventory.estimated_tokens,
        )
        .map_err(|_| contract("projection reduction exceeds basis points"))?;
        if !(MIN_TARGET_REDUCTION_BPS..=MAX_TARGET_REDUCTION_BPS).contains(&estimated_reduction_bps)
        {
            return Err(contract(
                "selected dispositions must produce an estimated 50%..=75% active-context reduction",
            ));
        }
        let mut plan = Self {
            target_reduction_bps,
            estimated_reduction_bps,
            source_tokens: inventory.estimated_tokens,
            hot_tokens,
            warm_tokens,
            cold_tokens,
            decisions,
            projection_sha256: String::new(),
        };
        plan.projection_sha256 = plan_digest(&plan)?;
        plan.validate(inventory)?;
        Ok(plan)
    }

    pub fn validate(&self, inventory: &MaintenanceInventory) -> Result<()> {
        target(self.target_reduction_bps)?;
        validate_sha256(&self.projection_sha256, "projection_sha256")?;
        validate_decisions(&self.decisions)?;
        if self.source_tokens != inventory.estimated_tokens
            || self
                .hot_tokens
                .checked_add(self.warm_tokens)
                .and_then(|value| value.checked_add(self.cold_tokens))
                != Some(self.source_tokens)
            || !(MIN_TARGET_REDUCTION_BPS..=MAX_TARGET_REDUCTION_BPS)
                .contains(&self.estimated_reduction_bps)
            || plan_digest(self)? != self.projection_sha256
        {
            return Err(contract("projection plan totals or digest do not verify"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectionEvidence {
    pub backup_id: String,
    pub archive_sha256: String,
    pub catalogue_sha256: String,
    pub claim_sequence: u64,
    pub runtime_cursor: u64,
    pub verified_at: u64,
}

impl ProtectionEvidence {
    fn validate(&self, inventory: &MaintenanceInventory) -> Result<()> {
        validate_sha256(&self.backup_id, "backup_id")?;
        validate_sha256(&self.archive_sha256, "archive_sha256")?;
        validate_sha256(&self.catalogue_sha256, "catalogue_sha256")?;
        if self.verified_at == 0
            || self.claim_sequence < inventory.claim_sequence
            || self.runtime_cursor < inventory.runtime_cursor
        {
            return Err(contract("backup does not cover the inventory source cut"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationGateKind {
    ArchiveIntegrity,
    ActiveConstraintRecall,
    CanonicalDecisionRecall,
    ProjectionDeterminism,
    TaskReplay,
    RegressionDifferential,
}

impl ValidationGateKind {
    pub const ALL: [Self; 6] = [
        Self::ArchiveIntegrity,
        Self::ActiveConstraintRecall,
        Self::CanonicalDecisionRecall,
        Self::ProjectionDeterminism,
        Self::TaskReplay,
        Self::RegressionDifferential,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationGateStatus {
    Pending,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationGate {
    pub kind: ValidationGateKind,
    pub status: ValidationGateStatus,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_sha256: Option<String>,
}

impl ValidationGate {
    fn validate(&self) -> Result<()> {
        text_field("validation.summary", &self.summary, true)?;
        match (self.status, self.evidence_sha256.as_deref()) {
            (ValidationGateStatus::Pending, None) => Ok(()),
            (ValidationGateStatus::Passed | ValidationGateStatus::Failed, Some(digest)) => {
                validate_sha256(digest, "validation.evidence_sha256")
            }
            _ => Err(contract(
                "pending gates omit evidence; passed and failed gates require evidence",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationDecision {
    Accept,
    RollBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceObservation {
    pub decision: ObservationDecision,
    pub observed_at: u64,
    pub task_success_bps: u16,
    pub context_tokens: u64,
    pub latency_ms: u64,
    pub summary: String,
    pub evidence_sha256: String,
}

impl MaintenanceObservation {
    fn validate(&self) -> Result<()> {
        if self.observed_at == 0 || self.task_success_bps > 10_000 {
            return Err(contract("observation time or task success is invalid"));
        }
        text_field("observation.summary", &self.summary, false)?;
        validate_sha256(&self.evidence_sha256, "observation.evidence_sha256")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceRun {
    pub format_version: u16,
    pub id: CanonicalId,
    pub instance_id: String,
    pub revision: u64,
    pub stage: MaintenanceStage,
    pub status: MaintenanceStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub actor: String,
    pub target_reduction_bps: u16,
    pub inventory: MaintenanceInventory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection: Option<ProtectionEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<ProjectionPlan>,
    #[serde(default)]
    pub validation: Vec<ValidationGate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<MaintenanceObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_state_sha256: Option<String>,
    pub state_sha256: String,
}

impl MaintenanceRun {
    pub fn start(
        id: CanonicalId,
        instance_id: impl Into<String>,
        actor: impl Into<String>,
        at: u64,
        target_reduction_bps: u16,
        inventory: MaintenanceInventory,
    ) -> Result<Self> {
        let mut run = Self {
            format_version: MAINTENANCE_FORMAT_VERSION,
            id,
            instance_id: instance_id.into(),
            revision: 1,
            stage: MaintenanceStage::Inventory,
            status: MaintenanceStatus::Active,
            created_at: at,
            updated_at: at,
            actor: actor.into(),
            target_reduction_bps,
            inventory,
            protection: None,
            proposal: None,
            validation: pending_gates(),
            applied_generation: None,
            observation: None,
            previous_state_sha256: None,
            state_sha256: String::new(),
        };
        run.refresh_digest()?;
        run.validate()?;
        Ok(run)
    }

    pub fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }

    pub fn all_gates_pass(&self) -> bool {
        exact_gates(self.validation.iter().map(|gate| gate.kind)).is_ok()
            && self
                .validation
                .iter()
                .all(|gate| gate.status == ValidationGateStatus::Passed)
    }

    pub fn validate(&self) -> Result<()> {
        if self.format_version != MAINTENANCE_FORMAT_VERSION
            || self.revision == 0
            || self.created_at == 0
            || self.updated_at < self.created_at
        {
            return Err(contract(
                "maintenance run version, revision, or time is invalid",
            ));
        }
        text_field("instance_id", &self.instance_id, false)?;
        text_field("actor", &self.actor, false)?;
        target(self.target_reduction_bps)?;
        self.inventory.validate()?;
        exact_gates(self.validation.iter().map(|gate| gate.kind))?;
        for gate in &self.validation {
            gate.validate()?;
        }
        if let Some(protection) = &self.protection {
            protection.validate(&self.inventory)?;
        }
        if let Some(proposal) = &self.proposal {
            proposal.validate(&self.inventory)?;
        }
        if let Some(observation) = &self.observation {
            observation.validate()?;
        }
        if self.revision == 1 && self.previous_state_sha256.is_some()
            || self.revision > 1 && self.previous_state_sha256.is_none()
        {
            return Err(contract("maintenance state predecessor is inconsistent"));
        }
        if let Some(previous) = &self.previous_state_sha256 {
            validate_sha256(previous, "previous_state_sha256")?;
        }
        validate_sha256(&self.state_sha256, "state_sha256")?;
        if state_digest(self)? != self.state_sha256 {
            return Err(contract("maintenance state digest does not verify"));
        }
        validate_stage_shape(self)
    }

    fn next(
        &self,
        stage: MaintenanceStage,
        status: MaintenanceStatus,
        actor: &str,
        at: u64,
    ) -> Result<Self> {
        if self.is_terminal() || at <= self.updated_at {
            return Err(contract(
                "terminal runs cannot advance and transition time must increase",
            ));
        }
        let mut next = self.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| contract("maintenance revision overflow"))?;
        next.stage = stage;
        next.status = status;
        next.updated_at = at;
        next.actor = actor.into();
        next.previous_state_sha256 = Some(self.state_sha256.clone());
        next.state_sha256.clear();
        Ok(next)
    }

    fn refresh_digest(&mut self) -> Result<()> {
        self.state_sha256 = state_digest(self)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionActivationKind {
    Apply,
    Rollback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveMaintenanceProjection {
    pub format_version: u16,
    pub generation: u64,
    pub activation: ProjectionActivationKind,
    pub source_run_id: CanonicalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<ProjectionPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_plan: Option<ProjectionPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_projection_sha256: Option<String>,
    pub activated_at: u64,
    pub actor: String,
    pub state_sha256: String,
}

impl ActiveMaintenanceProjection {
    pub fn injected_token_ceiling(&self) -> Option<usize> {
        self.plan.as_ref().map(|plan| {
            usize::try_from(plan.hot_tokens)
                .unwrap_or(usize::MAX)
                .clamp(128, 32_000)
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.format_version != MAINTENANCE_FORMAT_VERSION
            || self.generation == 0
            || self.activated_at == 0
        {
            return Err(contract(
                "active projection version, generation, or time is invalid",
            ));
        }
        text_field("projection.actor", &self.actor, false)?;
        if let Some(previous) = &self.previous_projection_sha256 {
            validate_sha256(previous, "previous_projection_sha256")?;
        }
        if self.activation == ProjectionActivationKind::Apply && self.plan.is_none() {
            return Err(contract("apply activation requires a projection plan"));
        }
        validate_sha256(&self.state_sha256, "projection.state_sha256")?;
        if active_projection_digest(self)? != self.state_sha256 {
            return Err(contract("active projection digest does not verify"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceEvent {
    pub run_id: CanonicalId,
    pub revision: u64,
    pub stage: MaintenanceStage,
    pub status: MaintenanceStatus,
    pub at: u64,
    pub actor: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_state_sha256: Option<String>,
    pub state_sha256: String,
}

pub struct MaintenanceRepository<'a, E: Engine + ?Sized> {
    engine: &'a E,
    instance_id: String,
    scope: ScopeId,
}

impl<'a, E: Engine + ?Sized> MaintenanceRepository<'a, E> {
    pub fn new(engine: &'a E, instance_id: impl Into<String>) -> Result<Self> {
        let instance_id = instance_id.into();
        text_field("instance_id", &instance_id, false)?;
        let scope = ScopeId::new(format!("maintenance:{instance_id}"))
            .map_err(|error| contract(error.to_string()))?;
        Ok(Self {
            engine,
            instance_id,
            scope,
        })
    }

    pub fn scope(&self) -> &ScopeId {
        &self.scope
    }

    pub fn runs(&self) -> Result<Vec<MaintenanceRun>> {
        Ok(self.load()?.runs.into_values().collect())
    }

    pub fn active_run(&self) -> Result<Option<MaintenanceRun>> {
        Ok(self.runs()?.into_iter().find(|run| !run.is_terminal()))
    }

    pub fn run(&self, id: &CanonicalId) -> Result<Option<MaintenanceRun>> {
        Ok(self.load()?.runs.remove(id.as_str()))
    }

    pub fn active_projection(&self) -> Result<Option<ActiveMaintenanceProjection>> {
        Ok(self.load()?.active_projection)
    }

    pub fn start(
        &self,
        id: CanonicalId,
        actor: &str,
        at: u64,
        target_reduction_bps: u16,
        inventory: MaintenanceInventory,
    ) -> Result<MaintenanceRun> {
        let loaded = self.load()?;
        if loaded.runs.values().any(|run| !run.is_terminal()) {
            return Err(contract("an active maintenance run already exists"));
        }
        if loaded.runs.contains_key(id.as_str()) {
            return Err(contract("maintenance run identity already exists"));
        }
        let run = MaintenanceRun::start(
            id,
            self.instance_id.clone(),
            actor,
            at,
            target_reduction_bps,
            inventory,
        )?;
        self.commit(
            None,
            &run,
            "maintenance.inventory",
            None,
            loaded.head_cursor,
        )?;
        Ok(run)
    }

    pub fn protect(
        &self,
        id: &CanonicalId,
        expected_revision: u64,
        actor: &str,
        at: u64,
        evidence: ProtectionEvidence,
    ) -> Result<MaintenanceRun> {
        self.advance(
            id,
            expected_revision,
            actor,
            at,
            "maintenance.protect",
            |current| {
                if current.stage != MaintenanceStage::Inventory {
                    return Err(contract("protect requires the inventory stage"));
                }
                evidence.validate(&current.inventory)?;
                let mut next = current.next(
                    MaintenanceStage::Protect,
                    MaintenanceStatus::Active,
                    actor,
                    at,
                )?;
                next.protection = Some(evidence);
                Ok((next, None))
            },
        )
    }

    pub fn propose(
        &self,
        id: &CanonicalId,
        expected_revision: u64,
        actor: &str,
        at: u64,
        decisions: Vec<MaintenanceDecision>,
    ) -> Result<MaintenanceRun> {
        self.advance(
            id,
            expected_revision,
            actor,
            at,
            "maintenance.propose",
            |current| {
                if current.stage != MaintenanceStage::Protect || current.protection.is_none() {
                    return Err(contract("proposal requires verified protection"));
                }
                let proposal = ProjectionPlan::build(
                    &current.inventory,
                    current.target_reduction_bps,
                    decisions,
                )?;
                let mut next = current.next(
                    MaintenanceStage::Propose,
                    MaintenanceStatus::Active,
                    actor,
                    at,
                )?;
                next.proposal = Some(proposal);
                Ok((next, None))
            },
        )
    }

    pub fn review(
        &self,
        id: &CanonicalId,
        expected_revision: u64,
        actor: &str,
        at: u64,
        decisions: Vec<MaintenanceDecision>,
    ) -> Result<MaintenanceRun> {
        self.advance(
            id,
            expected_revision,
            actor,
            at,
            "maintenance.review",
            |current| {
                if current.stage != MaintenanceStage::Propose {
                    return Err(contract("review requires the proposal stage"));
                }
                let proposal = ProjectionPlan::build(
                    &current.inventory,
                    current.target_reduction_bps,
                    decisions,
                )?;
                let mut next = current.next(
                    MaintenanceStage::Review,
                    MaintenanceStatus::Active,
                    actor,
                    at,
                )?;
                next.proposal = Some(proposal);
                next.validation = pending_gates();
                Ok((next, None))
            },
        )
    }

    pub fn validate_evidence(
        &self,
        id: &CanonicalId,
        expected_revision: u64,
        actor: &str,
        at: u64,
        gates: Vec<ValidationGate>,
    ) -> Result<MaintenanceRun> {
        self.advance(
            id,
            expected_revision,
            actor,
            at,
            "maintenance.validate",
            |current| {
                if !matches!(
                    current.stage,
                    MaintenanceStage::Review | MaintenanceStage::Validate
                ) {
                    return Err(contract(
                        "validation requires review or an earlier validation attempt",
                    ));
                }
                exact_gates(gates.iter().map(|gate| gate.kind))?;
                for gate in &gates {
                    gate.validate()?;
                }
                let ready = gates
                    .iter()
                    .all(|gate| gate.status == ValidationGateStatus::Passed);
                let mut next = current.next(
                    MaintenanceStage::Validate,
                    if ready {
                        MaintenanceStatus::ReadyToApply
                    } else {
                        MaintenanceStatus::Active
                    },
                    actor,
                    at,
                )?;
                next.validation = gates;
                Ok((next, None))
            },
        )
    }

    pub fn apply(
        &self,
        id: &CanonicalId,
        expected_revision: u64,
        actor: &str,
        at: u64,
    ) -> Result<MaintenanceRun> {
        self.advance(
            id,
            expected_revision,
            actor,
            at,
            "maintenance.apply",
            |current| {
                if current.stage != MaintenanceStage::Validate
                    || current.status != MaintenanceStatus::ReadyToApply
                    || !current.all_gates_pass()
                {
                    return Err(contract("apply requires every validation gate to pass"));
                }
                let loaded = self.load()?;
                let previous = loaded.active_projection;
                let generation = previous
                    .as_ref()
                    .map_or(1, |projection| projection.generation.saturating_add(1));
                if generation == u64::MAX {
                    return Err(contract("maintenance projection generation overflow"));
                }
                let mut projection = ActiveMaintenanceProjection {
                    format_version: MAINTENANCE_FORMAT_VERSION,
                    generation,
                    activation: ProjectionActivationKind::Apply,
                    source_run_id: current.id.clone(),
                    plan: current.proposal.clone(),
                    previous_plan: previous
                        .as_ref()
                        .and_then(|projection| projection.plan.clone()),
                    previous_projection_sha256: previous.map(|projection| projection.state_sha256),
                    activated_at: at,
                    actor: actor.into(),
                    state_sha256: String::new(),
                };
                projection.state_sha256 = active_projection_digest(&projection)?;
                projection.validate()?;
                let mut next = current.next(
                    MaintenanceStage::Apply,
                    MaintenanceStatus::Applied,
                    actor,
                    at,
                )?;
                next.applied_generation = Some(generation);
                Ok((next, Some(projection)))
            },
        )
    }

    pub fn observe(
        &self,
        id: &CanonicalId,
        expected_revision: u64,
        actor: &str,
        at: u64,
        observation: MaintenanceObservation,
    ) -> Result<MaintenanceRun> {
        self.advance(
            id,
            expected_revision,
            actor,
            at,
            "maintenance.observe",
            |current| {
                if current.stage != MaintenanceStage::Apply
                    || current.status != MaintenanceStatus::Applied
                {
                    return Err(contract("observation requires an applied projection"));
                }
                observation.validate()?;
                let mut next = current.next(
                    MaintenanceStage::Observe,
                    if observation.decision == ObservationDecision::Accept {
                        MaintenanceStatus::Completed
                    } else {
                        MaintenanceStatus::RolledBack
                    },
                    actor,
                    at,
                )?;
                next.observation = Some(observation.clone());
                let projection = if observation.decision == ObservationDecision::RollBack {
                    let loaded = self.load()?;
                    let active = loaded
                        .active_projection
                        .ok_or_else(|| contract("rollback requires an active projection"))?;
                    if active.source_run_id != current.id
                        || Some(active.generation) != current.applied_generation
                    {
                        return Err(contract("active projection no longer belongs to this run"));
                    }
                    let mut rollback = ActiveMaintenanceProjection {
                        format_version: MAINTENANCE_FORMAT_VERSION,
                        generation: active.generation.checked_add(1).ok_or_else(|| {
                            contract("maintenance projection generation overflow")
                        })?,
                        activation: ProjectionActivationKind::Rollback,
                        source_run_id: current.id.clone(),
                        plan: active.previous_plan.clone(),
                        previous_plan: active.plan.clone(),
                        previous_projection_sha256: Some(active.state_sha256),
                        activated_at: at,
                        actor: actor.into(),
                        state_sha256: String::new(),
                    };
                    rollback.state_sha256 = active_projection_digest(&rollback)?;
                    rollback.validate()?;
                    Some(rollback)
                } else {
                    None
                };
                Ok((next, projection))
            },
        )
    }

    pub fn cancel(
        &self,
        id: &CanonicalId,
        expected_revision: u64,
        actor: &str,
        at: u64,
    ) -> Result<MaintenanceRun> {
        self.advance(
            id,
            expected_revision,
            actor,
            at,
            "maintenance.cancel",
            |current| {
                if current.stage == MaintenanceStage::Apply {
                    return Err(contract("an applied run must be observed or rolled back"));
                }
                let next = current.next(current.stage, MaintenanceStatus::Cancelled, actor, at)?;
                Ok((next, None))
            },
        )
    }

    fn advance<F>(
        &self,
        id: &CanonicalId,
        expected_revision: u64,
        _actor: &str,
        _at: u64,
        action: &str,
        transition: F,
    ) -> Result<MaintenanceRun>
    where
        F: FnOnce(&MaintenanceRun) -> Result<(MaintenanceRun, Option<ActiveMaintenanceProjection>)>,
    {
        let loaded = self.load()?;
        let current = loaded
            .runs
            .get(id.as_str())
            .ok_or_else(|| contract("maintenance run does not exist"))?;
        if current.revision != expected_revision {
            return Err(contract("maintenance run revision conflict"));
        }
        let (mut next, projection) = transition(current)?;
        next.refresh_digest()?;
        next.validate()?;
        self.commit(
            Some(current),
            &next,
            action,
            projection.as_ref(),
            loaded.head_cursor,
        )?;
        Ok(next)
    }

    fn commit(
        &self,
        previous: Option<&MaintenanceRun>,
        next: &MaintenanceRun,
        action: &str,
        projection: Option<&ActiveMaintenanceProjection>,
        expected_cursor: u64,
    ) -> Result<()> {
        let mut mutations = Vec::new();
        if self.engine.runtime_schema(&self.scope)?.is_none() {
            mutations.push(RuntimeMutation::Schema {
                registry: maintenance_schema()?,
            });
        }
        mutations.push(run_record(next)?);
        mutations.push(event_mutation(&MaintenanceEvent {
            run_id: next.id.clone(),
            revision: next.revision,
            stage: next.stage,
            status: next.status,
            at: next.updated_at,
            actor: next.actor.clone(),
            action: action.into(),
            previous_state_sha256: previous.map(|run| run.state_sha256.clone()),
            state_sha256: next.state_sha256.clone(),
        })?);
        if let Some(projection) = projection {
            mutations.push(projection_record(projection)?);
        }
        self.engine.commit_runtime(&RuntimeCommit {
            scope: self.scope.clone(),
            at: next.updated_at,
            actor: next.actor.clone(),
            expected_cursor,
            mutations,
        })?;
        Ok(())
    }

    fn load(&self) -> Result<LoadedMaintenance> {
        let head_cursor = self.engine.runtime_cursor()?;
        let mut after = 0_u64;
        let mut revisions = BTreeMap::<String, Vec<MaintenanceRun>>::new();
        let mut events = BTreeMap::<(String, u64), MaintenanceEvent>::new();
        let mut projections = Vec::<ActiveMaintenanceProjection>::new();
        while after < head_cursor {
            let page = self
                .engine
                .runtime_changes_since(after, REPLAY_PAGE, Some(&self.scope))?;
            if page.head_cursor != head_cursor {
                return Err(contract("maintenance source cursor changed during replay"));
            }
            let through_cursor = page.through_cursor;
            let has_more = page.has_more();
            for change in page.changes {
                match change.mutation {
                    RuntimeMutation::Record { record }
                        if record.reference.kind.as_str() == RUN_TYPE =>
                    {
                        let run = decode_run(&record)?;
                        revisions
                            .entry(run.id.as_str().into())
                            .or_default()
                            .push(run);
                    }
                    RuntimeMutation::Record { record }
                        if record.reference.kind.as_str() == PROJECTION_TYPE =>
                    {
                        projections.push(decode_projection(&record)?);
                    }
                    RuntimeMutation::Event { event } if event.kind.as_str() == EVENT_TYPE => {
                        let decoded = decode_event(&event)?;
                        let key = (decoded.run_id.as_str().into(), decoded.revision);
                        if events.insert(key, decoded).is_some() {
                            return Err(contract("duplicate maintenance event revision"));
                        }
                    }
                    _ => {}
                }
            }
            if through_cursor <= after || !has_more {
                after = through_cursor;
                break;
            }
            after = through_cursor;
        }
        if after < head_cursor && !revisions.is_empty() {
            return Err(contract(
                "maintenance replay ended before the captured head",
            ));
        }
        let mut runs = BTreeMap::new();
        for (id, mut history) in revisions {
            history.sort_by_key(|run| run.revision);
            let latest = verify_history(&history, &events)?;
            runs.insert(id, latest.clone());
        }
        projections.sort_by_key(|projection| projection.generation);
        let mut previous_generation = 0_u64;
        let mut previous_digest: Option<&str> = None;
        for projection in &projections {
            projection.validate()?;
            if projection.generation != previous_generation + 1
                || projection.previous_projection_sha256.as_deref() != previous_digest
            {
                return Err(contract(
                    "active projection generation or digest chain is broken",
                ));
            }
            previous_generation = projection.generation;
            previous_digest = Some(&projection.state_sha256);
        }
        Ok(LoadedMaintenance {
            head_cursor,
            runs,
            active_projection: projections.pop(),
        })
    }
}

struct LoadedMaintenance {
    head_cursor: u64,
    runs: BTreeMap<String, MaintenanceRun>,
    active_projection: Option<ActiveMaintenanceProjection>,
}

fn verify_history<'a>(
    history: &'a [MaintenanceRun],
    events: &BTreeMap<(String, u64), MaintenanceEvent>,
) -> Result<&'a MaintenanceRun> {
    let mut previous: Option<&MaintenanceRun> = None;
    for run in history {
        run.validate()?;
        if run.revision != previous.map_or(1, |value| value.revision + 1)
            || run.previous_state_sha256.as_deref()
                != previous.map(|value| value.state_sha256.as_str())
        {
            return Err(contract(
                "maintenance revision or state digest chain is broken",
            ));
        }
        let event = events
            .get(&(run.id.as_str().into(), run.revision))
            .ok_or_else(|| contract("maintenance revision has no matching event"))?;
        if event.state_sha256 != run.state_sha256
            || event.previous_state_sha256 != run.previous_state_sha256
            || event.stage != run.stage
            || event.status != run.status
            || event.at != run.updated_at
            || event.actor != run.actor
        {
            return Err(contract(
                "maintenance event disagrees with its state revision",
            ));
        }
        previous = Some(run);
    }
    previous.ok_or_else(|| contract("maintenance history is empty"))
}

fn validate_stage_shape(run: &MaintenanceRun) -> Result<()> {
    if run.stage >= MaintenanceStage::Protect && run.protection.is_none()
        || run.stage >= MaintenanceStage::Propose && run.proposal.is_none()
        || run.stage >= MaintenanceStage::Apply && run.applied_generation.is_none()
        || run.stage == MaintenanceStage::Observe && run.observation.is_none()
    {
        return Err(contract(
            "maintenance stage is missing its required evidence",
        ));
    }
    if run.status == MaintenanceStatus::ReadyToApply && !run.all_gates_pass()
        || run.status == MaintenanceStatus::Applied && run.stage != MaintenanceStage::Apply
        || matches!(
            run.status,
            MaintenanceStatus::Completed | MaintenanceStatus::RolledBack
        ) && run.stage != MaintenanceStage::Observe
    {
        return Err(contract(
            "maintenance status is inconsistent with its stage",
        ));
    }
    Ok(())
}

fn validate_decisions(decisions: &[MaintenanceDecision]) -> Result<()> {
    exact_classes(decisions.iter().map(|decision| decision.class))?;
    for decision in decisions {
        decision.validate()?;
    }
    Ok(())
}

fn exact_classes(classes: impl Iterator<Item = HistoryClass>) -> Result<()> {
    let actual = classes.collect::<BTreeSet<_>>();
    let expected = HistoryClass::ALL.into_iter().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(contract(
            "maintenance history classes must be complete and unique",
        ));
    }
    Ok(())
}

fn exact_gates(gates: impl Iterator<Item = ValidationGateKind>) -> Result<()> {
    let actual = gates.collect::<BTreeSet<_>>();
    let expected = ValidationGateKind::ALL.into_iter().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(contract(
            "maintenance validation gates must be complete and unique",
        ));
    }
    Ok(())
}

pub fn default_decisions() -> Vec<MaintenanceDecision> {
    HistoryClass::ALL
        .into_iter()
        .map(|class| MaintenanceDecision {
            class,
            proposed: class.default_disposition(),
            selected: class.default_disposition(),
            rationale: match class.default_disposition() {
                ContextDisposition::Hot => "required on every model turn",
                ContextDisposition::Warm => "retain for governed recall",
                ContextDisposition::Cold => "retain in authenticated archive and omit by default",
            }
            .into(),
            operator_note: None,
        })
        .collect()
}

pub fn pending_gates() -> Vec<ValidationGate> {
    ValidationGateKind::ALL
        .into_iter()
        .map(|kind| ValidationGate {
            kind,
            status: ValidationGateStatus::Pending,
            summary: "evidence has not been evaluated".into(),
            evidence_sha256: None,
        })
        .collect()
}

fn maintenance_schema() -> Result<RuntimeSchemaRegistry> {
    let mut registry = RuntimeSchemaRegistry::empty(1, "install context maintenance v1");
    registry.records.insert(
        RuntimeType::new(RUN_TYPE).map_err(|error| contract(error.to_string()))?,
        RuntimeRecordSchema {
            properties: BTreeMap::from([
                ("run_json".into(), required(RuntimeValueType::String)),
                ("revision".into(), required(RuntimeValueType::Unsigned)),
                ("stage".into(), required(RuntimeValueType::String)),
                ("status".into(), required(RuntimeValueType::String)),
                ("state_sha256".into(), required(RuntimeValueType::Digest)),
            ]),
            ..RuntimeRecordSchema::default()
        },
    );
    registry.records.insert(
        RuntimeType::new(PROJECTION_TYPE).map_err(|error| contract(error.to_string()))?,
        RuntimeRecordSchema {
            properties: BTreeMap::from([
                ("projection_json".into(), required(RuntimeValueType::String)),
                ("generation".into(), required(RuntimeValueType::Unsigned)),
                ("state_sha256".into(), required(RuntimeValueType::Digest)),
            ]),
            ..RuntimeRecordSchema::default()
        },
    );
    registry.events.insert(
        RuntimeType::new(EVENT_TYPE).map_err(|error| contract(error.to_string()))?,
        RuntimeEventSchema {
            subject_required: true,
            subject_types: BTreeSet::from([
                RuntimeType::new(RUN_TYPE).map_err(|error| contract(error.to_string()))?
            ]),
            properties: BTreeMap::from([
                ("event_json".into(), required(RuntimeValueType::String)),
                ("revision".into(), required(RuntimeValueType::Unsigned)),
                ("stage".into(), required(RuntimeValueType::String)),
                ("state_sha256".into(), required(RuntimeValueType::Digest)),
            ]),
            ..RuntimeEventSchema::default()
        },
    );
    registry
        .validate()
        .map_err(|error| contract(error.to_string()))?;
    Ok(registry)
}

fn run_record(run: &MaintenanceRun) -> Result<RuntimeMutation> {
    Ok(RuntimeMutation::Record {
        record: RuntimeRecord {
            reference: RuntimeRef::new(RUN_TYPE, run.id.as_str())
                .map_err(|error| contract(error.to_string()))?,
            valid_from: run.updated_at,
            valid_to: None,
            properties: RuntimeProperties::from([
                (
                    "run_json".into(),
                    RuntimeValue::String(serde_json::to_string(run)?),
                ),
                ("revision".into(), RuntimeValue::Unsigned(run.revision)),
                ("stage".into(), RuntimeValue::String(enum_name(&run.stage)?)),
                (
                    "status".into(),
                    RuntimeValue::String(enum_name(&run.status)?),
                ),
                (
                    "state_sha256".into(),
                    RuntimeValue::Digest(run.state_sha256.clone()),
                ),
            ]),
        },
    })
}

fn projection_record(projection: &ActiveMaintenanceProjection) -> Result<RuntimeMutation> {
    Ok(RuntimeMutation::Record {
        record: RuntimeRecord {
            reference: RuntimeRef::new(PROJECTION_TYPE, ACTIVE_PROJECTION_ID)
                .map_err(|error| contract(error.to_string()))?,
            valid_from: projection.activated_at,
            valid_to: None,
            properties: RuntimeProperties::from([
                (
                    "projection_json".into(),
                    RuntimeValue::String(serde_json::to_string(projection)?),
                ),
                (
                    "generation".into(),
                    RuntimeValue::Unsigned(projection.generation),
                ),
                (
                    "state_sha256".into(),
                    RuntimeValue::Digest(projection.state_sha256.clone()),
                ),
            ]),
        },
    })
}

fn event_mutation(event: &MaintenanceEvent) -> Result<RuntimeMutation> {
    Ok(RuntimeMutation::Event {
        event: RuntimeEvent {
            kind: RuntimeType::new(EVENT_TYPE).map_err(|error| contract(error.to_string()))?,
            subject: Some(
                RuntimeRef::new(RUN_TYPE, event.run_id.as_str())
                    .map_err(|error| contract(error.to_string()))?,
            ),
            properties: RuntimeProperties::from([
                (
                    "event_json".into(),
                    RuntimeValue::String(serde_json::to_string(event)?),
                ),
                ("revision".into(), RuntimeValue::Unsigned(event.revision)),
                (
                    "stage".into(),
                    RuntimeValue::String(enum_name(&event.stage)?),
                ),
                (
                    "state_sha256".into(),
                    RuntimeValue::Digest(event.state_sha256.clone()),
                ),
            ]),
        },
    })
}

fn decode_run(record: &RuntimeRecord) -> Result<MaintenanceRun> {
    let RuntimeValue::String(encoded) = property(record, "run_json")? else {
        return Err(contract("maintenance run JSON property has the wrong type"));
    };
    let run: MaintenanceRun = serde_json::from_str(encoded)?;
    if record.reference.id.as_str() != run.id.as_str()
        || property(record, "revision")? != &RuntimeValue::Unsigned(run.revision)
        || property(record, "state_sha256")? != &RuntimeValue::Digest(run.state_sha256.clone())
    {
        return Err(contract(
            "maintenance run record disagrees with its encoded state",
        ));
    }
    Ok(run)
}

fn decode_projection(record: &RuntimeRecord) -> Result<ActiveMaintenanceProjection> {
    if record.reference.id.as_str() != ACTIVE_PROJECTION_ID {
        return Err(contract(
            "maintenance projection record identity is not active",
        ));
    }
    let RuntimeValue::String(encoded) = property(record, "projection_json")? else {
        return Err(contract(
            "maintenance projection JSON property has the wrong type",
        ));
    };
    let projection: ActiveMaintenanceProjection = serde_json::from_str(encoded)?;
    if property(record, "generation")? != &RuntimeValue::Unsigned(projection.generation)
        || property(record, "state_sha256")?
            != &RuntimeValue::Digest(projection.state_sha256.clone())
    {
        return Err(contract(
            "maintenance projection record disagrees with its encoded state",
        ));
    }
    Ok(projection)
}

fn decode_event(event: &RuntimeEvent) -> Result<MaintenanceEvent> {
    let RuntimeValue::String(encoded) = event
        .properties
        .get("event_json")
        .ok_or_else(|| contract("maintenance event has no event_json"))?
    else {
        return Err(contract(
            "maintenance event JSON property has the wrong type",
        ));
    };
    let decoded: MaintenanceEvent = serde_json::from_str(encoded)?;
    if event.subject.as_ref().map(|value| value.id.as_str()) != Some(decoded.run_id.as_str()) {
        return Err(contract("maintenance event subject disagrees with its run"));
    }
    Ok(decoded)
}

fn property<'a>(record: &'a RuntimeRecord, name: &str) -> Result<&'a RuntimeValue> {
    record
        .properties
        .get(name)
        .ok_or_else(|| contract(format!("maintenance record has no {name}")))
}

fn required(value_type: RuntimeValueType) -> RuntimePropertySchema {
    RuntimePropertySchema::required(value_type)
}

fn state_digest(run: &MaintenanceRun) -> Result<String> {
    let mut canonical = run.clone();
    canonical.state_sha256.clear();
    Ok(digest::sha256_hex(&serde_json::to_vec(&canonical)?))
}

fn plan_digest(plan: &ProjectionPlan) -> Result<String> {
    let mut canonical = plan.clone();
    canonical.projection_sha256.clear();
    Ok(digest::sha256_hex(&serde_json::to_vec(&canonical)?))
}

fn active_projection_digest(projection: &ActiveMaintenanceProjection) -> Result<String> {
    let mut canonical = projection.clone();
    canonical.state_sha256.clear();
    Ok(digest::sha256_hex(&serde_json::to_vec(&canonical)?))
}

fn enum_name(value: &impl Serialize) -> Result<String> {
    let encoded = serde_json::to_string(value)?;
    Ok(encoded.trim_matches('"').into())
}

fn target(value: u16) -> Result<()> {
    if !(MIN_TARGET_REDUCTION_BPS..=MAX_TARGET_REDUCTION_BPS).contains(&value) {
        return Err(contract("target reduction must be 50%..=75%"));
    }
    Ok(())
}

fn text_field(name: &str, value: &str, allow_empty: bool) -> Result<()> {
    if (!allow_empty && value.trim().is_empty())
        || value.len() > MAX_TEXT_BYTES
        || value.as_bytes().contains(&0)
    {
        return Err(contract(format!(
            "{name} is empty, oversized, or contains NUL"
        )));
    }
    Ok(())
}

fn validate_sha256(value: &str, name: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(contract(format!("{name} must be a lowercase SHA-256")));
    }
    Ok(())
}

fn contract(message: impl Into<String>) -> MaintenanceError {
    MaintenanceError::Contract(message.into())
}
