use crate::{
    invalid, validate_protocol, validate_sha256, CanonicalId, Result, SecurityAction,
    MAX_MESSAGE_BYTES, PROTOCOL, PROTOCOL_VERSION,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const RUNTIME_TOOL_CATALOGUE_VERSION: u16 = 1;
pub const MAX_RUNTIME_TOOLS: usize = 256;
pub const MAX_RUNTIME_TOOL_ARGUMENT_BYTES: usize = 768 * 1024;
pub const MAX_RUNTIME_TOOL_RESULT_BYTES: usize = 768 * 1024;

/// Empty, explicitly typed request for the governed runtime-tool catalogue.
///
/// The catalogue uses POST because RRD session authentication and correlation
/// coordinates are carried in a `RequestEnvelope`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListRuntimeTools {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeToolAuthorization {
    Public,
    Governed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeToolLifecyclePolicy {
    /// The operation cannot mutate authoritative or derived RRD state.
    ReadOnly,
    /// The operation advances or rebuilds RRD's own guarded control state and
    /// is validated by that control plane's state machine.
    ControlTransition,
    /// Execute only the exact verification argv already persisted by the
    /// work-plan state machine, then seal bounded process evidence. This is a
    /// privileged host-process boundary, not an ordinary control transition
    /// and not a project mutation proposed by an AI tool call.
    VerificationExecution,
    /// The operation can change project, data, index, archive, or deployment
    /// state and must consume one exact canonical lifecycle authorization.
    PlannedMutation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeToolDescriptor {
    pub name: CanonicalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<CanonicalId>,
    pub description: String,
    pub input_schema: Value,
    pub mutation: bool,
    pub authorization: RuntimeToolAuthorization,
    pub action: SecurityAction,
    pub lifecycle: RuntimeToolLifecyclePolicy,
}

impl RuntimeToolDescriptor {
    pub fn validate(&self) -> Result<()> {
        if !self.name.as_str().starts_with("rrflow_") {
            return invalid("runtime tool names must start with rrflow_");
        }
        if self.description.is_empty() || self.description.len() > MAX_MESSAGE_BYTES {
            return invalid(format!(
                "runtime tool description length must be in 1..={MAX_MESSAGE_BYTES} bytes"
            ));
        }
        if !self.input_schema.is_object() {
            return invalid("runtime tool input schema must be a JSON object");
        }
        let schema_bytes = serde_json::to_vec(&self.input_schema)
            .map_err(|error| crate::ContractError(error.to_string()))?;
        if schema_bytes.len() > MAX_RUNTIME_TOOL_ARGUMENT_BYTES {
            return invalid("runtime tool input schema exceeds the transport bound");
        }
        if self.authorization == RuntimeToolAuthorization::Public
            && (self.mutation || self.action != SecurityAction::ServiceInspect)
        {
            return invalid("public runtime tools must be read-only service inspection");
        }
        match (self.mutation, self.lifecycle) {
            (false, RuntimeToolLifecyclePolicy::ReadOnly)
            | (true, RuntimeToolLifecyclePolicy::ControlTransition)
            | (true, RuntimeToolLifecyclePolicy::VerificationExecution)
            | (true, RuntimeToolLifecyclePolicy::PlannedMutation) => {}
            (false, _) => {
                return invalid("read-only runtime tools must use the read_only lifecycle policy")
            }
            (true, RuntimeToolLifecyclePolicy::ReadOnly) => {
                return invalid("mutating runtime tools cannot use the read_only lifecycle policy")
            }
        }
        if self.lifecycle == RuntimeToolLifecyclePolicy::PlannedMutation
            && self.authorization != RuntimeToolAuthorization::Governed
        {
            return invalid("planned runtime mutations must use governed authorization");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeToolCatalogue {
    pub protocol: String,
    pub protocol_version: u16,
    pub catalogue_version: u16,
    pub tools: Vec<RuntimeToolDescriptor>,
}

impl RuntimeToolCatalogue {
    pub fn validate(&self) -> Result<()> {
        validate_protocol(&self.protocol, self.protocol_version)?;
        if self.catalogue_version != RUNTIME_TOOL_CATALOGUE_VERSION {
            return invalid("unsupported runtime tool catalogue version");
        }
        if self.tools.is_empty() || self.tools.len() > MAX_RUNTIME_TOOLS {
            return invalid(format!(
                "runtime tool catalogue must contain 1..={MAX_RUNTIME_TOOLS} tools"
            ));
        }
        let mut names = BTreeSet::new();
        for tool in &self.tools {
            tool.validate()?;
            if !names.insert(tool.name.as_str()) {
                return invalid("runtime tool names must be unique");
            }
        }
        if self
            .tools
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
        {
            return invalid("runtime tools must be sorted by canonical name");
        }
        Ok(())
    }
}

impl Default for RuntimeToolCatalogue {
    fn default() -> Self {
        Self {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            catalogue_version: RUNTIME_TOOL_CATALOGUE_VERSION,
            tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeToolInvocation {
    pub catalogue_version: u16,
    pub tool: CanonicalId,
    pub arguments: Value,
    pub arguments_sha256: String,
}

impl RuntimeToolInvocation {
    pub fn validate(&self) -> Result<()> {
        if self.catalogue_version != RUNTIME_TOOL_CATALOGUE_VERSION {
            return invalid("runtime invocation catalogue version is unsupported");
        }
        if !self.tool.as_str().starts_with("rrflow_") {
            return invalid("runtime invocation tool must start with rrflow_");
        }
        if !self.arguments.is_object() {
            return invalid("runtime tool arguments must be a JSON object");
        }
        let encoded = serde_json::to_vec(&self.arguments)
            .map_err(|error| crate::ContractError(error.to_string()))?;
        if encoded.len() > MAX_RUNTIME_TOOL_ARGUMENT_BYTES {
            return invalid("runtime tool arguments exceed the transport bound");
        }
        validate_sha256(&self.arguments_sha256, "arguments_sha256")?;
        if self.arguments_sha256 != runtime_tool_arguments_sha256(&self.arguments)? {
            return invalid("runtime tool arguments digest does not match arguments");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeToolInvocationResult {
    pub catalogue_version: u16,
    pub tool: CanonicalId,
    pub arguments_sha256: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub content_sha256: String,
}

impl RuntimeToolInvocationResult {
    pub fn validate(&self) -> Result<()> {
        if self.catalogue_version != RUNTIME_TOOL_CATALOGUE_VERSION {
            return invalid("runtime result catalogue version is unsupported");
        }
        if !self.tool.as_str().starts_with("rrflow_") {
            return invalid("runtime result tool must start with rrflow_");
        }
        validate_sha256(&self.arguments_sha256, "arguments_sha256")?;
        if self.content.len() > MAX_RUNTIME_TOOL_RESULT_BYTES {
            return invalid("runtime tool result exceeds the transport bound");
        }
        if self
            .detail
            .as_ref()
            .is_some_and(|detail| detail.is_empty() || detail.len() > MAX_MESSAGE_BYTES)
        {
            return invalid(format!(
                "runtime tool result detail length must be in 1..={MAX_MESSAGE_BYTES} bytes"
            ));
        }
        validate_sha256(&self.content_sha256, "content_sha256")?;
        if self.content_sha256 != sha256_hex(self.content.as_bytes()) {
            return invalid("runtime tool content digest does not match content");
        }
        Ok(())
    }
}

pub fn runtime_tool_arguments_sha256(arguments: &Value) -> Result<String> {
    if !arguments.is_object() {
        return invalid("runtime tool arguments must be a JSON object");
    }
    let encoded =
        serde_json::to_vec(arguments).map_err(|error| crate::ContractError(error.to_string()))?;
    if encoded.len() > MAX_RUNTIME_TOOL_ARGUMENT_BYTES {
        return invalid("runtime tool arguments exceed the transport bound");
    }
    Ok(sha256_hex(&encoded))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}
