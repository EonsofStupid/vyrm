//! Provider-neutral lifecycle state persisted in RRD's shared runtime log.
//!
//! Lifecycle events are authoritative product state, not telemetry. Each
//! transition is replay-validated, sealed by the public contract, and committed
//! atomically with its derived session record through the one RRD engine.

mod aggregate;
mod binding;
mod persistence;
mod state;
mod supervisor;

use aggregate::LifecycleAggregate;
use persistence::{event_mutation, lifecycle_schema_update, load_aggregate_at, session_record};
pub use rrd_contract::{
    LifecycleEnforcementLevelV1, LifecycleEventCommandV1, LifecycleEventEnvelopeV1,
    LifecycleEventTypeV1, LifecyclePayloadV1, LifecyclePhaseV1, LifecycleProjectionRefreshV1,
    LifecycleReadStampV1, LifecycleRiskV1, LifecycleSessionSnapshotV1,
    LifecycleSupervisorContextV1, LifecycleTaskKindV1, LifecycleToolAuthorizationV1,
    LifecycleToolCompletionV1, LifecycleToolRequestV1, LifecycleTraceContextV1,
    LifecycleTurnStatusV1,
};
use rrd_core::{RuntimeCommit, RuntimeMutation, ScopeId};
use rrd_store::Engine;

pub use supervisor::{
    authorize_lifecycle_tool, authorize_planned_lifecycle_tool, complete_lifecycle_tool,
    complete_planned_lifecycle_tool, consume_lifecycle_tool_authorization,
    load_active_lifecycle_tool_authorization, refresh_lifecycle_projection,
};

pub const LIFECYCLE_RUNTIME_EVENT_TYPE: &str = "rrflow_lifecycle_event_v1";
pub const LIFECYCLE_RUNTIME_SESSION_TYPE: &str = "rrflow_lifecycle_session_v1";

pub fn append_lifecycle_event<E: Engine>(
    store: &E,
    command: LifecycleEventCommandV1,
    recorded_at_unix_ms: u64,
) -> Result<LifecycleSessionSnapshotV1, Box<dyn std::error::Error>> {
    command.validate()?;
    if recorded_at_unix_ms < command.occurred_at_unix_ms {
        return Err("lifecycle recording time precedes occurrence time".into());
    }
    let scope = ScopeId::new(command.scope.clone())?;
    let observed_head = store.runtime_cursor()?;
    let mut aggregate = load_aggregate_at(
        store,
        &scope,
        &command.project_id,
        &command.session_id,
        observed_head,
    )?
    .unwrap_or_else(|| LifecycleAggregate::empty(&command.project_id, &command.session_id));
    validate_work_plan_authority(store, &aggregate, &command)?;
    let event = aggregate.seal_next(command, recorded_at_unix_ms)?;
    aggregate.append_replayed(event.clone())?;
    let snapshot = aggregate.snapshot()?;

    let mut mutations = vec![
        event_mutation(&event)?,
        session_record(
            &snapshot,
            aggregate.state.projection_stale,
            recorded_at_unix_ms,
        )?,
    ];
    if let Some(registry) = lifecycle_schema_update(store, &scope)? {
        mutations.insert(0, RuntimeMutation::Schema { registry });
    }
    store.commit_runtime(&RuntimeCommit {
        scope,
        at: recorded_at_unix_ms,
        actor: event.actor.clone(),
        expected_cursor: observed_head,
        mutations,
    })?;
    Ok(snapshot)
}

