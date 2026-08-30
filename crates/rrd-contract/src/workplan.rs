//! Versioned work-plan and lifecycle contracts used to govern RRFlow changes.

use crate::{invalid, ContractError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const WORK_PLAN_SCHEMA_VERSION: u16 = 1;
pub const MAX_WORK_PLAN_GATES: usize = 128;
pub const MAX_WORK_PLAN_ITEMS: usize = 2_048;
pub const MAX_WORK_PLAN_TEXT_BYTES: usize = 8_192;

/// Stable operation identities for every public work-plan surface.
///
/// RRD owns this set. CLI, MCP, Connectome, and future adapters consume the
/// engine catalogue generated from these identities instead of maintaining
/// local command or tool registries.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkPlanOperation {
    Sync,
    Status,
    Activate,
    Record,
    Verify,
}

impl WorkPlanOperation {
    pub const ALL: [Self; 5] = [
        Self::Sync,
        Self::Status,
        Self::Activate,
        Self::Record,
        Self::Verify,
    ];

    pub const fn runtime_tool_name(self) -> &'static str {
        match self {
            Self::Sync => "rrflow_work_plan_sync",
            Self::Status => "rrflow_work_plan_status",
            Self::Activate => "rrflow_work_item_activate",
            Self::Record => "rrflow_work_item_plan_record",
            Self::Verify => "rrflow_work_item_verify",
        }
    }

    pub const fn capability_id(self) -> &'static str {
        match self {
            Self::Sync => "work-plan-sync",
            Self::Status => "work-plan-status",
            Self::Activate => "work-item-activate",
            Self::Record => "work-item-plan-record",
            Self::Verify => "work-item-verify",
        }
    }

    pub const fn cli_command(self) -> &'static str {
        match self {
            Self::Sync => "rrflow work-plan sync",
            Self::Status => "rrflow work-plan status",
            Self::Activate => "rrflow work-plan activate",
            Self::Record => "rrflow work-plan record",
            Self::Verify => "rrflow work-plan verify",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkPlanDefinition {
    pub schema_version: u16,
    pub plan_id: String,
    pub title: String,
    pub authority: String,
    pub status_policy: String,
    pub gate: Vec<WorkGateDefinition>,
    pub item: Vec<WorkItemDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkGateDefinition {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkItemDefinition {
    pub id: String,
    pub gate: String,
    pub title: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub acceptance: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemStatus {
    Pending,
    Active,
    Verifying,
    Verified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkItemStatusSnapshot {
    pub item_id: String,
    pub status: WorkItemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkPlanSnapshot {
    pub schema_version: u16,
    pub plan_id: String,
    pub plan_sha256: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_item_id: Option<String>,
    pub items: Vec<WorkItemStatusSnapshot>,
    pub event_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkPlanEventKind {
    WorkPlanLoaded,
    WorkItemActivated,
    PlanRecorded,
    ToolProposed,
    ToolAuthorized,
    ToolCompleted,
    VerificationCompleted,
    WorkItemVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkPlanEventEnvelope {
    pub schema_version: u16,
    pub sequence: u64,
    pub event_id: String,
    pub event_kind: WorkPlanEventKind,
    pub occurred_at_unix_ms: u64,
    pub actor: String,
    pub plan_id: String,
    pub plan_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_item_id: Option<String>,
    pub correlation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    pub payload_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_event_sha256: Option<String>,
    pub event_sha256: String,
}

impl WorkPlanDefinition {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != WORK_PLAN_SCHEMA_VERSION {
            return invalid(format!(
                "unsupported work-plan schema version {}; expected {}",
                self.schema_version, WORK_PLAN_SCHEMA_VERSION
            ));
        }
        validate_identifier("plan_id", &self.plan_id)?;
        validate_text("title", &self.title)?;
        validate_text("authority", &self.authority)?;
        validate_text("status_policy", &self.status_policy)?;
        if self.gate.is_empty() || self.gate.len() > MAX_WORK_PLAN_GATES {
            return invalid(format!(
                "work plan must contain 1..={MAX_WORK_PLAN_GATES} gates"
            ));
        }
        if self.item.is_empty() || self.item.len() > MAX_WORK_PLAN_ITEMS {
            return invalid(format!(
                "work plan must contain 1..={MAX_WORK_PLAN_ITEMS} items"
            ));
        }

        let mut gates = BTreeMap::new();
        for gate in &self.gate {
            validate_identifier("gate id", &gate.id)?;
            validate_text("gate title", &gate.title)?;
            validate_sorted_dependencies("gate", &gate.id, &gate.depends_on)?;
            if gates.insert(gate.id.as_str(), &gate.depends_on).is_some() {
                return invalid(format!("duplicate work gate {}", gate.id));
            }
        }
        validate_dependency_graph("gate", &gates)?;

        let gate_ids = gates.keys().copied().collect::<BTreeSet<_>>();
        let mut items = BTreeMap::new();
        for item in &self.item {
            validate_identifier("work item id", &item.id)?;
            validate_text("work item title", &item.title)?;
            if !gate_ids.contains(item.gate.as_str()) {
                return invalid(format!(
                    "work item {} references unknown gate {}",
                    item.id, item.gate
                ));
            }
            validate_sorted_dependencies("work item", &item.id, &item.depends_on)?;
            if item.acceptance.is_empty() {
                return invalid(format!(
                    "work item {} must define acceptance evidence",
                    item.id
                ));
            }
            for acceptance in &item.acceptance {
                validate_text("work item acceptance", acceptance)?;
            }
            if items.insert(item.id.as_str(), &item.depends_on).is_some() {
                return invalid(format!("duplicate work item {}", item.id));
            }
        }
        validate_dependency_graph("work item", &items)
    }

    pub fn sha256(&self) -> Result<String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|error| ContractError(error.to_string()))?;
        Ok(hex_sha256(&bytes))
    }
}

impl WorkPlanEventEnvelope {
    pub fn seal(&mut self) -> Result<()> {
        self.validate_fields()?;
        self.event_sha256.clear();
        let bytes = serde_json::to_vec(self).map_err(|error| ContractError(error.to_string()))?;
        self.event_sha256 = hex_sha256(&bytes);
        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        self.validate_fields()?;
        let mut candidate = self.clone();
        let expected = candidate.event_sha256.clone();
        candidate.seal()?;
        if candidate.event_sha256 != expected {
            return invalid("work-plan event digest mismatch");
        }
        Ok(())
    }

    fn validate_fields(&self) -> Result<()> {
        if self.schema_version != WORK_PLAN_SCHEMA_VERSION || self.sequence == 0 {
            return invalid("invalid work-plan event version or sequence");
        }
        for (name, value) in [
            ("event_id", self.event_id.as_str()),
            ("actor", self.actor.as_str()),
            ("plan_id", self.plan_id.as_str()),
            ("correlation_id", self.correlation_id.as_str()),
            ("payload_sha256", self.payload_sha256.as_str()),
        ] {
            validate_text(name, value)?;
        }
        if self.occurred_at_unix_ms == 0 {
            return invalid("work-plan event time must be non-zero");
        }
        if self.plan_sha256.len() != 64 || self.payload_sha256.len() != 64 {
            return invalid("work-plan event digests must be lowercase SHA-256 hex");
        }
        if !self.plan_sha256.bytes().all(is_lower_hex)
            || !self.payload_sha256.bytes().all(is_lower_hex)
            || self
                .previous_event_sha256
                .as_ref()
                .is_some_and(|value| value.len() != 64 || !value.bytes().all(is_lower_hex))
        {
            return invalid("work-plan event contains an invalid digest");
        }
        Ok(())
    }
}

fn validate_dependency_graph<'a>(
    kind: &str,
    nodes: &BTreeMap<&'a str, &'a Vec<String>>,
) -> Result<()> {
    for (id, dependencies) in nodes {
        for dependency in *dependencies {
            if !nodes.contains_key(dependency.as_str()) {
                return invalid(format!("{kind} {id} depends on unknown {dependency}"));
            }
        }
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for id in nodes.keys().copied() {
        visit(kind, id, nodes, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit<'a>(
    kind: &str,
    id: &'a str,
    nodes: &BTreeMap<&'a str, &'a Vec<String>>,
    visiting: &mut BTreeSet<&'a str>,
    visited: &mut BTreeSet<&'a str>,
) -> Result<()> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id) {
        return invalid(format!("{kind} dependency cycle contains {id}"));
    }
    for dependency in nodes[id] {
        visit(kind, dependency, nodes, visiting, visited)?;
    }
    visiting.remove(id);
    visited.insert(id);
    Ok(())
}

fn validate_sorted_dependencies(kind: &str, id: &str, values: &[String]) -> Result<()> {
    let mut previous: Option<&str> = None;
    for value in values {
        validate_identifier("dependency", value)?;
        if previous.is_some_and(|candidate| candidate >= value.as_str()) {
            return invalid(format!(
                "{kind} {id} dependencies must be sorted and unique"
            ));
        }
        previous = Some(value);
    }
    Ok(())
}

fn validate_identifier(name: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return invalid(format!("invalid work-plan {name}"));
    }
    Ok(())
}

fn validate_text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > MAX_WORK_PLAN_TEXT_BYTES || value.contains('\0') {
        return invalid(format!("invalid work-plan {name}"));
    }
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> WorkPlanDefinition {
        WorkPlanDefinition {
            schema_version: 1,
            plan_id: "foundation".into(),
            title: "Foundation".into(),
            authority: "RRD".into(),
            status_policy: "Evidence only".into(),
            gate: vec![WorkGateDefinition {
                id: "G00".into(),
                title: "Control".into(),
                depends_on: vec![],
            }],
            item: vec![
                WorkItemDefinition {
                    id: "G00-W01".into(),
                    gate: "G00".into(),
                    title: "Contract".into(),
                    depends_on: vec![],
                    acceptance: vec!["reopens".into()],
                },
                WorkItemDefinition {
                    id: "G00-W02".into(),
                    gate: "G00".into(),
                    title: "Events".into(),
                    depends_on: vec!["G00-W01".into()],
                    acceptance: vec!["rejects invalid transitions".into()],
                },
            ],
        }
    }

    #[test]
    fn validates_and_hashes_deterministically() {
        let plan = plan();
        plan.validate().unwrap();
        assert_eq!(plan.sha256().unwrap(), plan.sha256().unwrap());
    }

    #[test]
    fn operation_identities_are_complete_unique_and_stable() {
        let names = WorkPlanOperation::ALL
            .into_iter()
            .map(WorkPlanOperation::runtime_tool_name)
            .collect::<BTreeSet<_>>();
        let capabilities = WorkPlanOperation::ALL
            .into_iter()
            .map(WorkPlanOperation::capability_id)
            .collect::<BTreeSet<_>>();
        let commands = WorkPlanOperation::ALL
            .into_iter()
            .map(WorkPlanOperation::cli_command)
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), WorkPlanOperation::ALL.len());
        assert_eq!(capabilities.len(), WorkPlanOperation::ALL.len());
        assert_eq!(commands.len(), WorkPlanOperation::ALL.len());
        assert!(names.iter().all(|name| name.starts_with("rrflow_work_")));
    }

    #[test]
    fn rejects_unknown_dependencies_cycles_and_unsorted_dependencies() {
        let mut value = plan();
        value.item[1].depends_on = vec!["missing".into()];
        assert!(value.validate().is_err());

        let mut value = plan();
        value.item[0].depends_on = vec!["G00-W02".into()];
        assert!(value.validate().is_err());

        let mut value = plan();
        value.item[1].depends_on = vec!["G00-W01".into(), "G00-W01".into()];
        assert!(value.validate().is_err());
    }

    #[test]
    fn event_digest_detects_tampering() {
        let mut event = WorkPlanEventEnvelope {
            schema_version: 1,
            sequence: 1,
            event_id: "event-1".into(),
            event_kind: WorkPlanEventKind::WorkPlanLoaded,
            occurred_at_unix_ms: 1,
            actor: "test".into(),
            plan_id: "foundation".into(),
            plan_sha256: "a".repeat(64),
            work_item_id: None,
            correlation_id: "request-1".into(),
            causation_id: None,
            payload_sha256: "b".repeat(64),
            previous_event_sha256: None,
            event_sha256: String::new(),
        };
        event.seal().unwrap();
        event.verify().unwrap();
        event.actor = "tampered".into();
        assert!(event.verify().is_err());
    }
}
