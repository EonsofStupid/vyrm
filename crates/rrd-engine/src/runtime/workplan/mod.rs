//! Engine-owned execution of the checked-in RRFlow foundation work plan.

mod model;
mod verification;

pub use model::{
    WorkItemExecutionMode, WorkItemPlanRecord, WorkItemToolAuthorization, WorkItemVerification,
    WorkItemVerificationArtifact, WorkItemVerificationCheck,
};
pub use verification::{record_project_work_item_plan, verify_recorded_work_item};

use model::{validate_plan_record, validate_sha256, validate_verification, PersistedWorkPlan};
pub use rrd_contract::{WorkItemStatus, WorkPlanDefinition, WorkPlanOperation, WorkPlanSnapshot};
use rrd_contract::{WorkPlanEventEnvelope, WorkPlanEventKind, WORK_PLAN_SCHEMA_VERSION};
use rrd_core::digest;
use rrd_store::{ControlTransition, Engine};
use serde::Serialize;
use std::path::Path;

pub const WORK_PLAN_FILE: &str = "rrflow.workplan.toml";

#[derive(Debug, Clone, Copy)]
pub struct WorkPlanEventContext<'a> {
    pub now: u64,
    pub actor: &'a str,
    pub correlation_id: &'a str,
}

pub fn read_work_plan(root: &Path) -> Result<WorkPlanDefinition, Box<dyn std::error::Error>> {
    let path = root.join(WORK_PLAN_FILE);
    let bytes =
        std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))?;
    let definition: WorkPlanDefinition = toml::from_str(text)
        .map_err(|error| format!("{} is not a valid V1 work plan: {error}", path.display()))?;
    definition.validate()?;
    Ok(definition)
}

pub fn sync_project_work_plan<E: Engine>(
    store: &E,
    root: &Path,
    now: u64,
    actor: &str,
) -> Result<Option<WorkPlanSnapshot>, Box<dyn std::error::Error>> {
    if !root.join(WORK_PLAN_FILE).is_file() {
        return Ok(None);
    }
    let definition = read_work_plan(root)?;
    install_work_plan(store, definition, now, actor, "project-workplan-sync").map(Some)
}

pub fn install_work_plan<E: Engine>(
    store: &E,
    definition: WorkPlanDefinition,
    now: u64,
    actor: &str,
    correlation_id: &str,
) -> Result<WorkPlanSnapshot, Box<dyn std::error::Error>> {
    definition.validate()?;
    let plan_sha256 = definition.sha256()?;
    let key = state_key(&definition.plan_id);
    if let Some(bytes) = store.control_record(&key)? {
        let state = decode_state(&bytes)?;
        if state.plan_sha256 != plan_sha256 {
            return Err(
                "work-plan source changed without a reviewed plan-revision transition".into(),
            );
        }
        return Ok(state.snapshot());
    }
    let mut state = PersistedWorkPlan::new(definition, plan_sha256);
    let loaded_payload = state.plan_sha256.clone();
    append_event(
        &mut state,
        WorkPlanEventKind::WorkPlanLoaded,
        None,
        &loaded_payload,
        now,
        actor,
        correlation_id,
    )?;
    commit_state(store, key, None, &state, now, actor, "workplan.loaded")?;
    Ok(state.snapshot())
}

pub fn load_work_plan<E: Engine>(
    store: &E,
    plan_id: &str,
) -> Result<Option<WorkPlanSnapshot>, Box<dyn std::error::Error>> {
    store
        .control_record(&state_key(plan_id))?
        .map(|bytes| Ok(decode_state(&bytes)?.snapshot()))
        .transpose()
}

pub fn load_active_work_item_plan<E: Engine>(
    store: &E,
    plan_id: &str,
) -> Result<Option<WorkItemPlanRecord>, Box<dyn std::error::Error>> {
    let Some(bytes) = store.control_record(&state_key(plan_id))? else {
        return Ok(None);
    };
    let state = decode_state(&bytes)?;
    Ok(state.plan_record)
}

pub fn load_active_work_item_authorization<E: Engine>(
    store: &E,
    plan_id: &str,
) -> Result<Option<WorkItemToolAuthorization>, Box<dyn std::error::Error>> {
    let Some(bytes) = store.control_record(&state_key(plan_id))? else {
        return Ok(None);
    };
    let state = decode_state(&bytes)?;
    Ok(state.authorization)
}

pub fn load_work_item_verification<E: Engine>(
    store: &E,
    plan_id: &str,
    item_id: &str,
) -> Result<Option<WorkItemVerification>, Box<dyn std::error::Error>> {
    let Some(bytes) = store.control_record(&state_key(plan_id))? else {
        return Ok(None);
    };
    let state = decode_state(&bytes)?;
    Ok(state
        .items
        .get(item_id)
        .ok_or_else(|| format!("unknown work item {item_id}"))?
        .verification
        .clone())
}

