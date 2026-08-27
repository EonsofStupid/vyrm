use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const LIFECYCLE_SPEC_VERSION: u16 = 1;
pub const MAX_LIFECYCLE_EVENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum LifecycleEventTypeV1 {
    #[serde(rename = "session.opened")]
    SessionOpened,
    #[serde(rename = "project.attunement.requested")]
    ProjectAttunementRequested,
    #[serde(rename = "project.attunement.completed")]
    ProjectAttunementCompleted,
    #[serde(rename = "project.attunement.failed")]
    ProjectAttunementFailed,
    #[serde(rename = "turn.opened")]
    TurnOpened,
    #[serde(rename = "prompt.received")]
    PromptReceived,
    #[serde(rename = "preflight.started")]
    PreflightStarted,
    #[serde(rename = "preflight.completed")]
    PreflightCompleted,
    #[serde(rename = "preflight.denied")]
    PreflightDenied,
    #[serde(rename = "task.classification.completed")]
    TaskClassificationCompleted,
    #[serde(rename = "architecture.assessment.requested")]
    ArchitectureAssessmentRequested,
    #[serde(rename = "architecture.assessment.completed")]
    ArchitectureAssessmentCompleted,
    #[serde(rename = "architecture.decision.required")]
    ArchitectureDecisionRequired,
    #[serde(rename = "architecture.assessment.denied")]
    ArchitectureAssessmentDenied,
    #[serde(rename = "pattern.selection.completed")]
    PatternSelectionCompleted,
    #[serde(rename = "planning.authorized")]
    PlanningAuthorized,
    #[serde(rename = "planning.denied")]
    PlanningDenied,
    #[serde(rename = "context.assembled")]
    ContextAssembled,
    #[serde(rename = "plan.recorded")]
    PlanRecorded,
    #[serde(rename = "tool.proposed")]
    ToolProposed,
    #[serde(rename = "tool.authorized")]
    ToolAuthorized,
    #[serde(rename = "tool.denied")]
    ToolDenied,
    #[serde(rename = "tool.started")]
    ToolStarted,
    #[serde(rename = "tool.completed")]
    ToolCompleted,
    #[serde(rename = "tool.failed")]
    ToolFailed,
    #[serde(rename = "project.state.invalidated")]
    ProjectStateInvalidated,
    #[serde(rename = "projection.refresh.completed")]
    ProjectionRefreshCompleted,
    #[serde(rename = "projection.refresh.failed")]
    ProjectionRefreshFailed,
    #[serde(rename = "verification.completed")]
    VerificationCompleted,
    #[serde(rename = "reasoning.outcome.recorded")]
    ReasoningOutcomeRecorded,
    #[serde(rename = "turn.closed")]
    TurnClosed,
    #[serde(rename = "session.compaction.started")]
    SessionCompactionStarted,
    #[serde(rename = "session.compaction.completed")]
    SessionCompactionCompleted,
    #[serde(rename = "session.closed")]
    SessionClosed,
}

