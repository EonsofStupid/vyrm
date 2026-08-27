use super::persistence::load_aggregate_at;
use super::{append_lifecycle_event, validate_active_mutation_authority, LifecycleAggregate};
use rrd_contract::{
    LifecycleEventCommandV1, LifecycleEventTypeV1, LifecyclePayloadV1, LifecyclePhaseV1,
    LifecycleReadStampV1, LifecycleSupervisorContextV1, LifecycleToolAuthorizationV1,
    LifecycleToolCompletionV1, LifecycleToolRequestV1, LifecycleTraceContextV1,
};
use rrd_core::{digest, ScopeId};
use rrd_store::Engine;

/// Proposes and authorizes one exact tool request through the canonical RRD
/// lifecycle. The active work-plan binding is validated by
/// `append_lifecycle_event`; this supervisor is the adapter-independent seam
/// that prevents each provider from implementing its own policy state.
pub fn authorize_lifecycle_tool<E: Engine>(
    store: &E,
    context: &LifecycleSupervisorContextV1,
    request: &LifecycleToolRequestV1,
    now: u64,
) -> Result<LifecycleToolAuthorizationV1, Box<dyn std::error::Error>> {
    context.validate()?;
    request.validate()?;
    let scope = ScopeId::new(context.scope.clone())?;
    let aggregate = load_current(store, &scope, context)?;
    if let Some(active) = aggregate.state.active_tool.as_ref() {
        require_same_request(active, request)?;
        return match aggregate.state.phase {
            Some(LifecyclePhaseV1::ToolProposed) => {
                authorize_proposal(store, context, &aggregate, request, now)
            }
            Some(LifecyclePhaseV1::ToolAuthorized) => active_authorization(active),
            Some(LifecyclePhaseV1::ToolStarted) => {
                Err("tool authorization was already consumed before execution".into())
            }
            _ => Err("active lifecycle tool is not in an authorizable phase".into()),
        };
    }
    if aggregate
        .events
        .iter()
        .any(|event| event.tool_call_id.as_deref() == Some(request.tool_call_id.as_str()))
    {
        return Err("tool-call identity already reached a terminal lifecycle state".into());
    }

    let turn_id = aggregate
        .state
        .active_turn_id
        .clone()
        .ok_or("mutation has no active lifecycle turn")?;
    let reasoning_run_id = aggregate
        .state
        .active_reasoning_run_id
        .clone()
        .ok_or("mutation has no lifecycle-bound reasoning run")?;
    let attempt_id = attempt_id(&aggregate, request)?;
    let proposal = command(
        store,
        context,
        &aggregate,
        LifecycleEventTypeV1::ToolProposed,
        LifecyclePayloadV1::ToolProposed {
            tool_name: request.tool_name.clone(),
            tool_request_sha256: request.tool_request_sha256.clone(),
            mutation: request.mutation,
        },
        now,
        Some(turn_id),
        Some(reasoning_run_id),
        Some(attempt_id),
        Some(request.tool_call_id.clone()),
    )?;
    append_lifecycle_event(store, proposal, now)?;

    // Reload after the proposal so the decision is causally and cryptographically
    // bound to the exact state that will be authorized.
    let aggregate = load_current(store, &scope, context)?;
    authorize_proposal(store, context, &aggregate, request, now)
}

/// Consumes one canonical authorization immediately before the side effect.
/// A crash after this transition leaves the attempt deliberately stuck; it
/// can never start the same request twice.
pub fn consume_lifecycle_tool_authorization<E: Engine>(
    store: &E,
    context: &LifecycleSupervisorContextV1,
    authorization: &LifecycleToolAuthorizationV1,
    now: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    context.validate()?;
    authorization.validate()?;
    let scope = ScopeId::new(context.scope.clone())?;
    let aggregate = load_current(store, &scope, context)?;
    let active = aggregate
        .state
        .active_tool
        .as_ref()
        .ok_or("tool execution has no canonical lifecycle authorization")?;
    require_same_authorization(active, authorization)?;
    if aggregate.state.phase == Some(LifecyclePhaseV1::ToolStarted) || active.consumed {
        return Err("tool authorization was already consumed before execution".into());
    }
    if aggregate.state.phase != Some(LifecyclePhaseV1::ToolAuthorized) {
        return Err("tool execution is not at the canonical authorized phase".into());
    }
    if active.mutation {
        validate_active_mutation_authority(store, &aggregate, now)?;
    }
    let event = command_from_active(
        store,
        context,
        &aggregate,
        LifecycleEventTypeV1::ToolStarted,
        LifecyclePayloadV1::ToolStarted {
            authorization_sha256: authorization.decision_sha256.clone(),
        },
        now,
    )?;
    append_lifecycle_event(store, event, now)?;
    Ok(())
}

