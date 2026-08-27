use super::event::{
    LifecycleEnforcementLevelV1, LifecycleEventTypeV1, LIFECYCLE_SPEC_VERSION,
    MAX_LIFECYCLE_EVENT_BYTES,
};
use super::payload::LifecyclePayloadV1;
use super::validation;
use crate::{invalid, ContractError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecycleReadStampV1 {
    pub runtime_cursor: u64,
    pub manifest_id: String,
    pub catalogue_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecycleTraceContextV1 {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecycleEventCommandV1 {
    pub event_type: LifecycleEventTypeV1,
    pub occurred_at_unix_ms: u64,
    pub instance_id: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub correlation_id: String,
    pub actor: String,
    pub adapter_kind: String,
    pub adapter_version: String,
    pub enforcement_level: LifecycleEnforcementLevelV1,
    pub scope: String,
    pub payload: LifecyclePayloadV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_stamp: Option<LifecycleReadStampV1>,
    pub trace: LifecycleTraceContextV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecycleEventEnvelopeV1 {
    pub spec_version: u16,
    pub sequence: u64,
    pub event_id: String,
    pub event_type: LifecycleEventTypeV1,
    pub occurred_at_unix_ms: u64,
    pub recorded_at_unix_ms: u64,
    pub instance_id: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub correlation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    pub actor: String,
    pub adapter_kind: String,
    pub adapter_version: String,
    pub enforcement_level: LifecycleEnforcementLevelV1,
    pub scope: String,
    pub payload: LifecyclePayloadV1,
    pub payload_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_state_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_stamp: Option<LifecycleReadStampV1>,
    pub trace: LifecycleTraceContextV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_event_sha256: Option<String>,
    pub event_sha256: String,
}

impl LifecycleEventCommandV1 {
    pub fn validate(&self) -> Result<()> {
        validate_common(self)?;
        self.payload.validate_for(self.event_type)?;
        validate_coordinates(
            self.event_type,
            self.turn_id.as_deref(),
            self.reasoning_run_id.as_deref(),
            self.attempt_id.as_deref(),
            self.tool_call_id.as_deref(),
        )?;
        if self.occurred_at_unix_ms == 0 {
            return invalid("lifecycle occurrence time must be non-zero");
        }
        validate_size(self)
    }
}

impl LifecycleEventEnvelopeV1 {
    pub fn seal(&mut self) -> Result<()> {
        self.payload_sha256 = sha256(&serde_json::to_vec(&self.payload).map_err(json)?);
        self.event_sha256.clear();
        self.validate_fields(false)?;
        self.event_sha256 = sha256(&serde_json::to_vec(self).map_err(json)?);
        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        self.validate_fields(true)?;
        if self.payload_sha256 != sha256(&serde_json::to_vec(&self.payload).map_err(json)?) {
            return invalid("lifecycle payload digest mismatch");
        }
        let expected = self.event_sha256.clone();
        let mut candidate = self.clone();
        candidate.seal()?;
        if candidate.event_sha256 != expected {
            return invalid("lifecycle event digest mismatch");
        }
        Ok(())
    }

    fn validate_fields(&self, require_event_digest: bool) -> Result<()> {
        if self.spec_version != LIFECYCLE_SPEC_VERSION || self.sequence == 0 {
            return invalid("invalid lifecycle spec version or sequence");
        }
        validate_common(self)?;
        validation::identity("event id", &self.event_id)?;
        validation::optional_identity("causation id", &self.causation_id)?;
        validation::sha256("payload digest", &self.payload_sha256)?;
        validation::optional_sha256("previous state digest", &self.previous_state_sha256)?;
        validation::optional_sha256("previous event digest", &self.previous_event_sha256)?;
        if require_event_digest {
            validation::sha256("event digest", &self.event_sha256)?;
        }
        if self.occurred_at_unix_ms == 0 || self.recorded_at_unix_ms < self.occurred_at_unix_ms {
            return invalid("invalid lifecycle occurrence or recording time");
        }
        self.payload.validate_for(self.event_type)?;
        validate_coordinates(
            self.event_type,
            self.turn_id.as_deref(),
            self.reasoning_run_id.as_deref(),
            self.attempt_id.as_deref(),
            self.tool_call_id.as_deref(),
        )?;
        validate_size(self)
    }
}

trait CommonFields {
    fn instance_id(&self) -> &str;
    fn project_id(&self) -> &str;
    fn member_id(&self) -> &Option<String>;
    fn session_id(&self) -> &str;
    fn turn_id(&self) -> &Option<String>;
    fn reasoning_run_id(&self) -> &Option<String>;
    fn attempt_id(&self) -> &Option<String>;
    fn tool_call_id(&self) -> &Option<String>;
    fn correlation_id(&self) -> &str;
    fn actor(&self) -> &str;
    fn adapter_kind(&self) -> &str;
    fn adapter_version(&self) -> &str;
    fn scope(&self) -> &str;
    fn read_stamp(&self) -> &Option<LifecycleReadStampV1>;
    fn trace(&self) -> &LifecycleTraceContextV1;
}

macro_rules! common_fields {
    ($type:ty) => {
        impl CommonFields for $type {
            fn instance_id(&self) -> &str {
                &self.instance_id
            }
            fn project_id(&self) -> &str {
                &self.project_id
            }
            fn member_id(&self) -> &Option<String> {
                &self.member_id
            }
            fn session_id(&self) -> &str {
                &self.session_id
            }
            fn turn_id(&self) -> &Option<String> {
                &self.turn_id
            }
            fn reasoning_run_id(&self) -> &Option<String> {
                &self.reasoning_run_id
            }
            fn attempt_id(&self) -> &Option<String> {
                &self.attempt_id
            }
            fn tool_call_id(&self) -> &Option<String> {
                &self.tool_call_id
            }
            fn correlation_id(&self) -> &str {
                &self.correlation_id
            }
            fn actor(&self) -> &str {
                &self.actor
            }
            fn adapter_kind(&self) -> &str {
                &self.adapter_kind
            }
            fn adapter_version(&self) -> &str {
                &self.adapter_version
            }
            fn scope(&self) -> &str {
                &self.scope
            }
            fn read_stamp(&self) -> &Option<LifecycleReadStampV1> {
                &self.read_stamp
            }
            fn trace(&self) -> &LifecycleTraceContextV1 {
                &self.trace
            }
        }
    };
}

common_fields!(LifecycleEventCommandV1);
common_fields!(LifecycleEventEnvelopeV1);

fn validate_common(value: &impl CommonFields) -> Result<()> {
    validation::identity("instance id", value.instance_id())?;
    validation::identity("project id", value.project_id())?;
    validation::optional_identity("member id", value.member_id())?;
    validation::identity("session id", value.session_id())?;
    validation::optional_identity("turn id", value.turn_id())?;
    validation::optional_identity("reasoning run id", value.reasoning_run_id())?;
    validation::optional_identity("attempt id", value.attempt_id())?;
    validation::optional_identity("tool call id", value.tool_call_id())?;
    validation::identity("correlation id", value.correlation_id())?;
    validation::text("actor", value.actor())?;
    validation::identity("adapter kind", value.adapter_kind())?;
    validation::text("adapter version", value.adapter_version())?;
    validation::identity("scope", value.scope())?;
    if let Some(stamp) = value.read_stamp() {
        stamp.validate()?;
    }
    value.trace().validate()
}

fn validate_coordinates(
    event_type: LifecycleEventTypeV1,
    turn_id: Option<&str>,
    reasoning_run_id: Option<&str>,
    attempt_id: Option<&str>,
    tool_call_id: Option<&str>,
) -> Result<()> {
    let turn_forbidden = matches!(
        event_type,
        LifecycleEventTypeV1::SessionOpened
            | LifecycleEventTypeV1::ProjectAttunementRequested
            | LifecycleEventTypeV1::ProjectAttunementCompleted
            | LifecycleEventTypeV1::ProjectAttunementFailed
            | LifecycleEventTypeV1::SessionClosed
    );
    let turn_optional = matches!(
        event_type,
        LifecycleEventTypeV1::SessionCompactionStarted
            | LifecycleEventTypeV1::SessionCompactionCompleted
    );
    if (turn_forbidden && turn_id.is_some())
        || (!turn_forbidden && !turn_optional && turn_id.is_none())
    {
        return invalid("lifecycle turn coordinate does not match the event type");
    }
    let tool_event = matches!(
        event_type,
        LifecycleEventTypeV1::ToolProposed
            | LifecycleEventTypeV1::ToolAuthorized
            | LifecycleEventTypeV1::ToolDenied
            | LifecycleEventTypeV1::ToolStarted
            | LifecycleEventTypeV1::ToolCompleted
            | LifecycleEventTypeV1::ToolFailed
    );
    if tool_event != tool_call_id.is_some() {
        return invalid("lifecycle tool-call coordinate does not match the event type");
    }
    let reasoning_required = matches!(
        event_type,
        LifecycleEventTypeV1::PlanRecorded
            | LifecycleEventTypeV1::ToolProposed
            | LifecycleEventTypeV1::ToolAuthorized
            | LifecycleEventTypeV1::ToolDenied
            | LifecycleEventTypeV1::ToolStarted
            | LifecycleEventTypeV1::ToolCompleted
            | LifecycleEventTypeV1::ToolFailed
            | LifecycleEventTypeV1::ProjectStateInvalidated
            | LifecycleEventTypeV1::ProjectionRefreshCompleted
            | LifecycleEventTypeV1::ProjectionRefreshFailed
            | LifecycleEventTypeV1::VerificationCompleted
            | LifecycleEventTypeV1::ReasoningOutcomeRecorded
    );
    if reasoning_required && reasoning_run_id.is_none() {
        return invalid("lifecycle event requires a reasoning-run coordinate");
    }
    if tool_event != attempt_id.is_some() {
        return invalid("lifecycle attempt coordinate does not match the event type");
    }
    if !tool_event && tool_call_id.is_none() && attempt_id.is_some() {
        return invalid("lifecycle attempt coordinate requires a tool event");
    }
    Ok(())
}

impl LifecycleReadStampV1 {
    pub fn validate(&self) -> Result<()> {
        validation::identity("read manifest", &self.manifest_id)
    }
}

impl LifecycleTraceContextV1 {
    pub fn validate(&self) -> Result<()> {
        validation::hex("trace id", &self.trace_id, 32)?;
        validation::hex("span id", &self.span_id, 16)?;
        if let Some(parent) = &self.parent_span_id {
            validation::hex("parent span id", parent, 16)?;
        }
        Ok(())
    }
}

fn validate_size(value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(json)?;
    if bytes.len() > MAX_LIFECYCLE_EVENT_BYTES {
        return invalid(format!(
            "lifecycle event exceeds {MAX_LIFECYCLE_EVENT_BYTES} bytes"
        ));
    }
    Ok(())
}

fn json(error: serde_json::Error) -> ContractError {
    ContractError(error.to_string())
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
