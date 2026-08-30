use super::{validation, LifecycleEnforcementLevelV1, LifecycleEventTypeV1};
use crate::{invalid, ContractError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const ADAPTER_CONFORMANCE_FORMAT_VERSION: u16 = 1;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKindV1 {
    ClaudeCode,
    Codex,
    Gemini,
    Copilot,
    OpenAiOrchestrator,
    XaiOrchestrator,
    Mcp,
    LocalRuntime,
}

impl AdapterKindV1 {
    pub const ALL: [Self; 8] = [
        Self::ClaudeCode,
        Self::Codex,
        Self::Gemini,
        Self::Copilot,
        Self::OpenAiOrchestrator,
        Self::XaiOrchestrator,
        Self::Mcp,
        Self::LocalRuntime,
    ];
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AdapterMutationClassV1 {
    Patch,
    Shell,
    GeneratedCode,
    External,
    HostedTool,
    Subagent,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AdapterCoveragePointV1 {
    SessionStartResume,
    PlanningGate,
    PreToolPatch,
    PreToolShell,
    PreToolGeneratedCode,
    PreToolExternal,
    PostTool,
    ToolFailure,
    Compaction,
    StopSessionEnd,
    Subagent,
    HostedTool,
    TimeoutFailure,
    CrashFailure,
    InvalidOutput,
    TrustFailure,
}

impl AdapterCoveragePointV1 {
    pub const ALL: [Self; 16] = [
        Self::SessionStartResume,
        Self::PlanningGate,
        Self::PreToolPatch,
        Self::PreToolShell,
        Self::PreToolGeneratedCode,
        Self::PreToolExternal,
        Self::PostTool,
        Self::ToolFailure,
        Self::Compaction,
        Self::StopSessionEnd,
        Self::Subagent,
        Self::HostedTool,
        Self::TimeoutFailure,
        Self::CrashFailure,
        Self::InvalidOutput,
        Self::TrustFailure,
    ];

    const MUTATION_READINESS: [Self; 10] = [
        Self::PreToolPatch,
        Self::PreToolShell,
        Self::PreToolGeneratedCode,
        Self::PreToolExternal,
        Self::Subagent,
        Self::HostedTool,
        Self::TimeoutFailure,
        Self::CrashFailure,
        Self::InvalidOutput,
        Self::TrustFailure,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdapterCoverageStateV1 {
    Enforced,
    Observed,
    Uncovered,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdapterCoverageEntryV1 {
    pub point: AdapterCoveragePointV1,
    pub state: AdapterCoverageStateV1,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdapterCoverageVectorV1 {
    pub format: u16,
    pub adapter: AdapterKindV1,
    pub adapter_version: String,
    pub enforcement_level: LifecycleEnforcementLevelV1,
    pub planning_enforced: bool,
    pub mutation_enforced: bool,
    pub entries: Vec<AdapterCoverageEntryV1>,
    pub coverage_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CanonicalAdapterEventV1 {
    pub format: u16,
    pub adapter: AdapterKindV1,
    pub adapter_version: String,
    pub native_event: String,
    pub canonical_event: LifecycleEventTypeV1,
    pub session_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub tool_input_sha256: String,
    pub mutation_class: AdapterMutationClassV1,
    pub event_sha256: String,
}

impl AdapterCoverageVectorV1 {
    pub fn seal(&mut self) -> Result<()> {
        self.coverage_sha256.clear();
        self.validate_fields(false)?;
        self.coverage_sha256 = sealed_sha256(b"rrflow-adapter-coverage-v1\0", self)?;
        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        self.validate_fields(true)?;
        let expected = self.coverage_sha256.clone();
        let mut candidate = self.clone();
        candidate.seal()?;
        if candidate.coverage_sha256 != expected {
            return invalid("adapter coverage digest mismatch");
        }
        Ok(())
    }

    pub fn uncovered(&self) -> Vec<AdapterCoveragePointV1> {
        self.entries
            .iter()
            .filter_map(|entry| {
                (entry.state == AdapterCoverageStateV1::Uncovered).then_some(entry.point)
            })
            .collect()
    }

    fn validate_fields(&self, require_digest: bool) -> Result<()> {
        if self.format != ADAPTER_CONFORMANCE_FORMAT_VERSION {
            return invalid("unsupported adapter coverage format");
        }
        validation::text("adapter version", &self.adapter_version)?;
        if self.entries.len() != AdapterCoveragePointV1::ALL.len() {
            return invalid("adapter coverage must declare every canonical point");
        }
        let mut points = BTreeSet::new();
        for entry in &self.entries {
            validation::text("adapter coverage reason", &entry.reason)?;
            if !points.insert(entry.point) {
                return invalid("adapter coverage contains a duplicate point");
            }
        }
        if points
            != AdapterCoveragePointV1::ALL
                .into_iter()
                .collect::<BTreeSet<_>>()
        {
            return invalid("adapter coverage omits or invents a canonical point");
        }
        let state = |point| {
            self.entries
                .iter()
                .find(|entry| entry.point == point)
                .expect("complete point set")
                .state
        };
        let planning_enforced =
            state(AdapterCoveragePointV1::PlanningGate) == AdapterCoverageStateV1::Enforced;
        let mutation_enforced =
            AdapterCoveragePointV1::MUTATION_READINESS
                .into_iter()
                .all(|point| {
                    matches!(
                        state(point),
                        AdapterCoverageStateV1::Enforced | AdapterCoverageStateV1::NotApplicable
                    )
                });
        if self.planning_enforced != planning_enforced
            || self.mutation_enforced != mutation_enforced
        {
            return invalid("adapter readiness contradicts its coverage points");
        }
        if require_digest {
            validation::sha256("adapter coverage", &self.coverage_sha256)?;
        }
        Ok(())
    }
}

impl CanonicalAdapterEventV1 {
    pub fn seal(&mut self) -> Result<()> {
        self.event_sha256.clear();
        self.validate_fields(false)?;
        self.event_sha256 = sealed_sha256(b"rrflow-canonical-adapter-event-v1\0", self)?;
        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        self.validate_fields(true)?;
        let expected = self.event_sha256.clone();
        let mut candidate = self.clone();
        candidate.seal()?;
        if candidate.event_sha256 != expected {
            return invalid("canonical adapter event digest mismatch");
        }
        Ok(())
    }

    fn validate_fields(&self, require_digest: bool) -> Result<()> {
        if self.format != ADAPTER_CONFORMANCE_FORMAT_VERSION {
            return invalid("unsupported canonical adapter event format");
        }
        validation::text("adapter version", &self.adapter_version)?;
        validation::text("adapter native event", &self.native_event)?;
        if self.canonical_event != LifecycleEventTypeV1::ToolProposed {
            return invalid("adapter tool fixture must normalize to tool.proposed");
        }
        validation::identity("adapter session", &self.session_id)?;
        validation::identity("adapter tool call", &self.tool_call_id)?;
        validation::text("adapter tool name", &self.tool_name)?;
        validation::sha256("adapter tool input", &self.tool_input_sha256)?;
        if require_digest {
            validation::sha256("canonical adapter event", &self.event_sha256)?;
        }
        Ok(())
    }
}

fn sealed_sha256(prefix: &[u8], value: &impl Serialize) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(serde_json::to_vec(value).map_err(json)?);
    Ok(lowercase_hex(&hasher.finalize()))
}

fn json(error: serde_json::Error) -> ContractError {
    ContractError(error.to_string())
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