pub fn activate_work_item<E: Engine>(
    store: &E,
    plan_id: &str,
    item_id: &str,
    now: u64,
    actor: &str,
    correlation_id: &str,
) -> Result<WorkPlanSnapshot, Box<dyn std::error::Error>> {
    mutate_state(store, plan_id, now, actor, "workitem.activated", |state| {
        if state.active_item_id.as_deref() == Some(item_id) {
            return Ok(false);
        }
        if state.active_item_id.is_some() {
            return Err("another work item is active; verify it before advancing".into());
        }
        let definition = state
            .definition
            .item
            .iter()
            .find(|candidate| candidate.id == item_id)
            .ok_or_else(|| format!("unknown work item {item_id}"))?;
        for dependency in &definition.depends_on {
            if state.items[dependency].status != WorkItemStatus::Verified {
                return Err(format!(
                    "work item {item_id} is blocked by unverified dependency {dependency}"
                )
                .into());
            }
        }
        let item = state
            .items
            .get_mut(item_id)
            .expect("definition and state agree");
        if item.status == WorkItemStatus::Verified {
            return Err(format!("work item {item_id} is already verified").into());
        }
        item.status = WorkItemStatus::Active;
        state.active_item_id = Some(item_id.to_owned());
        state.plan_record = None;
        state.authorization = None;
        append_event(
            state,
            WorkPlanEventKind::WorkItemActivated,
            Some(item_id),
            &item_id,
            now,
            actor,
            correlation_id,
        )?;
        Ok(true)
    })
}

pub fn record_work_item_plan<E: Engine>(
    store: &E,
    plan_id: &str,
    record: WorkItemPlanRecord,
    now: u64,
    actor: &str,
    correlation_id: &str,
) -> Result<WorkPlanSnapshot, Box<dyn std::error::Error>> {
    validate_plan_record(&record)?;
    mutate_state(store, plan_id, now, actor, "plan.recorded", |state| {
        require_active(state, &record.work_item_id, WorkItemStatus::Active)?;
        if let Some(existing) = &state.plan_record {
            if existing == &record {
                return Ok(false);
            }
            if state
                .authorization
                .as_ref()
                .is_some_and(|authorization| !authorization.consumed)
            {
                return Err(
                    "active work item cannot replace its plan while a tool is authorized".into(),
                );
            }
        }
        let payload = record.clone();
        state.plan_record = Some(record);
        state.authorization = None;
        append_event(
            state,
            WorkPlanEventKind::PlanRecorded,
            Some(&payload.work_item_id),
            &payload,
            now,
            actor,
            correlation_id,
        )?;
        Ok(true)
    })
}

pub fn authorize_work_item_tool<E: Engine>(
    store: &E,
    plan_id: &str,
    item_id: &str,
    source_tree_sha256: &str,
    attunement_receipt_sha256: &str,
    tool_request_sha256: &str,
    context: WorkPlanEventContext<'_>,
) -> Result<WorkItemToolAuthorization, Box<dyn std::error::Error>> {
    validate_sha256("source tree digest", source_tree_sha256)?;
    validate_sha256("attunement receipt digest", attunement_receipt_sha256)?;
    validate_sha256("tool request digest", tool_request_sha256)?;
    let mut result = None;
    mutate_state(
        store,
        plan_id,
        context.now,
        context.actor,
        "tool.authorized",
        |state| {
            require_active(state, item_id, WorkItemStatus::Active)?;
            let plan = state
                .plan_record
                .as_ref()
                .ok_or("mutation denied: active work item has no recorded plan")?;
            if plan.execution_mode != WorkItemExecutionMode::Change {
                return Err("mutation denied: active work item is qualification-only".into());
            }
            let expected_source_tree = state
                .authorization
                .as_ref()
                .filter(|authorization| authorization.consumed)
                .and_then(|authorization| authorization.result_source_tree_sha256.as_deref())
                .unwrap_or(plan.source_tree_sha256.as_str());
            if expected_source_tree != source_tree_sha256 {
                return Err(
                    "mutation denied: source tree does not continue the observed work-item chain"
                        .into(),
                );
            }
            if state.authorization.is_none()
                && plan.attunement_receipt_sha256 != attunement_receipt_sha256
            {
                return Err("mutation denied: initial plan attunement evidence is stale".into());
            }
            let authorization = WorkItemToolAuthorization {
                work_item_id: item_id.to_owned(),
                plan_payload_sha256: plan.plan_payload_sha256.clone(),
                authorized_source_tree_sha256: source_tree_sha256.to_owned(),
                attunement_receipt_sha256: attunement_receipt_sha256.to_owned(),
                tool_request_sha256: tool_request_sha256.to_owned(),
                consumed: false,
                observation_sha256: None,
                result_source_tree_sha256: None,
            };
            if let Some(existing) = &state.authorization {
                if existing == &authorization {
                    result = Some(existing.clone());
                    return Ok(false);
                }
                if !existing.consumed {
                    return Err("mutation denied: another tool authorization is outstanding".into());
                }
            }
            append_event(
                state,
                WorkPlanEventKind::ToolProposed,
                Some(item_id),
                &authorization,
                context.now,
                context.actor,
                context.correlation_id,
            )?;
            append_event(
                state,
                WorkPlanEventKind::ToolAuthorized,
                Some(item_id),
                &authorization,
                context.now,
                context.actor,
                context.correlation_id,
            )?;
            state.authorization = Some(authorization.clone());
            result = Some(authorization);
            Ok(true)
        },
    )?;
    result.ok_or_else(|| "tool authorization did not produce a result".into())
}

