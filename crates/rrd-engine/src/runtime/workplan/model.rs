use rrd_contract::{
    WorkItemStatus, WorkItemStatusSnapshot, WorkPlanDefinition, WorkPlanEventEnvelope,
    WorkPlanSnapshot, WORK_PLAN_SCHEMA_VERSION,
};
use rrd_core::digest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const STATE_FORMAT: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PersistedWorkPlan {
    pub format: u16,
    pub definition: WorkPlanDefinition,
    pub plan_sha256: String,
    pub revision: u64,
    pub active_item_id: Option<String>,
    pub items: BTreeMap<String, PersistedWorkItem>,
    pub plan_record: Option<WorkItemPlanRecord>,
    pub authorization: Option<WorkItemToolAuthorization>,
    pub events: Vec<WorkPlanEventEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PersistedWorkItem {
    pub status: WorkItemStatus,
    pub verification_sha256: Option<String>,
    pub verification: Option<WorkItemVerification>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemExecutionMode {
    /// The recorded plan is expected to execute one or more supervised
    /// mutations before verification.
    Change,
    /// The current tree already contains the reviewed implementation and this
    /// work item only qualifies it. Mutation is forbidden while this record is
    /// active and verification requires the tree to remain unchanged.
    Qualification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItemPlanRecord {
    pub work_item_id: String,
    pub execution_mode: WorkItemExecutionMode,
    pub source_tree_sha256: String,
    pub attunement_receipt_sha256: String,
    pub plan_payload_sha256: String,
    pub verification_commands: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItemToolAuthorization {
    pub work_item_id: String,
    pub plan_payload_sha256: String,
    pub authorized_source_tree_sha256: String,
    pub attunement_receipt_sha256: String,
    pub tool_request_sha256: String,
    pub consumed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_source_tree_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItemVerification {
    pub work_item_id: String,
    pub source_tree_sha256: String,
    pub plan_payload_sha256: String,
    pub repository_revision: String,
    pub platform: String,
    pub checks: Vec<WorkItemVerificationCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItemVerificationCheck {
    pub name: String,
    pub argv: Vec<String>,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub stdout: WorkItemVerificationArtifact,
    pub stderr: WorkItemVerificationArtifact,
    pub evidence_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItemVerificationArtifact {
    pub byte_count: u64,
    pub sha256: String,
}

impl WorkItemVerificationCheck {
    pub fn seal_evidence(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.evidence_sha256.clear();
        self.evidence_sha256 = verification_check_sha256(self)?;
        Ok(())
    }

    pub fn verify_evidence(&self) -> Result<(), Box<dyn std::error::Error>> {
        validate_sha256("verification evidence digest", &self.evidence_sha256)?;
        if verification_check_sha256(self)? != self.evidence_sha256 {
            return Err("verification check evidence digest mismatch".into());
        }
        Ok(())
    }
}

impl PersistedWorkPlan {
    pub fn new(definition: WorkPlanDefinition, plan_sha256: String) -> Self {
        let items = definition
            .item
            .iter()
            .map(|item| {
                (
                    item.id.clone(),
                    PersistedWorkItem {
                        status: WorkItemStatus::Pending,
                        verification_sha256: None,
                        verification: None,
                    },
                )
            })
            .collect();
        Self {
            format: STATE_FORMAT,
            definition,
            plan_sha256,
            revision: 1,
            active_item_id: None,
            items,
            plan_record: None,
            authorization: None,
            events: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.format != STATE_FORMAT || self.revision == 0 {
            return Err("invalid persisted work-plan state version or revision".into());
        }
        self.definition.validate()?;
        if self.definition.sha256()? != self.plan_sha256 {
            return Err("persisted work-plan definition digest mismatch".into());
        }
        let expected = self
            .definition
            .item
            .iter()
            .map(|item| item.id.as_str())
            .collect::<BTreeSet<_>>();
        let actual = self
            .items
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if expected != actual {
            return Err("persisted work-plan item state does not match its definition".into());
        }
        for (item_id, item) in &self.items {
            match (item.status, &item.verification_sha256, &item.verification) {
                (WorkItemStatus::Verified, Some(expected_sha256), Some(verification)) => {
                    validate_verification(verification)?;
                    if verification.work_item_id != *item_id
                        || digest::sha256_hex(&serde_json::to_vec(verification)?)
                            != *expected_sha256
                    {
                        return Err(
                            "persisted work-item verification evidence is inconsistent".into()
                        );
                    }
                }
                (WorkItemStatus::Verified, _, _) => {
                    return Err("verified work item is missing its exact evidence".into());
                }
                (_, None, None) => {}
                _ => {
                    return Err("unverified work item cannot carry verification evidence".into());
                }
            }
        }
        let active = self
            .items
            .iter()
            .filter(|(_, item)| {
                item.status == WorkItemStatus::Active || item.status == WorkItemStatus::Verifying
            })
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>();
        if active.len() > 1 || active.first().copied() != self.active_item_id.as_deref() {
            return Err("persisted work-plan active-item state is inconsistent".into());
        }
        if self.plan_record.as_ref().is_some_and(|record| {
            self.active_item_id.as_deref() != Some(record.work_item_id.as_str())
        }) || self.authorization.as_ref().is_some_and(|authorization| {
            self.active_item_id.as_deref() != Some(authorization.work_item_id.as_str())
        }) {
            return Err("work-plan binding does not belong to the active item".into());
        }
        if let Some(authorization) = &self.authorization {
            validate_sha256(
                "authorized source tree digest",
                &authorization.authorized_source_tree_sha256,
            )?;
            validate_sha256(
                "authorization attunement receipt digest",
                &authorization.attunement_receipt_sha256,
            )?;
            validate_sha256(
                "authorization tool request digest",
                &authorization.tool_request_sha256,
            )?;
            validate_sha256(
                "authorization plan payload digest",
                &authorization.plan_payload_sha256,
            )?;
            if let Some(observation) = &authorization.observation_sha256 {
                validate_sha256("tool observation digest", observation)?;
            }
            if let Some(result_tree) = &authorization.result_source_tree_sha256 {
                validate_sha256("result source tree digest", result_tree)?;
            }
            if authorization.consumed != authorization.observation_sha256.is_some()
                || (!authorization.consumed && authorization.result_source_tree_sha256.is_some())
            {
                return Err("work-plan authorization consumption evidence is inconsistent".into());
            }
        }
        let mut previous_digest: Option<&str> = None;
        let mut previous_id: Option<&str> = None;
        for (index, event) in self.events.iter().enumerate() {
            event.verify()?;
            if event.sequence != index as u64 + 1
                || event.plan_id != self.definition.plan_id
                || event.plan_sha256 != self.plan_sha256
                || event.previous_event_sha256.as_deref() != previous_digest
                || event.causation_id.as_deref() != previous_id
            {
                return Err("persisted work-plan event chain is inconsistent".into());
            }
            previous_digest = Some(&event.event_sha256);
            previous_id = Some(&event.event_id);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> WorkPlanSnapshot {
        WorkPlanSnapshot {
            schema_version: WORK_PLAN_SCHEMA_VERSION,
            plan_id: self.definition.plan_id.clone(),
            plan_sha256: self.plan_sha256.clone(),
            revision: self.revision,
            active_item_id: self.active_item_id.clone(),
            items: self
                .items
                .iter()
                .map(|(item_id, state)| WorkItemStatusSnapshot {
                    item_id: item_id.clone(),
                    status: state.status,
                    verification_sha256: state.verification_sha256.clone(),
                })
                .collect(),
            event_sequence: self.events.len() as u64,
            last_event_sha256: self.events.last().map(|event| event.event_sha256.clone()),
        }
    }
}

pub(super) fn validate_sha256(name: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(format!("{name} must be lowercase SHA-256 hex").into());
    }
    Ok(())
}

pub(super) fn validate_plan_record(
    record: &WorkItemPlanRecord,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_sha256("source tree digest", &record.source_tree_sha256)?;
    validate_sha256(
        "attunement receipt digest",
        &record.attunement_receipt_sha256,
    )?;
    validate_sha256("plan payload digest", &record.plan_payload_sha256)?;
    if record.verification_commands.is_empty()
        || record.verification_commands.iter().any(|argv| {
            argv.is_empty()
                || argv
                    .iter()
                    .any(|argument| argument.is_empty() || argument.contains('\0'))
        })
    {
        return Err("work-item plan requires exact non-empty verification argv vectors".into());
    }
    Ok(())
}

pub(super) fn validate_verification(
    verification: &WorkItemVerification,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_sha256(
        "verification source tree digest",
        &verification.source_tree_sha256,
    )?;
    validate_sha256(
        "verification plan digest",
        &verification.plan_payload_sha256,
    )?;
    if verification.repository_revision.trim().is_empty()
        || verification.platform.trim().is_empty()
        || verification.checks.is_empty()
    {
        return Err("verification requires revision, platform, and checks".into());
    }
    for check in &verification.checks {
        if check.name.trim().is_empty()
            || check.argv.is_empty()
            || check
                .argv
                .iter()
                .any(|argument| argument.is_empty() || argument.contains('\0'))
        {
            return Err("verification checks require a name and exact argv".into());
        }
        validate_sha256("verification stdout digest", &check.stdout.sha256)?;
        validate_sha256("verification stderr digest", &check.stderr.sha256)?;
        check.verify_evidence()?;
        if !check.passed {
            return Err(format!("verification check {} did not pass", check.name).into());
        }
        if check.exit_code != Some(0) {
            return Err(format!(
                "verification check {} passed without exit code zero",
                check.name
            )
            .into());
        }
    }
    if !verification
        .repository_revision
        .ends_with(&format!(":{}", verification.source_tree_sha256))
    {
        return Err("verification revision is not bound to its source tree".into());
    }
    Ok(())
}

fn verification_check_sha256(
    check: &WorkItemVerificationCheck,
) -> Result<String, Box<dyn std::error::Error>> {
    #[derive(Serialize)]
    struct Evidence<'a> {
        name: &'a str,
        argv: &'a [String],
        passed: bool,
        exit_code: Option<i32>,
        stdout: &'a WorkItemVerificationArtifact,
        stderr: &'a WorkItemVerificationArtifact,
    }

    Ok(digest::sha256_hex(&serde_json::to_vec(&Evidence {
        name: &check.name,
        argv: &check.argv,
        passed: check.passed,
        exit_code: check.exit_code,
        stdout: &check.stdout,
        stderr: &check.stderr,
    })?))
}
