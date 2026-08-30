use rrd_contract::{
    LifecycleEventEnvelopeV1, LifecycleEventTypeV1, LifecyclePayloadV1, LifecyclePhaseV1,
    LifecycleTurnStatusV1,
};
use rrd_core::digest;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct ReplayState {
    pub project_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub phase: Option<LifecyclePhaseV1>,
    pub instance_id: Option<String>,
    pub member_id: Option<String>,
    pub scope: Option<String>,
    pub active_turn_id: Option<String>,
    pub active_reasoning_run_id: Option<String>,
    pub planning_binding: Option<PlanningBinding>,
    pub active_tool: Option<ToolBinding>,
    pub projection_stale: bool,
    pub last_verification_passed: Option<bool>,
    pub compaction_source_sha256: Option<String>,
}

impl ReplayState {
    pub fn empty(project_id: &str, session_id: &str) -> Self {
        Self {
            project_id: project_id.to_owned(),
            session_id: session_id.to_owned(),
            sequence: 0,
            phase: None,
            instance_id: None,
            member_id: None,
            scope: None,
            active_turn_id: None,
            active_reasoning_run_id: None,
            planning_binding: None,
            active_tool: None,
            projection_stale: false,
            last_verification_passed: None,
            compaction_source_sha256: None,
        }
    }

    pub fn digest(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(digest::sha256_hex(&serde_json::to_vec(self)?))
    }

    pub fn planning_binding(&self) -> Option<&PlanningBinding> {
        self.planning_binding.as_ref()
    }

    pub fn apply(
        &mut self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.validate_identity(event)?;
        self.validate_coordinates(event)?;
        let next = self.transition(event)?;
        self.sequence = event.sequence;
        self.phase = Some(next);
        Ok(())
    }

    fn validate_identity(
        &mut self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if event.project_id != self.project_id || event.session_id != self.session_id {
            return Err("lifecycle event coordinates disagree with the session aggregate".into());
        }
        match (&self.instance_id, &self.scope) {
            (None, None) => {
                self.instance_id = Some(event.instance_id.clone());
                self.member_id = event.member_id.clone();
                self.scope = Some(event.scope.clone());
            }
            (Some(instance_id), Some(scope))
                if instance_id == &event.instance_id
                    && scope == &event.scope
                    && self.member_id == event.member_id => {}
            _ => {
                return Err(
                    "lifecycle instance, member, or scope changed within one session".into(),
                );
            }
        }
        Ok(())
    }

    fn validate_coordinates(
        &self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use LifecycleEventTypeV1 as Event;
        let opens_turn = event.event_type == Event::TurnOpened;
        let session_level = matches!(
            event.event_type,
            Event::SessionOpened
                | Event::ProjectAttunementRequested
                | Event::ProjectAttunementCompleted
                | Event::ProjectAttunementFailed
                | Event::SessionCompactionStarted
                | Event::SessionCompactionCompleted
                | Event::SessionClosed
        );
        if opens_turn && self.active_turn_id.is_some() {
            return Err("turn.opened cannot replace an active turn".into());
        }
        if !opens_turn && !session_level && event.turn_id != self.active_turn_id {
            return Err(format!(
                "lifecycle event {} does not belong to the active turn",
                event.event_type.as_str()
            )
            .into());
        }
        if matches!(
            event.event_type,
            Event::SessionCompactionStarted | Event::SessionCompactionCompleted
        ) && event.turn_id != self.active_turn_id
        {
            return Err("compaction does not preserve the active turn coordinate".into());
        }
        if event.event_type == Event::PlanRecorded {
            if self.active_reasoning_run_id.is_some()
                && event.reasoning_run_id != self.active_reasoning_run_id
            {
                return Err("plan addresses another reasoning run".into());
            }
        } else if event.reasoning_run_id.is_some()
            && event.reasoning_run_id != self.active_reasoning_run_id
        {
            return Err("lifecycle event addresses another reasoning run".into());
        }
        if event.event_type == Event::ToolProposed {
            if self.active_tool.is_some() {
                return Err("tool.proposed cannot replace an active tool".into());
            }
        } else if event.tool_call_id.is_some() {
            self.active_tool
                .as_ref()
                .ok_or("tool lifecycle event has no active tool")?
                .require_coordinates(event)?;
        }
        Ok(())
    }

