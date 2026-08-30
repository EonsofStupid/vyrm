//! Generated work-plan runtime tools over the engine-owned state machine.

use super::{
    typed_tool, ExecutedTool, RuntimeToolAuthorization, RuntimeToolDefinition,
    RuntimeToolLifecyclePolicy,
};
use crate::runtime::{
    activate_work_item, install_work_plan, load_work_plan, read_work_plan,
    record_project_work_item_plan, verify_recorded_work_item, WorkItemExecutionMode,
};
use crate::RrdEngine;
use rrd_contract::WorkPlanOperation;
use rrd_core::digest;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

const DEFAULT_PLAN_ID: &str = "rrflow-foundation";

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SyncArguments {
    at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StatusArguments {
    plan_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ActivateArguments {
    plan_id: Option<String>,
    item_id: String,
    at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RecordArguments {
    plan_id: Option<String>,
    item_id: String,
    /// Reviewed implementation plan. RRD persists and binds its digest.
    plan_payload: String,
    #[serde(default)]
    qualification_only: bool,
    verification_commands: Vec<Vec<String>>,
    at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VerifyArguments {
    plan_id: Option<String>,
    at: Option<u64>,
}

pub(super) fn operation(name: &str) -> Option<WorkPlanOperation> {
    WorkPlanOperation::ALL
        .into_iter()
        .find(|operation| operation.runtime_tool_name() == name)
}

pub(super) fn definitions() -> Vec<RuntimeToolDefinition> {
    WorkPlanOperation::ALL
        .into_iter()
        .map(|operation| match operation {
            WorkPlanOperation::Sync => typed_tool::<SyncArguments>(
                operation.runtime_tool_name(),
                Some(operation.capability_id()),
                "Validate the checked-in work plan and install its immutable revision in RRD",
                true,
                RuntimeToolAuthorization::Governed,
                RuntimeToolLifecyclePolicy::ControlTransition,
            ),
            WorkPlanOperation::Status => typed_tool::<StatusArguments>(
                operation.runtime_tool_name(),
                Some(operation.capability_id()),
                "Read the authoritative persisted work-plan state and evidence coordinates",
                false,
                RuntimeToolAuthorization::Governed,
                RuntimeToolLifecyclePolicy::ReadOnly,
            ),
            WorkPlanOperation::Activate => typed_tool::<ActivateArguments>(
                operation.runtime_tool_name(),
                Some(operation.capability_id()),
                "Activate one dependency-ready work item while refusing parallel active work",
                true,
                RuntimeToolAuthorization::Governed,
                RuntimeToolLifecyclePolicy::ControlTransition,
            ),
            WorkPlanOperation::Record => typed_tool::<RecordArguments>(
                operation.runtime_tool_name(),
                Some(operation.capability_id()),
                "Bind the active item to fresh attunement, a reviewed plan, and exact verification argv",
                true,
                RuntimeToolAuthorization::Governed,
                RuntimeToolLifecyclePolicy::ControlTransition,
            ),
            WorkPlanOperation::Verify => typed_tool::<VerifyArguments>(
                operation.runtime_tool_name(),
                Some(operation.capability_id()),
                "Execute RRD-recorded verification argv and verify only from fresh passing engine evidence",
                true,
                RuntimeToolAuthorization::Governed,
                RuntimeToolLifecyclePolicy::VerificationExecution,
            ),
        })
        .collect()
}

pub(super) fn execute(
    engine: &RrdEngine,
    root: &Path,
    name: &str,
    arguments: &Value,
    invocation_at: u64,
) -> Result<ExecutedTool, Box<dyn std::error::Error>> {
    let operation = operation(name).ok_or("unknown work-plan runtime operation")?;
    let actor = "agent:runtime-workplan";
    let encoded = serde_json::to_vec(arguments)?;
    let correlation = format!(
        "runtime-{}-{}",
        operation.capability_id(),
        &digest::sha256_hex(&encoded)[..16]
    );
    let snapshot = match operation {
        WorkPlanOperation::Sync => {
            let request: SyncArguments = serde_json::from_value(arguments.clone())?;
            let definition = read_work_plan(root)?;
            install_work_plan(
                &engine.storage,
                definition,
                request.at.unwrap_or(invocation_at),
                actor,
                &correlation,
            )?
        }
        WorkPlanOperation::Status => {
            let request: StatusArguments = serde_json::from_value(arguments.clone())?;
            let plan_id = request.plan_id.as_deref().unwrap_or(DEFAULT_PLAN_ID);
            load_work_plan(&engine.storage, plan_id)?
                .ok_or_else(|| format!("work plan {plan_id} is not installed"))?
        }
        WorkPlanOperation::Activate => {
            let request: ActivateArguments = serde_json::from_value(arguments.clone())?;
            activate_work_item(
                &engine.storage,
                request.plan_id.as_deref().unwrap_or(DEFAULT_PLAN_ID),
                &request.item_id,
                request.at.unwrap_or(invocation_at),
                actor,
                &correlation,
            )?
        }
        WorkPlanOperation::Record => {
            let request: RecordArguments = serde_json::from_value(arguments.clone())?;
            record_project_work_item_plan(
                &engine.storage,
                root,
                request.plan_id.as_deref().unwrap_or(DEFAULT_PLAN_ID),
                &request.item_id,
                if request.qualification_only {
                    WorkItemExecutionMode::Qualification
                } else {
                    WorkItemExecutionMode::Change
                },
                request.plan_payload.as_bytes(),
                request.verification_commands,
                request.at.unwrap_or(invocation_at),
                actor,
                &correlation,
            )?
        }
        WorkPlanOperation::Verify => {
            let request: VerifyArguments = serde_json::from_value(arguments.clone())?;
            verify_recorded_work_item(
                &engine.storage,
                root,
                request.plan_id.as_deref().unwrap_or(DEFAULT_PLAN_ID),
                request.at.unwrap_or(invocation_at),
                actor,
                &correlation,
            )?
        }
    };
    Ok(ExecutedTool {
        detail: Some(format!(
            "work plan {} revision {} event {}",
            snapshot.plan_id, snapshot.revision, snapshot.event_sequence
        )),
        text: serde_json::to_string_pretty(&snapshot)?,
        effectiveness: None,
    })
}
