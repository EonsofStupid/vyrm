use super::event::LifecycleEventTypeV1;
use crate::{invalid, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleTaskKindV1 {
    CodeChange,
    Investigation,
    Query,
    Operation,
    Maintenance,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleRiskV1 {
    ReadOnly,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleTurnStatusV1 {
    Completed,
    Denied,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LifecyclePayloadV1 {
    SessionOpened {
        resumed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_session_sha256: Option<String>,
    },
    AttunementRequested {
        source_fingerprint_sha256: String,
    },
    AttunementCompleted {
        topology_sha256: String,
        profile_sha256: String,
        source_tree_sha256: String,
    },
    Failure {
        reason_code: String,
        evidence_sha256: String,
        retryable: bool,
    },
    TurnOpened,
    PromptReceived {
        prompt_sha256: String,
        prompt_bytes: u64,
    },
    PreflightStarted {
        source_tree_sha256: String,
    },
    PreflightCompleted {
        receipt_sha256: String,
        context_sha256: String,
    },
    Decision {
        decision_code: String,
        evidence_sha256: String,
    },
    TaskClassified {
        task: LifecycleTaskKindV1,
        risk: LifecycleRiskV1,
        classification_sha256: String,
    },
    ArchitectureRequested {
        questions_sha256: String,
    },
    ArchitectureCompleted {
        assessment_sha256: String,
        owner_boundary: String,
        dependency_direction: String,
    },
    PatternSelected {
        selection_sha256: String,
        pattern_count: u32,
    },
    PlanningAuthorized {
        permit_sha256: String,
        expires_at_unix_ms: u64,
        plan_id: String,
        plan_revision: u64,
        work_item_id: String,
    },
    ContextAssembled {
        context_sha256: String,
        context_bytes: u64,
        truncated: bool,
    },
    PlanRecorded {
        plan_sha256: String,
        plan_id: String,
        plan_revision: u64,
        work_item_id: String,
        source_tree_sha256: String,
        verification_plan_sha256: String,
    },
    ToolProposed {
        tool_name: String,
        tool_request_sha256: String,
        mutation: bool,
    },
    ToolDecision {
        decision_sha256: String,
        allowed: bool,
        reason_code: String,
    },
    ToolStarted {
        authorization_sha256: String,
    },
    ToolFinished {
        tool_request_sha256: String,
        observation_sha256: String,
        success: bool,
        project_state_changed: bool,
    },
    ProjectStateInvalidated {
        before_tree_sha256: String,
        after_tree_sha256: String,
        changed_paths_sha256: String,
    },
    ProjectionRefresh {
        projection_sha256: String,
        evidence_sha256: String,
        fresh: bool,
    },
    VerificationCompleted {
        verification_sha256: String,
        checks: u32,
        passed: bool,
    },
    OutcomeRecorded {
        outcome_sha256: String,
        success: bool,
    },
    TurnClosed {
        status: LifecycleTurnStatusV1,
    },
    Compaction {
        state_sha256: String,
    },
    SessionClosed {
        reason_code: String,
    },
}

impl LifecyclePayloadV1 {
    pub fn validate_for(&self, event_type: LifecycleEventTypeV1) -> Result<()> {
        let compatible = matches!(
            (event_type, self),
            (
                LifecycleEventTypeV1::SessionOpened,
                Self::SessionOpened { .. }
            ) | (
                LifecycleEventTypeV1::ProjectAttunementRequested,
                Self::AttunementRequested { .. }
            ) | (
                LifecycleEventTypeV1::ProjectAttunementCompleted,
                Self::AttunementCompleted { .. }
            ) | (
                LifecycleEventTypeV1::ProjectAttunementFailed,
                Self::Failure { .. }
            ) | (LifecycleEventTypeV1::TurnOpened, Self::TurnOpened)
                | (
                    LifecycleEventTypeV1::PromptReceived,
                    Self::PromptReceived { .. }
                )
                | (
                    LifecycleEventTypeV1::PreflightStarted,
                    Self::PreflightStarted { .. }
                )
                | (
                    LifecycleEventTypeV1::PreflightCompleted,
                    Self::PreflightCompleted { .. }
                )
                | (LifecycleEventTypeV1::PreflightDenied, Self::Decision { .. })
                | (
                    LifecycleEventTypeV1::TaskClassificationCompleted,
                    Self::TaskClassified { .. }
                )
                | (
                    LifecycleEventTypeV1::ArchitectureAssessmentRequested,
                    Self::ArchitectureRequested { .. }
                )
                | (
                    LifecycleEventTypeV1::ArchitectureAssessmentCompleted,
                    Self::ArchitectureCompleted { .. }
                )
                | (
                    LifecycleEventTypeV1::ArchitectureDecisionRequired,
                    Self::Decision { .. }
                )
                | (
                    LifecycleEventTypeV1::ArchitectureAssessmentDenied,
                    Self::Decision { .. }
                )
                | (
                    LifecycleEventTypeV1::PatternSelectionCompleted,
                    Self::PatternSelected { .. }
                )
                | (
                    LifecycleEventTypeV1::PlanningAuthorized,
                    Self::PlanningAuthorized { .. }
                )
                | (LifecycleEventTypeV1::PlanningDenied, Self::Decision { .. })
                | (
                    LifecycleEventTypeV1::ContextAssembled,
                    Self::ContextAssembled { .. }
                )
                | (
                    LifecycleEventTypeV1::PlanRecorded,
                    Self::PlanRecorded { .. }
                )
                | (
                    LifecycleEventTypeV1::ToolProposed,
                    Self::ToolProposed { .. }
                )
                | (
                    LifecycleEventTypeV1::ToolAuthorized,
                    Self::ToolDecision { .. }
                )
                | (LifecycleEventTypeV1::ToolDenied, Self::ToolDecision { .. })
                | (LifecycleEventTypeV1::ToolStarted, Self::ToolStarted { .. })
                | (
                    LifecycleEventTypeV1::ToolCompleted,
                    Self::ToolFinished { .. }
                )
                | (LifecycleEventTypeV1::ToolFailed, Self::ToolFinished { .. })
                | (
                    LifecycleEventTypeV1::ProjectStateInvalidated,
                    Self::ProjectStateInvalidated { .. }
                )
                | (
                    LifecycleEventTypeV1::ProjectionRefreshCompleted,
                    Self::ProjectionRefresh { .. }
                )
                | (
                    LifecycleEventTypeV1::ProjectionRefreshFailed,
                    Self::ProjectionRefresh { .. }
                )
                | (
                    LifecycleEventTypeV1::VerificationCompleted,
                    Self::VerificationCompleted { .. }
                )
                | (
                    LifecycleEventTypeV1::ReasoningOutcomeRecorded,
                    Self::OutcomeRecorded { .. }
                )
                | (LifecycleEventTypeV1::TurnClosed, Self::TurnClosed { .. })
                | (
                    LifecycleEventTypeV1::SessionCompactionStarted,
                    Self::Compaction { .. }
                )
                | (
                    LifecycleEventTypeV1::SessionCompactionCompleted,
                    Self::Compaction { .. }
                )
                | (
                    LifecycleEventTypeV1::SessionClosed,
                    Self::SessionClosed { .. }
                )
        );
        if !compatible {
            return invalid("lifecycle event type and payload kind do not match");
        }

        match self {
            Self::SessionOpened {
                provider_session_sha256,
                ..
            } => validate_optional_sha256("provider session digest", provider_session_sha256),
            Self::AttunementRequested {
                source_fingerprint_sha256,
            } => validate_sha256("source fingerprint", source_fingerprint_sha256),
            Self::AttunementCompleted {
                topology_sha256,
                profile_sha256,
                source_tree_sha256,
            } => {
                validate_sha256("topology", topology_sha256)?;
                validate_sha256("profile", profile_sha256)?;
                validate_sha256("source tree", source_tree_sha256)
            }
            Self::Failure {
                reason_code,
                evidence_sha256,
                ..
            }
            | Self::Decision {
                decision_code: reason_code,
                evidence_sha256,
            } => {
                validate_text("reason code", reason_code)?;
                validate_sha256("decision evidence", evidence_sha256)
            }
            Self::TurnOpened => Ok(()),
            Self::PromptReceived {
                prompt_sha256,
                prompt_bytes,
            } => {
                validate_sha256("prompt", prompt_sha256)?;
                if *prompt_bytes == 0 {
                    return invalid("prompt byte count must be non-zero");
                }
                Ok(())
            }
            Self::PreflightStarted { source_tree_sha256 } => {
                validate_sha256("preflight source tree", source_tree_sha256)
            }
            Self::PreflightCompleted {
                receipt_sha256,
                context_sha256,
            } => {
                validate_sha256("preflight receipt", receipt_sha256)?;
                validate_sha256("preflight context", context_sha256)
            }
            Self::TaskClassified {
                classification_sha256,
                ..
            } => validate_sha256("task classification", classification_sha256),
            Self::ArchitectureRequested { questions_sha256 } => {
                validate_sha256("architecture questions", questions_sha256)
            }
            Self::ArchitectureCompleted {
                assessment_sha256,
                owner_boundary,
                dependency_direction,
            } => {
                validate_sha256("architecture assessment", assessment_sha256)?;
                validate_text("owner boundary", owner_boundary)?;
                validate_text("dependency direction", dependency_direction)
            }
            Self::PatternSelected {
                selection_sha256,
                pattern_count,
            } => {
                validate_sha256("pattern selection", selection_sha256)?;
                if *pattern_count == 0 {
                    return invalid("pattern selection must contain at least one pattern");
                }
                Ok(())
            }
            Self::PlanningAuthorized {
                permit_sha256,
                expires_at_unix_ms,
                plan_id,
                plan_revision,
                work_item_id,
            } => {
                validate_sha256("planning permit", permit_sha256)?;
                if *expires_at_unix_ms == 0 {
                    return invalid("planning permit expiration must be non-zero");
                }
                validate_text("plan id", plan_id)?;
                if *plan_revision == 0 {
                    return invalid("planning permit plan revision must be non-zero");
                }
                validate_text("work item id", work_item_id)
            }
            Self::ContextAssembled {
                context_sha256,
                context_bytes,
                ..
            } => {
                validate_sha256("context", context_sha256)?;
                if *context_bytes == 0 {
                    return invalid("assembled context must not be empty");
                }
                Ok(())
            }
            Self::PlanRecorded {
                plan_sha256,
                plan_id,
                plan_revision,
                work_item_id,
                source_tree_sha256,
                verification_plan_sha256,
            } => {
                validate_sha256("plan", plan_sha256)?;
                validate_text("plan id", plan_id)?;
                if *plan_revision == 0 {
                    return invalid("plan revision must be non-zero");
                }
                validate_text("work item id", work_item_id)?;
                validate_sha256("plan source tree", source_tree_sha256)?;
                validate_sha256("verification plan", verification_plan_sha256)
            }
            Self::ToolProposed {
                tool_name,
                tool_request_sha256,
                ..
            } => {
                validate_text("tool name", tool_name)?;
                validate_sha256("tool request", tool_request_sha256)
            }
            Self::ToolDecision {
                decision_sha256,
                allowed,
                reason_code,
            } => {
                validate_sha256("tool decision", decision_sha256)?;
                validate_text("tool reason", reason_code)?;
                if event_type == LifecycleEventTypeV1::ToolAuthorized && !*allowed
                    || event_type == LifecycleEventTypeV1::ToolDenied && *allowed
                {
                    return invalid("tool decision boolean contradicts the event type");
                }
                Ok(())
            }
            Self::ToolStarted {
                authorization_sha256,
            } => validate_sha256("tool authorization", authorization_sha256),
            Self::ToolFinished {
                tool_request_sha256,
                observation_sha256,
                success,
                ..
            } => {
                validate_sha256("finished tool request", tool_request_sha256)?;
                validate_sha256("tool observation", observation_sha256)?;
                if event_type == LifecycleEventTypeV1::ToolCompleted && !*success
                    || event_type == LifecycleEventTypeV1::ToolFailed && *success
                {
                    return invalid("tool result boolean contradicts the event type");
                }
                Ok(())
            }
            Self::ProjectStateInvalidated {
                before_tree_sha256,
                after_tree_sha256,
                changed_paths_sha256,
            } => {
                validate_sha256("tree before mutation", before_tree_sha256)?;
                validate_sha256("tree after mutation", after_tree_sha256)?;
                validate_sha256("changed paths", changed_paths_sha256)
            }
            Self::ProjectionRefresh {
                projection_sha256,
                evidence_sha256,
                fresh,
            } => {
                validate_sha256("projection", projection_sha256)?;
                validate_sha256("projection evidence", evidence_sha256)?;
                if event_type == LifecycleEventTypeV1::ProjectionRefreshCompleted && !*fresh
                    || event_type == LifecycleEventTypeV1::ProjectionRefreshFailed && *fresh
                {
                    return invalid("projection freshness contradicts the event type");
                }
                Ok(())
            }
            Self::VerificationCompleted {
                verification_sha256,
                checks,
                ..
            } => {
                validate_sha256("verification", verification_sha256)?;
                if *checks == 0 {
                    return invalid("verification must contain at least one check");
                }
                Ok(())
            }
            Self::OutcomeRecorded { outcome_sha256, .. } => {
                validate_sha256("reasoning outcome", outcome_sha256)
            }
            Self::TurnClosed { .. } => Ok(()),
            Self::Compaction { state_sha256 } => validate_sha256("compaction state", state_sha256),
            Self::SessionClosed { reason_code } => validate_text("close reason", reason_code),
        }
    }

    pub fn project_state_changed(&self) -> Option<bool> {
        match self {
            Self::ToolFinished {
                project_state_changed,
                ..
            } => Some(*project_state_changed),
            _ => None,
        }
    }

    pub fn verification_passed(&self) -> Option<bool> {
        match self {
            Self::VerificationCompleted { passed, .. } => Some(*passed),
            _ => None,
        }
    }
}

fn validate_optional_sha256(name: &str, value: &Option<String>) -> Result<()> {
    match value {
        Some(value) => validate_sha256(name, value),
        None => Ok(()),
    }
}

fn validate_sha256(name: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return invalid(format!("lifecycle {name} must be lowercase SHA-256 hex"));
    }
    Ok(())
}

fn validate_text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4_096 || value.contains('\0') {
        return invalid(format!("invalid lifecycle {name}"));
    }
    Ok(())
}
