use super::validation;
use super::LifecycleEnforcementLevelV1;
use crate::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Adapter-neutral coordinates for one supervised lifecycle session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecycleSupervisorContextV1 {
    pub scope: String,
    pub project_id: String,
    pub session_id: String,
    pub actor: String,
    pub adapter_kind: String,
    pub adapter_version: String,
    pub enforcement_level: LifecycleEnforcementLevelV1,
}

/// Exact tool identity proposed at the mutation boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecycleToolRequestV1 {
    pub tool_name: String,
    pub tool_request_sha256: String,
    pub tool_call_id: String,
    pub mutation: bool,
}

/// Durable authorization returned to the execution boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecycleToolAuthorizationV1 {
    pub attempt_id: String,
    pub tool_call_id: String,
    pub tool_request_sha256: String,
    pub decision_sha256: String,
}

/// Exact observation captured after the authorized tool returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecycleToolCompletionV1 {
    pub observation_sha256: String,
    pub success: bool,
    pub project_state_changed: bool,
}

impl LifecycleSupervisorContextV1 {
    pub fn validate(&self) -> Result<()> {
        validation::identity("supervisor scope", &self.scope)?;
        validation::identity("supervisor project id", &self.project_id)?;
        validation::identity("supervisor session id", &self.session_id)?;
        validation::text("supervisor actor", &self.actor)?;
        validation::identity("supervisor adapter kind", &self.adapter_kind)?;
        validation::text("supervisor adapter version", &self.adapter_version)
    }
}

impl LifecycleToolRequestV1 {
    pub fn validate(&self) -> Result<()> {
        validation::text("supervisor tool name", &self.tool_name)?;
        validation::sha256("supervisor tool request", &self.tool_request_sha256)?;
        validation::identity("supervisor tool-call id", &self.tool_call_id)
    }
}

impl LifecycleToolAuthorizationV1 {
    pub fn validate(&self) -> Result<()> {
        validation::identity("supervisor attempt id", &self.attempt_id)?;
        validation::identity("supervisor authorization tool-call id", &self.tool_call_id)?;
        validation::sha256(
            "supervisor authorization tool request",
            &self.tool_request_sha256,
        )?;
        validation::sha256("supervisor authorization decision", &self.decision_sha256)
    }
}

impl LifecycleToolCompletionV1 {
    pub fn validate(&self) -> Result<()> {
        validation::sha256("supervisor tool observation", &self.observation_sha256)
    }
}