pub fn complete_work_item_tool<E: Engine>(
    store: &E,
    plan_id: &str,
    item_id: &str,
    tool_request_sha256: &str,
    observation_sha256: &str,
    result_source_tree_sha256: Option<&str>,
    context: WorkPlanEventContext<'_>,
) -> Result<WorkPlanSnapshot, Box<dyn std::error::Error>> {
    validate_sha256("tool request digest", tool_request_sha256)?;
    validate_sha256("tool observation digest", observation_sha256)?;
    if let Some(result_tree) = result_source_tree_sha256 {
        validate_sha256("result source tree digest", result_tree)?;
    }
    mutate_state(
        store,
        plan_id,
        context.now,
        context.actor,
        "tool.completed",
        |state| {
            require_active(state, item_id, WorkItemStatus::Active)?;
            let authorization = state
                .authorization
                .as_mut()
                .ok_or("tool completion has no outstanding authorization")?;
            if authorization.work_item_id != item_id
                || authorization.tool_request_sha256 != tool_request_sha256
            {
                return Err("tool completion does not match the authorized request".into());
            }
            if authorization.consumed {
                if authorization.observation_sha256.as_deref() != Some(observation_sha256) {
                    return Err("tool completion replay carries another observation".into());
                }
                return match (
                    authorization.result_source_tree_sha256.as_deref(),
                    result_source_tree_sha256,
                ) {
                    (Some(existing), Some(replayed)) if existing == replayed => Ok(false),
                    (None, None) | (Some(_), None) => Ok(false),
                    (None, Some(observed)) => {
                        authorization.result_source_tree_sha256 = Some(observed.to_owned());
                        append_event(
                            state,
                            WorkPlanEventKind::ToolCompleted,
                            Some(item_id),
                            &(observation_sha256, observed),
                            context.now,
                            context.actor,
                            context.correlation_id,
                        )?;
                        Ok(true)
                    }
                    (Some(_), Some(_)) => {
                        Err("tool completion replay carries another result source tree".into())
                    }
                };
            }
            authorization.consumed = true;
            authorization.observation_sha256 = Some(observation_sha256.to_owned());
            authorization.result_source_tree_sha256 = result_source_tree_sha256.map(str::to_owned);
            append_event(
                state,
                WorkPlanEventKind::ToolCompleted,
                Some(item_id),
                &(observation_sha256, result_source_tree_sha256),
                context.now,
                context.actor,
                context.correlation_id,
            )?;
            Ok(true)
        },
    )
}

pub fn verify_work_item<E: Engine>(
    store: &E,
    plan_id: &str,
    verification: WorkItemVerification,
    now: u64,
    actor: &str,
    correlation_id: &str,
) -> Result<WorkPlanSnapshot, Box<dyn std::error::Error>> {
    validate_verification(&verification)?;
    mutate_state(store, plan_id, now, actor, "workitem.verified", |state| {
        require_active(state, &verification.work_item_id, WorkItemStatus::Active)?;
        let plan = state
            .plan_record
            .as_ref()
            .ok_or("verification denied: active item has no recorded plan")?;
        if plan.plan_payload_sha256 != verification.plan_payload_sha256 {
            return Err("verification denied: evidence belongs to another plan".into());
        }
        let observed_commands = verification
            .checks
            .iter()
            .map(|check| check.argv.clone())
            .collect::<Vec<_>>();
        if observed_commands != plan.verification_commands {
            return Err(
                "verification denied: evidence does not match the exact recorded commands".into(),
            );
        }
        match plan.execution_mode {
            WorkItemExecutionMode::Change => {
                let authorization = state
                    .authorization
                    .as_ref()
                    .filter(|authorization| authorization.consumed)
                    .ok_or("verification denied: mutation authorization was not consumed")?;
                if authorization.result_source_tree_sha256.as_deref()
                    != Some(verification.source_tree_sha256.as_str())
                    || authorization.plan_payload_sha256 != verification.plan_payload_sha256
                {
                    return Err(
                        "verification denied: evidence belongs to another mutation result".into(),
                    );
                }
            }
            WorkItemExecutionMode::Qualification => {
                if state.authorization.is_some()
                    || plan.source_tree_sha256 != verification.source_tree_sha256
                {
                    return Err(
                        "verification denied: qualification changed the recorded source tree"
                            .into(),
                    );
                }
            }
        }
        let verification_sha256 = digest::sha256_hex(&serde_json::to_vec(&verification)?);
        append_event(
            state,
            WorkPlanEventKind::VerificationCompleted,
            Some(&verification.work_item_id),
            &verification,
            now,
            actor,
            correlation_id,
        )?;
        append_event(
            state,
            WorkPlanEventKind::WorkItemVerified,
            Some(&verification.work_item_id),
            &verification_sha256,
            now,
            actor,
            correlation_id,
        )?;
        let item = state
            .items
            .get_mut(&verification.work_item_id)
            .expect("active item exists");
        item.status = WorkItemStatus::Verified;
        item.verification_sha256 = Some(verification_sha256);
        item.verification = Some(verification);
        state.active_item_id = None;
        state.plan_record = None;
        state.authorization = None;
        Ok(true)
    })
}

