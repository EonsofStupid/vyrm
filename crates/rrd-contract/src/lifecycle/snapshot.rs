use super::{LifecycleEventEnvelopeV1, LifecyclePhaseV1, LIFECYCLE_SPEC_VERSION};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecycleSessionSnapshotV1 {
    pub spec_version: u16,
    pub instance_id: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    pub session_id: String,
    pub phase: LifecyclePhaseV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tool_call_id: Option<String>,
    pub event_count: u64,
    pub latest_event: LifecycleEventEnvelopeV1,
    pub state_sha256: String,
}

impl LifecycleSessionSnapshotV1 {
    pub fn is_v1(&self) -> bool {
        self.spec_version == LIFECYCLE_SPEC_VERSION
    }
}