fn validate_work_plan_authority<E: Engine>(
    store: &E,
    aggregate: &LifecycleAggregate,
    command: &LifecycleEventCommandV1,
) -> Result<(), Box<dyn std::error::Error>> {
    let coordinates = match &command.payload {
        LifecyclePayloadV1::PlanningAuthorized {
            plan_id,
            plan_revision,
            work_item_id,
            ..
        }
        | LifecyclePayloadV1::PlanRecorded {
            plan_id,
            plan_revision,
            work_item_id,
            ..
        } => Some((plan_id.as_str(), *plan_revision, work_item_id.as_str())),
        LifecyclePayloadV1::ToolProposed { mutation: true, .. } => {
            let binding = aggregate
                .state
                .planning_binding()
                .ok_or("mutating tool has no lifecycle planning binding")?;
            let (plan_id, revision, item_id, ..) = binding.authority_coordinates();
            Some((plan_id, revision, item_id))
        }
        _ => None,
    };
    let Some((plan_id, plan_revision, work_item_id)) = coordinates else {
        return Ok(());
    };
    let snapshot = super::workplan::load_work_plan(store, plan_id)?
        .ok_or("lifecycle planning references an uninstalled work plan")?;
    if snapshot.revision != plan_revision
        || snapshot.active_item_id.as_deref() != Some(work_item_id)
    {
        return Err(
            "lifecycle planning does not match the active work-plan revision and item".into(),
        );
    }
    let record = super::workplan::load_active_work_item_plan(store, plan_id)?
        .ok_or("active work item has no recorded implementation plan")?;
    if record.work_item_id != work_item_id {
        return Err("lifecycle planning addresses another work-item plan record".into());
    }

    let verification_plan_sha256 =
        rrd_core::digest::sha256_hex(&serde_json::to_vec(&record.verification_commands)?);
    match &command.payload {
        LifecyclePayloadV1::PlanRecorded {
            plan_sha256,
            source_tree_sha256,
            verification_plan_sha256: event_verification,
            ..
        } => {
            if plan_sha256 != &record.plan_payload_sha256
                || source_tree_sha256 != &record.source_tree_sha256
                || event_verification != &verification_plan_sha256
            {
                return Err(
                    "canonical plan.recorded differs from the active work-plan record".into(),
                );
            }
        }
        LifecyclePayloadV1::ToolProposed { mutation: true, .. } => {
            let binding = aggregate.state.planning_binding().expect("checked above");
            let (_, _, _, plan_sha256, source_tree_sha256, event_verification) =
                binding.authority_coordinates();
            if plan_sha256 != Some(record.plan_payload_sha256.as_str())
                || source_tree_sha256 != Some(record.source_tree_sha256.as_str())
                || event_verification != Some(verification_plan_sha256.as_str())
            {
                return Err(
                    "mutating tool planning evidence is stale or belongs to another work item"
                        .into(),
                );
            }
        }
        _ => {}
    }
    Ok(())
}

/// Rechecks the authoritative work-plan state at the side-effect boundary.
/// A valid proposal cannot be used after its permit expires or after the
/// active plan/revision/evidence changes.
fn validate_active_mutation_authority<E: Engine>(
    store: &E,
    aggregate: &LifecycleAggregate,
    at: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let binding = aggregate
        .state
        .planning_binding()
        .ok_or("mutating tool has no lifecycle planning binding")?;
    binding.require_fresh_plan(at)?;
    let (plan_id, plan_revision, work_item_id, plan_sha256, source_tree_sha256, event_verification) =
        binding.authority_coordinates();
    let snapshot = super::workplan::load_work_plan(store, plan_id)?
        .ok_or("lifecycle planning references an uninstalled work plan")?;
    if snapshot.revision != plan_revision
        || snapshot.active_item_id.as_deref() != Some(work_item_id)
    {
        return Err(
            "lifecycle planning does not match the active work-plan revision and item".into(),
        );
    }
    let record = super::workplan::load_active_work_item_plan(store, plan_id)?
        .ok_or("active work item has no recorded implementation plan")?;
    let verification_plan_sha256 =
        rrd_core::digest::sha256_hex(&serde_json::to_vec(&record.verification_commands)?);
    if record.work_item_id != work_item_id
        || plan_sha256 != Some(record.plan_payload_sha256.as_str())
        || source_tree_sha256 != Some(record.source_tree_sha256.as_str())
        || event_verification != Some(verification_plan_sha256.as_str())
    {
        return Err(
            "mutating tool planning evidence is stale or belongs to another work item".into(),
        );
    }
    Ok(())
}

pub fn load_lifecycle_session<E: Engine>(
    store: &E,
    scope: &str,
    project_id: &str,
    session_id: &str,
) -> Result<Option<LifecycleSessionSnapshotV1>, Box<dyn std::error::Error>> {
    persistence::validate_lookup_identity("project id", project_id)?;
    persistence::validate_lookup_identity("session id", session_id)?;
    let scope = ScopeId::new(scope.to_owned())?;
    let observed_head = store.runtime_cursor()?;
    load_aggregate_at(store, &scope, project_id, session_id, observed_head)?
        .map(|aggregate| aggregate.snapshot())
        .transpose()
}

/// Returns the complete verified event chain. Malformed stored events fail the
/// entire read and are never skipped.
pub fn load_lifecycle_events<E: Engine>(
    store: &E,
    scope: &str,
    project_id: &str,
    session_id: &str,
) -> Result<Vec<LifecycleEventEnvelopeV1>, Box<dyn std::error::Error>> {
    persistence::validate_lookup_identity("project id", project_id)?;
    persistence::validate_lookup_identity("session id", session_id)?;
    let scope = ScopeId::new(scope.to_owned())?;
    let observed_head = store.runtime_cursor()?;
    Ok(
        load_aggregate_at(store, &scope, project_id, session_id, observed_head)?
            .map_or_else(Vec::new, |aggregate| aggregate.events),
    )
}