    fn transition(
        &mut self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<LifecyclePhaseV1, Box<dyn std::error::Error>> {
        use LifecycleEventTypeV1 as Event;
        use LifecyclePhaseV1 as Phase;
        let current = self.phase;
        let next = match event.event_type {
            Event::SessionOpened if current.is_none() => Phase::SessionOpened,
            Event::ProjectAttunementRequested
                if matches!(
                    current,
                    Some(
                        Phase::SessionOpened | Phase::AttunementFailed | Phase::CompactionCompleted
                    )
                ) =>
            {
                Phase::AttunementPending
            }
            Event::ProjectAttunementCompleted if current == Some(Phase::AttunementPending) => {
                self.compaction_source_sha256 = None;
                Phase::Attuned
            }
            Event::ProjectAttunementFailed if current == Some(Phase::AttunementPending) => {
                Phase::AttunementFailed
            }
            Event::TurnOpened
                if matches!(current, Some(Phase::Attuned | Phase::TurnClosed))
                    && self.active_turn_id.is_none() =>
            {
                self.active_turn_id = event.turn_id.clone();
                self.last_verification_passed = None;
                Phase::TurnOpened
            }
            Event::PromptReceived if current == Some(Phase::TurnOpened) => Phase::PromptReceived,
            Event::PreflightStarted if current == Some(Phase::PromptReceived) => {
                Phase::PreflightPending
            }
            Event::PreflightCompleted if current == Some(Phase::PreflightPending) => {
                Phase::PreflightCompleted
            }
            Event::PreflightDenied if current == Some(Phase::PreflightPending) => {
                Phase::PreflightDenied
            }
            Event::TaskClassificationCompleted if current == Some(Phase::PreflightCompleted) => {
                Phase::TaskClassified
            }
            Event::ArchitectureAssessmentRequested
                if matches!(
                    current,
                    Some(Phase::TaskClassified | Phase::ArchitectureDecisionRequired)
                ) =>
            {
                Phase::ArchitecturePending
            }
            Event::ArchitectureAssessmentCompleted
                if current == Some(Phase::ArchitecturePending) =>
            {
                Phase::ArchitectureCompleted
            }
            Event::ArchitectureDecisionRequired if current == Some(Phase::ArchitecturePending) => {
                Phase::ArchitectureDecisionRequired
            }
            Event::ArchitectureAssessmentDenied if current == Some(Phase::ArchitecturePending) => {
                Phase::ArchitectureDenied
            }
            Event::PatternSelectionCompleted if current == Some(Phase::ArchitectureCompleted) => {
                Phase::PatternSelected
            }
            Event::PlanningAuthorized if current == Some(Phase::PatternSelected) => {
                self.planning_binding = Some(PlanningBinding::authorized(event)?);
                Phase::PlanningAuthorized
            }
            Event::PlanningDenied if current == Some(Phase::PatternSelected) => {
                Phase::PlanningDenied
            }
            Event::ContextAssembled if current == Some(Phase::PlanningAuthorized) => {
                Phase::ContextAssembled
            }
            Event::PlanRecorded if current == Some(Phase::ContextAssembled) => {
                self.planning_binding
                    .as_mut()
                    .ok_or("plan.recorded has no planning authorization")?
                    .record_plan(event)?;
                self.active_reasoning_run_id = event.reasoning_run_id.clone();
                Phase::PlanRecorded
            }
            Event::ToolProposed if self.may_propose_tool(current) => {
                self.planning_binding
                    .as_ref()
                    .ok_or("tool proposal has no active planning binding")?
                    .require_fresh_plan(event.occurred_at_unix_ms)?;
                self.active_tool = Some(ToolBinding::proposed(event)?);
                self.last_verification_passed = None;
                Phase::ToolProposed
            }
            Event::ToolAuthorized if current == Some(Phase::ToolProposed) => {
                self.active_tool
                    .as_mut()
                    .ok_or("tool.authorized has no active tool")?
                    .authorize(event)?;
                Phase::ToolAuthorized
            }
            Event::ToolDenied if current == Some(Phase::ToolProposed) => {
                self.clear_tool();
                Phase::ToolDenied
            }
            Event::ToolStarted if current == Some(Phase::ToolAuthorized) => {
                self.active_tool
                    .as_mut()
                    .ok_or("tool.started has no active tool")?
                    .consume(event)?;
                Phase::ToolStarted
            }
            Event::ToolCompleted if current == Some(Phase::ToolStarted) => {
                self.finish_tool(event)?;
                Phase::ToolCompleted
            }
            Event::ToolFailed if current == Some(Phase::ToolStarted) => {
                self.finish_tool(event)?;
                Phase::ToolFailed
            }
            Event::ProjectStateInvalidated
                if matches!(current, Some(Phase::ToolCompleted | Phase::ToolFailed))
                    && self.projection_stale =>
            {
                Phase::ProjectStateInvalidated
            }
            Event::ProjectionRefreshCompleted
                if matches!(
                    current,
                    Some(Phase::ProjectStateInvalidated | Phase::ProjectionFailed)
                ) && self.projection_stale =>
            {
                self.projection_stale = false;
                Phase::ProjectionReady
            }
            Event::ProjectionRefreshFailed
                if matches!(
                    current,
                    Some(Phase::ProjectStateInvalidated | Phase::ProjectionFailed)
                ) && self.projection_stale =>
            {
                Phase::ProjectionFailed
            }
            Event::VerificationCompleted if self.may_verify(current) => {
                self.last_verification_passed = event.payload.verification_passed();
                Phase::VerificationCompleted
            }
            Event::ReasoningOutcomeRecorded if self.may_record_outcome(current, event)? => {
                Phase::OutcomeRecorded
            }
            Event::TurnClosed if current == Some(Phase::OutcomeRecorded) => {
                self.close_turn(event)?;
                Phase::TurnClosed
            }
            Event::SessionCompactionStarted
                if current == Some(Phase::TurnClosed) && self.active_turn_id.is_none() =>
            {
                let LifecyclePayloadV1::Compaction { state_sha256 } = &event.payload else {
                    unreachable!("the public envelope validated its payload")
                };
                if state_sha256 != &self.digest()? {
                    return Err("compaction is not bound to the current lifecycle state".into());
                }
                self.compaction_source_sha256 = Some(state_sha256.clone());
                Phase::CompactionPending
            }
            Event::SessionCompactionCompleted if current == Some(Phase::CompactionPending) => {
                let LifecyclePayloadV1::Compaction { state_sha256 } = &event.payload else {
                    unreachable!("the public envelope validated its payload")
                };
                if Some(state_sha256) != self.compaction_source_sha256.as_ref() {
                    return Err("compaction completion addresses another source state".into());
                }
                Phase::CompactionCompleted
            }
            Event::SessionClosed
                if matches!(
                    current,
                    Some(
                        Phase::SessionOpened
                            | Phase::Attuned
                            | Phase::AttunementFailed
                            | Phase::TurnClosed
                    )
                ) && self.active_turn_id.is_none() =>
            {
                self.active_reasoning_run_id = None;
                self.planning_binding = None;
                self.compaction_source_sha256 = None;
                Phase::SessionClosed
            }
            _ => {
                return Err(format!(
                    "invalid lifecycle transition from {} through {}",
                    current.map_or("uninitialized", LifecyclePhaseV1::as_str),
                    event.event_type.as_str()
                )
                .into());
            }
        };
        Ok(next)
    }

    fn may_propose_tool(&self, current: Option<LifecyclePhaseV1>) -> bool {
        use LifecyclePhaseV1 as Phase;
        matches!(
            current,
            Some(
                Phase::PlanRecorded
                    | Phase::ToolCompleted
                    | Phase::ToolFailed
                    | Phase::ToolDenied
                    | Phase::ProjectionReady
            )
        ) && !self.projection_stale
            || current == Some(Phase::VerificationCompleted)
                && self.last_verification_passed == Some(false)
    }

    fn may_verify(&self, current: Option<LifecyclePhaseV1>) -> bool {
        use LifecyclePhaseV1 as Phase;
        matches!(
            current,
            Some(
                Phase::PlanRecorded
                    | Phase::ToolCompleted
                    | Phase::ToolFailed
                    | Phase::ToolDenied
                    | Phase::ProjectionReady
                    | Phase::ProjectionFailed
            )
        ) && !self.projection_stale
    }

    fn may_record_outcome(
        &self,
        current: Option<LifecyclePhaseV1>,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        use LifecyclePhaseV1 as Phase;
        let LifecyclePayloadV1::OutcomeRecorded { success, .. } = &event.payload else {
            unreachable!("the public envelope validated its payload")
        };
        if current == Some(Phase::VerificationCompleted) {
            if Some(*success) != self.last_verification_passed {
                return Err("reasoning outcome contradicts the verification result".into());
            }
            return Ok(true);
        }
        let denied = matches!(
            current,
            Some(Phase::PreflightDenied | Phase::ArchitectureDenied | Phase::PlanningDenied)
        );
        if denied && *success {
            return Err("a denied path cannot record a successful outcome".into());
        }
        Ok(denied)
    }

    fn finish_tool(
        &mut self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.active_tool
            .as_ref()
            .ok_or("tool result has no active tool")?
            .require_result(event)?;
        self.projection_stale = event
            .payload
            .project_state_changed()
            .ok_or("tool result omitted project-state disposition")?;
        self.clear_tool();
        Ok(())
    }

    fn close_turn(
        &mut self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let LifecyclePayloadV1::TurnClosed {
            status: LifecycleTurnStatusV1::Completed,
        } = event.payload
        {
            if self.last_verification_passed != Some(true) {
                return Err("turn cannot close completed without passing verification".into());
            }
        }
        self.active_turn_id = None;
        self.active_reasoning_run_id = None;
        self.planning_binding = None;
        self.clear_tool();
        self.projection_stale = false;
        self.last_verification_passed = None;
        Ok(())
    }

    fn clear_tool(&mut self) {
        self.active_tool = None;
    }
}
use super::binding::{PlanningBinding, ToolBinding};