/// Records the exact result of a consumed request. Projection invalidation,
/// refresh, verification, and outcome remain distinct subsequent lifecycle
/// transitions; completion never silently claims them.
pub fn complete_lifecycle_tool<E: Engine>(
    store: &E,
    context: &LifecycleSupervisorContextV1,
    authorization: &LifecycleToolAuthorizationV1,
    completion: &LifecycleToolCompletionV1,
    now: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    context.validate()?;
    authorization.validate()?;
    completion.validate()?;
    let scope = ScopeId::new(context.scope.clone())?;
    let aggregate = load_current(store, &scope, context)?;
    let Some(active) = aggregate.state.active_tool.as_ref() else {
        return replay_exact_completion(&aggregate, authorization, completion);
    };
    require_same_authorization(active, authorization)?;
    if aggregate.state.phase != Some(LifecyclePhaseV1::ToolStarted) || !active.consumed {
        return Err("tool completion does not follow one consumed authorization".into());
    }
    let event_type = if completion.success {
        LifecycleEventTypeV1::ToolCompleted
    } else {
        LifecycleEventTypeV1::ToolFailed
    };
    let event = command_from_active(
        store,
        context,
        &aggregate,
        event_type,
        LifecyclePayloadV1::ToolFinished {
            tool_request_sha256: authorization.tool_request_sha256.clone(),
            observation_sha256: completion.observation_sha256.clone(),
            success: completion.success,
            project_state_changed: completion.project_state_changed,
        },
        now,
    )?;
    append_lifecycle_event(store, event, now)?;
    Ok(())
}

fn authorize_proposal<E: Engine>(
    store: &E,
    context: &LifecycleSupervisorContextV1,
    aggregate: &LifecycleAggregate,
    request: &LifecycleToolRequestV1,
    now: u64,
) -> Result<LifecycleToolAuthorizationV1, Box<dyn std::error::Error>> {
    if aggregate.state.phase != Some(LifecyclePhaseV1::ToolProposed) {
        return Err("lifecycle tool is not awaiting an authorization decision".into());
    }
    let active = aggregate
        .state
        .active_tool
        .as_ref()
        .ok_or("lifecycle proposal has no active tool binding")?;
    require_same_request(active, request)?;
    if request.mutation {
        validate_active_mutation_authority(store, aggregate, now)?;
    }
    let decision_sha256 = digest::sha256_hex(&serde_json::to_vec(&(
        "rrflow-tool-authorization-v1",
        &aggregate.state.digest()?,
        &request.tool_call_id,
        &request.tool_request_sha256,
    ))?);
    let event = command_from_active(
        store,
        context,
        aggregate,
        LifecycleEventTypeV1::ToolAuthorized,
        LifecyclePayloadV1::ToolDecision {
            decision_sha256: decision_sha256.clone(),
            allowed: true,
            reason_code: "canonical-work-item-and-planning-binding".into(),
        },
        now,
    )?;
    append_lifecycle_event(store, event, now)?;
    Ok(LifecycleToolAuthorizationV1 {
        attempt_id: active.attempt_id.clone(),
        tool_call_id: active.tool_call_id.clone(),
        tool_request_sha256: active.tool_request_sha256.clone(),
        decision_sha256,
    })
}

fn active_authorization(
    active: &super::binding::ToolBinding,
) -> Result<LifecycleToolAuthorizationV1, Box<dyn std::error::Error>> {
    Ok(LifecycleToolAuthorizationV1 {
        attempt_id: active.attempt_id.clone(),
        tool_call_id: active.tool_call_id.clone(),
        tool_request_sha256: active.tool_request_sha256.clone(),
        decision_sha256: active
            .authorization_sha256()
            .ok_or("active lifecycle tool has no authorization digest")?
            .to_owned(),
    })
}

fn load_current<E: Engine>(
    store: &E,
    scope: &ScopeId,
    context: &LifecycleSupervisorContextV1,
) -> Result<LifecycleAggregate, Box<dyn std::error::Error>> {
    let observed_head = store.runtime_cursor()?;
    let aggregate = load_aggregate_at(
        store,
        scope,
        &context.project_id,
        &context.session_id,
        observed_head,
    )?
    .ok_or("canonical lifecycle session is absent")?;
    let snapshot = aggregate.snapshot()?;
    if snapshot.phase == LifecyclePhaseV1::SessionClosed {
        return Err("canonical lifecycle session is closed".into());
    }
    Ok(aggregate)
}

fn command_from_active<E: Engine>(
    store: &E,
    context: &LifecycleSupervisorContextV1,
    aggregate: &LifecycleAggregate,
    event_type: LifecycleEventTypeV1,
    payload: LifecyclePayloadV1,
    now: u64,
) -> Result<LifecycleEventCommandV1, Box<dyn std::error::Error>> {
    let active = aggregate
        .state
        .active_tool
        .as_ref()
        .ok_or("lifecycle transition has no active tool")?;
    command(
        store,
        context,
        aggregate,
        event_type,
        payload,
        now,
        aggregate.state.active_turn_id.clone(),
        aggregate.state.active_reasoning_run_id.clone(),
        Some(active.attempt_id.clone()),
        Some(active.tool_call_id.clone()),
    )
}

