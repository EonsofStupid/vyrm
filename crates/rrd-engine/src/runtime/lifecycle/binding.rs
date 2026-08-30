use rrd_contract::{LifecycleEventEnvelopeV1, LifecyclePayloadV1};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct PlanningBinding {
    permit_sha256: String,
    expires_at_unix_ms: u64,
    plan_id: String,
    work_item_id: String,
    plan_revision: u64,
    plan_sha256: Option<String>,
    source_tree_sha256: Option<String>,
    verification_plan_sha256: Option<String>,
}

impl PlanningBinding {
    pub fn authorized(
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let LifecyclePayloadV1::PlanningAuthorized {
            permit_sha256,
            expires_at_unix_ms,
            plan_id,
            plan_revision,
            work_item_id,
        } = &event.payload
        else {
            return Err("planning authorization has the wrong payload".into());
        };
        if *expires_at_unix_ms <= event.occurred_at_unix_ms {
            return Err("planning permit is already expired at authorization".into());
        }
        Ok(Self {
            permit_sha256: permit_sha256.clone(),
            expires_at_unix_ms: *expires_at_unix_ms,
            plan_id: plan_id.clone(),
            work_item_id: work_item_id.clone(),
            plan_revision: *plan_revision,
            plan_sha256: None,
            source_tree_sha256: None,
            verification_plan_sha256: None,
        })
    }

    pub fn record_plan(
        &mut self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let LifecyclePayloadV1::PlanRecorded {
            plan_sha256,
            plan_id,
            plan_revision,
            work_item_id,
            source_tree_sha256,
            verification_plan_sha256,
        } = &event.payload
        else {
            return Err("plan recording has the wrong payload".into());
        };
        if plan_id != &self.plan_id || work_item_id != &self.work_item_id {
            return Err("recorded plan does not match the authorized plan and work item".into());
        }
        if self.plan_sha256.is_some() {
            return Err("planning binding already contains a recorded plan".into());
        }
        if *plan_revision != self.plan_revision {
            return Err("recorded plan revision differs from the planning permit".into());
        }
        self.plan_sha256 = Some(plan_sha256.clone());
        self.source_tree_sha256 = Some(source_tree_sha256.clone());
        self.verification_plan_sha256 = Some(verification_plan_sha256.clone());
        Ok(())
    }

    pub fn require_fresh_plan(&self, at: u64) -> Result<(), Box<dyn std::error::Error>> {
        if at >= self.expires_at_unix_ms {
            return Err("planning permit expired before tool proposal".into());
        }
        if self.plan_sha256.is_none()
            || self.source_tree_sha256.is_none()
            || self.verification_plan_sha256.is_none()
        {
            return Err("tool proposal has no fully recorded planning binding".into());
        }
        Ok(())
    }

    pub fn authority_coordinates(
        &self,
    ) -> (&str, u64, &str, Option<&str>, Option<&str>, Option<&str>) {
        (
            &self.plan_id,
            self.plan_revision,
            &self.work_item_id,
            self.plan_sha256.as_deref(),
            self.source_tree_sha256.as_deref(),
            self.verification_plan_sha256.as_deref(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct ToolBinding {
    pub attempt_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub tool_request_sha256: String,
    pub mutation: bool,
    pub authorization_sha256: Option<String>,
    pub consumed: bool,
}

impl ToolBinding {
    pub fn proposed(event: &LifecycleEventEnvelopeV1) -> Result<Self, Box<dyn std::error::Error>> {
        let LifecyclePayloadV1::ToolProposed {
            tool_name,
            tool_request_sha256,
            mutation,
        } = &event.payload
        else {
            return Err("tool proposal has the wrong payload".into());
        };
        Ok(Self {
            attempt_id: event
                .attempt_id
                .clone()
                .ok_or("tool proposal has no attempt identity")?,
            tool_call_id: event
                .tool_call_id
                .clone()
                .ok_or("tool proposal has no tool-call identity")?,
            tool_name: tool_name.clone(),
            tool_request_sha256: tool_request_sha256.clone(),
            mutation: *mutation,
            authorization_sha256: None,
            consumed: false,
        })
    }

    pub fn authorize(
        &mut self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let LifecyclePayloadV1::ToolDecision {
            decision_sha256,
            allowed: true,
            ..
        } = &event.payload
        else {
            return Err("tool authorization has the wrong decision payload".into());
        };
        if self.authorization_sha256.is_some() {
            return Err("tool proposal was already authorized".into());
        }
        self.authorization_sha256 = Some(decision_sha256.clone());
        Ok(())
    }

    pub fn require_coordinates(
        &self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if event.attempt_id.as_deref() != Some(self.attempt_id.as_str())
            || event.tool_call_id.as_deref() != Some(self.tool_call_id.as_str())
        {
            return Err("lifecycle event addresses another tool attempt".into());
        }
        Ok(())
    }

    pub fn consume(
        &mut self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let LifecyclePayloadV1::ToolStarted {
            authorization_sha256,
        } = &event.payload
        else {
            return Err("tool start has the wrong payload".into());
        };
        if self.consumed {
            return Err("tool authorization was already consumed".into());
        }
        if self.authorization_sha256.as_deref() != Some(authorization_sha256.as_str()) {
            return Err("tool start does not match the authorized decision".into());
        }
        self.consumed = true;
        Ok(())
    }

    pub fn require_result(
        &self,
        event: &LifecycleEventEnvelopeV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let LifecyclePayloadV1::ToolFinished {
            tool_request_sha256,
            ..
        } = &event.payload
        else {
            return Err("tool result has the wrong payload".into());
        };
        if !self.consumed || tool_request_sha256 != &self.tool_request_sha256 {
            return Err("tool result does not match one consumed authorization".into());
        }
        Ok(())
    }

    pub(super) fn authorization_sha256(&self) -> Option<&str> {
        self.authorization_sha256.as_deref()
    }
}