fn mutate_state<E: Engine, F>(
    store: &E,
    plan_id: &str,
    now: u64,
    actor: &str,
    action: &str,
    mutation: F,
) -> Result<WorkPlanSnapshot, Box<dyn std::error::Error>>
where
    F: FnOnce(&mut PersistedWorkPlan) -> Result<bool, Box<dyn std::error::Error>>,
{
    let key = state_key(plan_id);
    let expected = store
        .control_record(&key)?
        .ok_or_else(|| format!("work plan {plan_id} is not installed"))?;
    let mut state = decode_state(&expected)?;
    if mutation(&mut state)? {
        state.validate()?;
        commit_state(store, key, Some(expected), &state, now, actor, action)?;
    }
    Ok(state.snapshot())
}

fn require_active(
    state: &PersistedWorkPlan,
    item_id: &str,
    expected_status: WorkItemStatus,
) -> Result<(), Box<dyn std::error::Error>> {
    if state.active_item_id.as_deref() != Some(item_id)
        || state.items.get(item_id).map(|item| item.status) != Some(expected_status)
    {
        return Err(format!(
            "work item {item_id} is not the active {:?} item",
            expected_status
        )
        .into());
    }
    Ok(())
}

fn append_event<T: Serialize>(
    state: &mut PersistedWorkPlan,
    event_kind: WorkPlanEventKind,
    work_item_id: Option<&str>,
    payload: &T,
    now: u64,
    actor: &str,
    correlation_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let sequence = state.events.len() as u64 + 1;
    let payload_sha256 = digest::sha256_hex(&serde_json::to_vec(payload)?);
    let previous = state.events.last();
    let mut event = WorkPlanEventEnvelope {
        schema_version: WORK_PLAN_SCHEMA_VERSION,
        sequence,
        event_id: format!("workplan-{sequence}-{}", &payload_sha256[..16]),
        event_kind,
        occurred_at_unix_ms: now,
        actor: actor.to_owned(),
        plan_id: state.definition.plan_id.clone(),
        plan_sha256: state.plan_sha256.clone(),
        work_item_id: work_item_id.map(str::to_owned),
        correlation_id: correlation_id.to_owned(),
        causation_id: previous.map(|event| event.event_id.clone()),
        payload_sha256,
        previous_event_sha256: previous.map(|event| event.event_sha256.clone()),
        event_sha256: String::new(),
    };
    event.seal()?;
    state.events.push(event);
    Ok(())
}

fn decode_state(bytes: &[u8]) -> Result<PersistedWorkPlan, Box<dyn std::error::Error>> {
    let state: PersistedWorkPlan = serde_json::from_slice(bytes)
        .map_err(|error| format!("persisted work-plan state is unreadable: {error}"))?;
    state.validate()?;
    Ok(state)
}

fn commit_state<E: Engine>(
    store: &E,
    key: String,
    expected: Option<Vec<u8>>,
    state: &PersistedWorkPlan,
    now: u64,
    actor: &str,
    action: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    state.validate()?;
    let replacement = serde_json::to_vec(state)?;
    let operation = digest::sha256_hex(&replacement);
    store.commit_control_transition(&ControlTransition {
        key,
        expected,
        replacement: Some(replacement),
        at: now,
        actor: actor.to_owned(),
        action: action.to_owned(),
        request_id: format!("workplan-{}", &operation[..16]),
        operation_id: operation,
    })?;
    Ok(())
}

fn state_key(plan_id: &str) -> String {
    format!("server/state/runtime/workplan/{plan_id}")
}

#[cfg(test)]
mod tests;