#[allow(clippy::too_many_arguments)]
fn command<E: Engine>(
    store: &E,
    context: &LifecycleSupervisorContextV1,
    aggregate: &LifecycleAggregate,
    event_type: LifecycleEventTypeV1,
    payload: LifecyclePayloadV1,
    now: u64,
    turn_id: Option<String>,
    reasoning_run_id: Option<String>,
    attempt_id: Option<String>,
    tool_call_id: Option<String>,
) -> Result<LifecycleEventCommandV1, Box<dyn std::error::Error>> {
    let scope = ScopeId::new(context.scope.clone())?;
    let read = store.runtime_read_stamp(&scope)?;
    let sequence = aggregate.state.sequence.saturating_add(1);
    let span_source = digest::sha256_hex(&serde_json::to_vec(&(
        &context.session_id,
        sequence,
        event_type.as_str(),
        &aggregate.state.digest()?,
    ))?);
    let latest = aggregate
        .events
        .last()
        .ok_or("canonical lifecycle session has no opening event")?;
    Ok(LifecycleEventCommandV1 {
        event_type,
        occurred_at_unix_ms: now,
        instance_id: latest.instance_id.clone(),
        project_id: context.project_id.clone(),
        member_id: latest.member_id.clone(),
        session_id: context.session_id.clone(),
        turn_id,
        reasoning_run_id,
        attempt_id,
        tool_call_id,
        correlation_id: format!("supervisor-{}", &span_source[..24]),
        actor: context.actor.clone(),
        adapter_kind: context.adapter_kind.clone(),
        adapter_version: context.adapter_version.clone(),
        enforcement_level: context.enforcement_level,
        scope: context.scope.clone(),
        payload,
        read_stamp: Some(LifecycleReadStampV1 {
            runtime_cursor: read.commit_cursor,
            manifest_id: read.manifest_id,
            catalogue_revision: read.catalog_revision,
        }),
        trace: LifecycleTraceContextV1 {
            trace_id: latest.trace.trace_id.clone(),
            span_id: span_source[..16].to_owned(),
            parent_span_id: Some(latest.trace.span_id.clone()),
        },
    })
}

fn attempt_id(
    aggregate: &LifecycleAggregate,
    request: &LifecycleToolRequestV1,
) -> Result<String, Box<dyn std::error::Error>> {
    let digest = digest::sha256_hex(&serde_json::to_vec(&(
        "rrflow-tool-attempt-v1",
        &aggregate.state.session_id,
        aggregate.state.sequence.saturating_add(1),
        &request.tool_call_id,
        &request.tool_request_sha256,
    ))?);
    Ok(format!("attempt-{}", &digest[..24]))
}

fn require_same_request(
    active: &super::binding::ToolBinding,
    request: &LifecycleToolRequestV1,
) -> Result<(), Box<dyn std::error::Error>> {
    if active.tool_call_id != request.tool_call_id
        || active.tool_name != request.tool_name
        || active.tool_request_sha256 != request.tool_request_sha256
        || active.mutation != request.mutation
    {
        return Err("another lifecycle tool request is already active".into());
    }
    Ok(())
}

fn require_same_authorization(
    active: &super::binding::ToolBinding,
    authorization: &LifecycleToolAuthorizationV1,
) -> Result<(), Box<dyn std::error::Error>> {
    if active.attempt_id != authorization.attempt_id
        || active.tool_call_id != authorization.tool_call_id
        || active.tool_request_sha256 != authorization.tool_request_sha256
        || active.authorization_sha256() != Some(authorization.decision_sha256.as_str())
    {
        return Err("tool operation does not match the canonical authorization".into());
    }
    Ok(())
}

fn replay_exact_completion(
    aggregate: &LifecycleAggregate,
    authorization: &LifecycleToolAuthorizationV1,
    completion: &LifecycleToolCompletionV1,
) -> Result<(), Box<dyn std::error::Error>> {
    let authorized = aggregate.events.iter().any(|event| {
        event.attempt_id.as_deref() == Some(authorization.attempt_id.as_str())
            && event.tool_call_id.as_deref() == Some(authorization.tool_call_id.as_str())
            && matches!(
                &event.payload,
                LifecyclePayloadV1::ToolDecision {
                    decision_sha256,
                    allowed: true,
                    ..
                } if decision_sha256 == &authorization.decision_sha256
            )
    });
    let terminal = aggregate.events.iter().rev().find(|event| {
        event.attempt_id.as_deref() == Some(authorization.attempt_id.as_str())
            && event.tool_call_id.as_deref() == Some(authorization.tool_call_id.as_str())
            && matches!(
                event.event_type,
                LifecycleEventTypeV1::ToolCompleted | LifecycleEventTypeV1::ToolFailed
            )
    });
    let Some(terminal) = terminal else {
        return Err("tool completion has no active or terminal canonical lifecycle tool".into());
    };
    let LifecyclePayloadV1::ToolFinished {
        tool_request_sha256,
        observation_sha256,
        success,
        project_state_changed,
    } = &terminal.payload
    else {
        return Err("terminal lifecycle tool has the wrong payload".into());
    };
    if !authorized
        || tool_request_sha256 != &authorization.tool_request_sha256
        || observation_sha256 != &completion.observation_sha256
        || success != &completion.success
        || project_state_changed != &completion.project_state_changed
    {
        return Err("tool completion replay differs from the terminal lifecycle result".into());
    }
    Ok(())
}