impl LifecycleEventTypeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionOpened => "session.opened",
            Self::ProjectAttunementRequested => "project.attunement.requested",
            Self::ProjectAttunementCompleted => "project.attunement.completed",
            Self::ProjectAttunementFailed => "project.attunement.failed",
            Self::TurnOpened => "turn.opened",
            Self::PromptReceived => "prompt.received",
            Self::PreflightStarted => "preflight.started",
            Self::PreflightCompleted => "preflight.completed",
            Self::PreflightDenied => "preflight.denied",
            Self::TaskClassificationCompleted => "task.classification.completed",
            Self::ArchitectureAssessmentRequested => "architecture.assessment.requested",
            Self::ArchitectureAssessmentCompleted => "architecture.assessment.completed",
            Self::ArchitectureDecisionRequired => "architecture.decision.required",
            Self::ArchitectureAssessmentDenied => "architecture.assessment.denied",
            Self::PatternSelectionCompleted => "pattern.selection.completed",
            Self::PlanningAuthorized => "planning.authorized",
            Self::PlanningDenied => "planning.denied",
            Self::ContextAssembled => "context.assembled",
            Self::PlanRecorded => "plan.recorded",
            Self::ToolProposed => "tool.proposed",
            Self::ToolAuthorized => "tool.authorized",
            Self::ToolDenied => "tool.denied",
            Self::ToolStarted => "tool.started",
            Self::ToolCompleted => "tool.completed",
            Self::ToolFailed => "tool.failed",
            Self::ProjectStateInvalidated => "project.state.invalidated",
            Self::ProjectionRefreshCompleted => "projection.refresh.completed",
            Self::ProjectionRefreshFailed => "projection.refresh.failed",
            Self::VerificationCompleted => "verification.completed",
            Self::ReasoningOutcomeRecorded => "reasoning.outcome.recorded",
            Self::TurnClosed => "turn.closed",
            Self::SessionCompactionStarted => "session.compaction.started",
            Self::SessionCompactionCompleted => "session.compaction.completed",
            Self::SessionClosed => "session.closed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleEnforcementLevelV1 {
    Intercepting,
    Orchestrated,
    Proxied,
    Cooperative,
    ObserveOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePhaseV1 {
    SessionOpened,
    AttunementPending,
    Attuned,
    AttunementFailed,
    TurnOpened,
    PromptReceived,
    PreflightPending,
    PreflightCompleted,
    PreflightDenied,
    TaskClassified,
    ArchitecturePending,
    ArchitectureCompleted,
    ArchitectureDecisionRequired,
    ArchitectureDenied,
    PatternSelected,
    PlanningAuthorized,
    PlanningDenied,
    ContextAssembled,
    PlanRecorded,
    ToolProposed,
    ToolAuthorized,
    ToolDenied,
    ToolStarted,
    ToolCompleted,
    ToolFailed,
    ProjectStateInvalidated,
    ProjectionReady,
    ProjectionFailed,
    VerificationCompleted,
    OutcomeRecorded,
    TurnClosed,
    CompactionPending,
    CompactionCompleted,
    SessionClosed,
}

impl LifecyclePhaseV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionOpened => "session_opened",
            Self::AttunementPending => "attunement_pending",
            Self::Attuned => "attuned",
            Self::AttunementFailed => "attunement_failed",
            Self::TurnOpened => "turn_opened",
            Self::PromptReceived => "prompt_received",
            Self::PreflightPending => "preflight_pending",
            Self::PreflightCompleted => "preflight_completed",
            Self::PreflightDenied => "preflight_denied",
            Self::TaskClassified => "task_classified",
            Self::ArchitecturePending => "architecture_pending",
            Self::ArchitectureCompleted => "architecture_completed",
            Self::ArchitectureDecisionRequired => "architecture_decision_required",
            Self::ArchitectureDenied => "architecture_denied",
            Self::PatternSelected => "pattern_selected",
            Self::PlanningAuthorized => "planning_authorized",
            Self::PlanningDenied => "planning_denied",
            Self::ContextAssembled => "context_assembled",
            Self::PlanRecorded => "plan_recorded",
            Self::ToolProposed => "tool_proposed",
            Self::ToolAuthorized => "tool_authorized",
            Self::ToolDenied => "tool_denied",
            Self::ToolStarted => "tool_started",
            Self::ToolCompleted => "tool_completed",
            Self::ToolFailed => "tool_failed",
            Self::ProjectStateInvalidated => "project_state_invalidated",
            Self::ProjectionReady => "projection_ready",
            Self::ProjectionFailed => "projection_failed",
            Self::VerificationCompleted => "verification_completed",
            Self::OutcomeRecorded => "outcome_recorded",
            Self::TurnClosed => "turn_closed",
            Self::CompactionPending => "compaction_pending",
            Self::CompactionCompleted => "compaction_completed",
            Self::SessionClosed => "session_closed",
        }
    }
}
