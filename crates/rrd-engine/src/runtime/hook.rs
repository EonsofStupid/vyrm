//! Harness lifecycle dispatch. One entrypoint (`rrflow hook <event>`) reads
//! the harness's JSON on stdin and answers on stdout — for injection events
//! the text goes into the model's context, for gate events a JSON decision
//! blocks or allows the tool call. `PLAN.md` Step P.
//!
//! The internal events are provider-neutral; the adapter edge renders Claude
//! Code/Codex or Gemini output shapes. Read-only payloads remain tolerant, but mutation
//! payloads crossing a checked-in work plan fail closed unless they carry a
//! stable canonical session and tool-call identity.

mod supervision;

use super::policy::{is_project_mutation_tool, tool_request_digest};
use super::preflight::{preflight, Preflight};
use super::reasoning::active_reasoning_run;
use super::routing::ensure_routing_fresh;
use super::stack;
use super::sync_project_work_plan;
use super::workflow::{
    resolve_package_argv, resolve_package_command, WorkflowDecision, WorkflowObservation,
};
use super::{
    authorize_attuned_tool, complete_attuned_tool, record_attunement_receipt,
    require_fresh_attunement,
};
use super::{evaluate_tool, DurableTraceSpan, ToolPolicy};
use rrd_core::{
    digest, recall, resolve_as_of, Check, CheckStatus, Claim, Evidence, Millis, Predicate,
    Producer, Reader, ReasoningPayload, ReasoningState, RecallQuery, RuntimeCommit,
    RuntimeEventSchema, RuntimeMutation, RuntimeProperties, RuntimeSchemaRegistry, RuntimeType,
    RuntimeValue, ScopeId, Subject, TraceDataClass, TraceDomain, TraceLink, TraceOutcome,
};
use rrd_store::{Effectiveness, Engine, ProjectionStatus, RecallOutcome};
use serde_json::Value;

/// Lifecycle events the dispatcher answers. Kebab-case names match the CLI
/// (`rrflow hook session-start`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    Stop,
    PreCompact,
    SessionEnd,
}

impl HookEvent {
    pub fn parse(name: &str) -> Option<HookEvent> {
        Some(match name {
            "session-start" => HookEvent::SessionStart,
            "user-prompt-submit" => HookEvent::UserPromptSubmit,
            "pre-tool-use" => HookEvent::PreToolUse,
            "post-tool-use" => HookEvent::PostToolUse,
            "stop" => HookEvent::Stop,
            "pre-compact" => HookEvent::PreCompact,
            "session-end" => HookEvent::SessionEnd,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            HookEvent::SessionStart => "session-start",
            HookEvent::UserPromptSubmit => "user-prompt-submit",
            HookEvent::PreToolUse => "pre-tool-use",
            HookEvent::PostToolUse => "post-tool-use",
            HookEvent::Stop => "stop",
            HookEvent::PreCompact => "pre-compact",
            HookEvent::SessionEnd => "session-end",
        }
    }
}

/// What a hook produced. `stdout` is the harness-facing answer; the rest is
/// for the invocation record.
#[derive(Debug, Default)]
pub struct HookResponse {
    pub stdout: String,
    pub effectiveness: Option<Effectiveness>,
    pub detail: Option<String>,
    pub lifecycle_context: Option<super::LifecycleSupervisorContextV1>,
    pub lifecycle_authorization: Option<super::LifecycleToolAuthorizationV1>,
}

/// Everything a dispatch runs against: the estate, the project, the adapter,
/// and the clock — which enters here and nowhere deeper, as everywhere else.
pub struct HookContext<'a, E: Engine> {
    pub store: &'a E,
    pub root: &'a std::path::Path,
    pub harness: Option<&'a str>,
    pub reader: &'a Reader,
    pub now: Millis,
    pub budget: usize,
}

/// Dispatches one event. `input` is the harness JSON from stdin.
#[tracing::instrument(level = "debug", skip_all, fields(event = event.name()))]
pub fn handle<E: Engine>(
    ctx: &HookContext<'_, E>,
    event: HookEvent,
    input: &Value,
) -> Result<HookResponse, Box<dyn std::error::Error>> {
    let binding = super::InstanceBinding::discover(ctx.root)?;
    binding.require_runtime_ready()?;
    let scope = ScopeId::new(super::REASONING_SCOPE)?;
    let read = ctx.store.runtime_read_stamp(&scope)?;
    let input_bytes = serde_json::to_vec(input)?;
    let input_digest = digest::sha256_hex(&input_bytes);
    let at_bytes = ctx.now.to_be_bytes();
    let cursor_bytes = read.commit_cursor.to_be_bytes();
    let identity = super::TraceIdentity::derive(&[
        binding.manifest.id.as_bytes(),
        event.name().as_bytes(),
        input_digest.as_bytes(),
        &at_bytes,
        &cursor_bytes,
    ])?;
    let actor = format!("hook:{}", ctx.harness.unwrap_or("unknown"));
    let mut links = vec![TraceLink::Read { stamp: read }];
    if let Ok(Some(run)) = active_reasoning_run(ctx.store) {
        links.push(TraceLink::ReasoningRun {
            run_id: run.id().to_owned(),
        });
    }
    let attributes = RuntimeProperties::from([
        ("event".into(), RuntimeValue::String(event.name().into())),
        (
            "harness".into(),
            RuntimeValue::String(ctx.harness.unwrap_or("unknown").into()),
        ),
        ("input_digest".into(), RuntimeValue::Digest(input_digest)),
        (
            "input_bytes".into(),
            RuntimeValue::Unsigned(input_bytes.len() as u64),
        ),
    ]);
    let span = DurableTraceSpan::start(
        ctx.store,
        scope,
        actor,
        identity,
        None,
        TraceDomain::Lifecycle,
        format!("lifecycle.{}", event.name()),
        ctx.now,
        TraceDataClass::Control,
        links,
        attributes,
    )?;

    let dispatch = handle_inner(ctx, event, input, &binding);
    let trace_outcome = match &dispatch {
        Ok(response) if response_denied(response) => TraceOutcome::Denied,
        Ok(_) => TraceOutcome::Ok,
        Err(_) => TraceOutcome::Error,
    };
    let trace_finish = span.finish(
        ctx.store,
        trace_outcome,
        Vec::new(),
        RuntimeProperties::from([(
            "response_bytes".into(),
            RuntimeValue::Unsigned(
                dispatch
                    .as_ref()
                    .map_or(0, |response| response.stdout.len() as u64),
            ),
        )]),
    );
    match (dispatch, trace_finish) {
        (Ok(response), Ok(_)) => Ok(response),
        (Err(error), Ok(_)) => Err(error),
        (Ok(_), Err(trace_error)) => Err(format!(
            "lifecycle operation completed but its finish trace was not durable: {trace_error}"
        )
        .into()),
        (Err(error), Err(trace_error)) => Err(format!(
            "lifecycle operation failed: {error}; its finish trace also failed: {trace_error}"
        )
        .into()),
    }
}

fn handle_inner<E: Engine>(
    ctx: &HookContext<'_, E>,
    event: HookEvent,
    input: &Value,
    binding: &super::InstanceBinding,
) -> Result<HookResponse, Box<dyn std::error::Error>> {
    let HookContext {
        store,
        root,
        harness,
        reader,
        now,
        budget,
    } = *ctx;
    match event {
        HookEvent::SessionStart => {
            let Preflight {
                context,
                effectiveness,
                warnings,
                ..
            } = preflight(store, root, harness, reader, now, budget)?;
            Ok(HookResponse {
                stdout: context_output(harness, HookEvent::SessionStart, context),
                effectiveness: Some(effectiveness),
                detail: (!warnings.is_empty()).then(|| format!("{} warning(s)", warnings.len())),
                ..HookResponse::default()
            })
        }

        HookEvent::UserPromptSubmit => {
            let prompt = input.get("prompt").and_then(Value::as_str).unwrap_or("");
            let ready = ensure_routing_fresh(store, root)?;
            record_attunement_receipt(
                store,
                root,
                &ready,
                now,
                &format!("hook:{}", harness.unwrap_or("unknown")),
                Some(digest::sha256_hex(prompt.as_bytes())),
            )?;
            let work_plan = sync_project_work_plan(
                store,
                root,
                now,
                &format!("hook:{}", harness.unwrap_or("unknown")),
            )?;
            let work_plan_context = work_plan.as_ref().map(|snapshot| {
                let verified = snapshot
                    .items
                    .iter()
                    .filter(|item| item.status == super::WorkItemStatus::Verified)
                    .count();
                format!(
                    "[rrflow] enforced work plan {}: verified={}/{} active={} revision={} digest={}",
                    snapshot.plan_id,
                    verified,
                    snapshot.items.len(),
                    snapshot.active_item_id.as_deref().unwrap_or("none"),
                    snapshot.revision,
                    snapshot.plan_sha256,
                )
            });
            let matched = matched_subjects(store, prompt)?;
            if matched.is_empty() {
                return Ok(HookResponse {
                    stdout: context_output(
                        harness,
                        HookEvent::UserPromptSubmit,
                        work_plan_context.unwrap_or_default(),
                    ),
                    ..HookResponse::default()
                });
            }
            let query = RecallQuery {
                subjects: matched,
                predicates: None,
                as_of: now,
            };
            let set = recall(store, &query, budget)?;
            for claim in &set.claims {
                store.observe(reader, &claim.subject, &claim.predicate, now)?;
            }
            let mut lines = vec![format!(
                "[rrflow] recall for this prompt ({} claim(s), ~{} token(s)):",
                set.claims.len(),
                set.token_estimate
            )];
            if let Some(context) = work_plan_context {
                lines.insert(0, context);
            }
            lines.extend(set.claims.iter().map(render_claim));
            let effectiveness = Effectiveness {
                query: query
                    .subjects
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                claims_returned: set.claims.len(),
                tokens_emitted: set.token_estimate as u64,
                baseline_tokens: None,
                baseline_mode: None,
                provider: harness
                    .map(|h| format!("harness:{h}"))
                    .unwrap_or_else(|| "operator:cli".into()),
                outcome: RecallOutcome::Unknown,
            };
            Ok(HookResponse {
                stdout: context_output(harness, HookEvent::UserPromptSubmit, lines.join("\n")),
                effectiveness: Some(effectiveness),
                detail: None,
                ..HookResponse::default()
            })
        }

        HookEvent::PreToolUse => {
            // Read-only and RRFlow control-plane calls bypass project-mutation
            // policy. The latter is deliberately narrow so recording the
            // contract or recovering a quarantine cannot deadlock itself.
            match evaluate_tool(None, input) {
                ToolPolicy::ReadOnly => return Ok(HookResponse::default()),
                ToolPolicy::ControlPlane => {
                    return Ok(HookResponse {
                        detail: Some("allowed: RRFlow control plane".into()),
                        ..HookResponse::default()
                    })
                }
                ToolPolicy::Allow { .. } | ToolPolicy::Deny { .. } => {}
            }

            let run = match active_reasoning_run(store) {
                Ok(run) => run,
                Err(error) => {
                    return Ok(deny(
                        harness,
                        format!("rrflow: reasoning contract cannot be trusted. Wait: {error}"),
                        "denied: reasoning ledger unavailable",
                    ))
                }
            };
            let policy_evidence = match evaluate_tool(run.as_ref(), input) {
                ToolPolicy::Allow { differential } => differential.render(),
                ToolPolicy::Deny { differential } => {
                    let rendered = differential.render();
                    return Ok(deny(
                        harness,
                        format!("rrflow: mutation denied by reasoning policy. Wait: {rendered}"),
                        &format!("denied: {rendered}"),
                    ));
                }
                ToolPolicy::ReadOnly | ToolPolicy::ControlPlane => unreachable!("handled above"),
            };
            let run_id = run
                .as_ref()
                .expect("an allowed mutation has an active reasoning run")
                .id()
                .to_owned();

            // The estate wait gate follows the contract gate: a quarantined
            // projection makes even a properly declared attempt wait.
            if let ProjectionStatus::Quarantined { at, .. } = store.current_projection()?.status {
                return Ok(deny(
                    harness,
                    format!(
                        "rrflow: memory projection quarantined at {at} — grounding found \
                         divergence. Wait: resolve it first (`rrflow ground` to see the \
                         differential, `rrflow reset-projection` to recover)."
                    ),
                    "denied: projection quarantined",
                ));
            }
            // The second wait gate is source evidence. It refreshes immediately
            // before the mutation, persists any new generation, and denies if
            // the project tree cannot be read or the stored routing state
            // cannot be trusted.
            let ready = match ensure_routing_fresh(store, root) {
                Ok(ready) => ready,
                Err(error) => {
                    return Ok(deny(
                        harness,
                        format!(
                            "rrflow: source-routing freshness could not be established. Wait: {error}"
                        ),
                        "denied: routing freshness unavailable",
                    ));
                }
            };
            let receipt = match require_fresh_attunement(store, root, &ready) {
                Ok(receipt) => receipt,
                Err(error) => {
                    return Ok(deny(
                        harness,
                        format!("rrflow: project attunement is absent or stale. Wait: {error}"),
                        "denied: project attunement receipt unavailable or stale",
                    ));
                }
            };

            let workflow_decision = match input.get("tool_name").and_then(Value::as_str) {
                Some("Bash") => {
                    let command = input
                        .pointer("/tool_input/command")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    Some(resolve_package_command(root, binding, command))
                }
                Some("RRFlowExec") => {
                    let argv = exact_exec_argv(input)?;
                    Some(resolve_package_argv(root, binding, &argv))
                }
                _ => None,
            };
            let workflow_evidence = match workflow_decision {
                Some(decision) => match decision {
                    Ok(WorkflowDecision::NotPackage) => None,
                    Ok(WorkflowDecision::Allow(authorization)) => {
                        match authorization.establish_freshness(&ready) {
                            Ok(evidence) => Some(evidence),
                            Err(differential) => {
                                let rendered = differential.render();
                                return Ok(deny(
                                    harness,
                                    format!("rrflow: package workflow denied. Wait: {rendered}"),
                                    &format!("denied: {rendered}"),
                                ));
                            }
                        }
                    }
                    Ok(WorkflowDecision::Deny(differential)) => {
                        let rendered = differential.render();
                        return Ok(deny(
                            harness,
                            format!("rrflow: package workflow denied. Wait: {rendered}"),
                            &format!("denied: {rendered}"),
                        ));
                    }
                    Err(error) => {
                        return Ok(deny(
                            harness,
                            format!(
                                "rrflow: package workflow policy cannot be trusted. Wait: {error}"
                            ),
                            "denied: workflow manifest unavailable",
                        ));
                    }
                },
                None => None,
            };
            let tool_sha256 = tool_request_digest(input)?;
            let work_plan = match sync_project_work_plan(
                store,
                root,
                now,
                &format!("hook:{}", harness.unwrap_or("unknown")),
            ) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return Ok(deny(
                        harness,
                        format!("rrflow: checked-in work plan cannot be trusted. Wait: {error}"),
                        "denied: work-plan source invalid",
                    ));
                }
            };
            let mut lifecycle_context = None;
            let mut lifecycle_authorization = None;
            let (receipt_sha256, work_plan_item, consumed) = if let Some(snapshot) = &work_plan {
                let supervised = match supervision::authorize(
                    store, binding, harness, input, snapshot, &receipt, now,
                ) {
                    Ok(supervised) => supervised,
                    Err(error) => {
                        return Ok(deny(
                            harness,
                            format!(
                                "rrflow: mutation denied by the canonical work-plan lifecycle. Wait: {error}"
                            ),
                            "denied: canonical lifecycle authorization unavailable",
                        ));
                    }
                };
                let result = (
                    receipt.receipt_sha256.clone(),
                    Some(supervised.work_item_id.clone()),
                    supervised.consumed,
                );
                lifecycle_context = Some(supervised.context);
                lifecycle_authorization = Some(supervised.authorization);
                result
            } else {
                let receipt = match authorize_attuned_tool(
                    store,
                    receipt,
                    &run_id,
                    &tool_sha256,
                    now,
                    &format!("hook:{}", harness.unwrap_or("unknown")),
                ) {
                    Ok(receipt) => receipt,
                    Err(error) => {
                        return Ok(deny(
                            harness,
                            format!(
                                "rrflow: tool authorization is stale or consumed. Wait: {error}"
                            ),
                            "denied: attunement tool authorization unavailable",
                        ));
                    }
                };
                (receipt.receipt_sha256, None, false)
            };
            let mut detail = format!(
                "policy allowed ({policy_evidence}); routing freshness established: {}; attunement receipt {}",
                ready.render(), receipt_sha256
            );
            if let Some(evidence) = workflow_evidence {
                detail.push_str("; ");
                detail.push_str(&evidence);
            }
            if let Some(item_id) = work_plan_item {
                detail.push_str(&format!(
                    "; canonical lifecycle bound work item {item_id}; authorization {}",
                    if consumed {
                        "consumed"
                    } else {
                        "awaiting proxy consumption"
                    }
                ));
            }
            Ok(HookResponse {
                detail: Some(detail),
                lifecycle_context,
                lifecycle_authorization,
                ..HookResponse::default()
            })
        }

        HookEvent::PostToolUse => {
            let tool = input
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if matches!(evaluate_tool(None, input), ToolPolicy::ControlPlane) {
                return Ok(HookResponse {
                    detail: Some("ignored: RRFlow control plane".into()),
                    ..HookResponse::default()
                });
            }
            let mut details = Vec::new();

            // Close the pre-tool authorization with immutable evidence. One
            // declared attempt authorizes one tool result; the resulting
            // observation moves the run to NeedsDecision, so another mutation
            // cannot ride the same declaration. Verification Bash calls are
            // similarly converted into typed pass/fail checks from exit code.
            if is_project_mutation_tool(tool) {
                let work_plan = sync_project_work_plan(
                    store,
                    root,
                    now,
                    &format!("hook:{}", harness.unwrap_or("unknown")),
                )?;
                if let Some(snapshot) = &work_plan {
                    let completed =
                        supervision::complete(store, root, binding, harness, input, snapshot, now)?;
                    details.push(format!(
                        "canonical work item {} observation recorded; projection_refreshed={}",
                        completed.work_item_id, completed.projection_refreshed
                    ));
                } else {
                    complete_attuned_tool(
                        store,
                        root,
                        &tool_request_digest(input)?,
                        now,
                        &format!("hook:{}", harness.unwrap_or("unknown")),
                    )?;
                }
                if let Some(run) = active_reasoning_run(store)? {
                    let encoded = serde_json::to_vec(input)?;
                    let evidence = Evidence {
                        source: tool_source(input),
                        digest: rrd_core::digest::sha256_hex(&encoded),
                        summary: format!("{tool} hook result captured"),
                    };
                    let payload = match run.state() {
                        ReasoningState::NeedsObservation => Some(ReasoningPayload::Observation {
                            summary: format!("observed result of declared {tool} attempt"),
                            evidence: vec![evidence],
                        }),
                        ReasoningState::NeedsVerification
                            if matches!(tool, "Bash" | "run_shell_command") =>
                        {
                            let status = if run_exit_code(input) == Some(0) {
                                CheckStatus::Passed
                            } else {
                                CheckStatus::Failed
                            };
                            Some(ReasoningPayload::Verification {
                                checks: vec![Check {
                                    name: format!(
                                        "verify {}",
                                        first_line(
                                            input
                                                .pointer("/tool_input/command")
                                                .and_then(Value::as_str)
                                                .unwrap_or("unreported command")
                                        )
                                    ),
                                    status,
                                    evidence: vec![evidence],
                                }],
                            })
                        }
                        _ => None,
                    };
                    if let Some(payload) = payload {
                        let event = super::reasoning::record_reasoning(
                            store,
                            run.id(),
                            now,
                            &format!("hook:{}", harness.unwrap_or("unknown")),
                            payload,
                        )?;
                        details.push(format!(
                            "reasoning {} #{} recorded",
                            event.payload.name(),
                            event.ordinal
                        ));
                    }
                }
            }

            // The application journal: a run's outcome becomes a claim, and
            // the next run of the same kind supersedes it. Retirement by
            // supersession, exactly as every other claim.
            if !matches!(tool, "Bash" | "RRFlowExec") {
                return Ok(HookResponse {
                    detail: (!details.is_empty()).then(|| details.join("; ")),
                    ..HookResponse::default()
                });
            }
            let exact_argv = (tool == "RRFlowExec")
                .then(|| exact_exec_argv(input))
                .transpose()?;
            let command = exact_argv.as_ref().map_or_else(
                || {
                    input
                        .pointer("/tool_input/command")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned()
                },
                |argv| argv.join(" "),
            );
            let workflow_decision = match exact_argv.as_ref() {
                Some(argv) => resolve_package_argv(root, binding, argv)?,
                None => resolve_package_command(root, binding, &command)?,
            };
            match workflow_decision {
                WorkflowDecision::Allow(authorization) => {
                    let response = input.get("tool_response").unwrap_or(&Value::Null);
                    let observation = match exact_argv.as_ref() {
                        Some(argv) => WorkflowObservation::capture_argv(
                            &authorization,
                            argv,
                            response,
                            run_exit_code(input),
                            now,
                        )?,
                        None => WorkflowObservation::capture(
                            &authorization,
                            &command,
                            response,
                            run_exit_code(input),
                            now,
                        )?,
                    };
                    let claim = Claim::new(
                        Subject::new(authorization.event.clone())?,
                        Predicate::new("status")?,
                        serde_json::to_string(&observation)?,
                        now,
                        now,
                        Producer {
                            actor: format!("hook:{}", harness.unwrap_or("unknown")),
                            on_behalf_of: None,
                            session: None,
                        },
                    );
                    let mut mutations = superseding_claim_mutations(store, &claim)?;
                    if store.runtime_schema(&authorization.scope)?.is_none() {
                        let mut registry = RuntimeSchemaRegistry::empty(
                            1,
                            "install package workflow evidence contract",
                        );
                        registry.events.insert(
                            RuntimeType::new("workflow-observation")?,
                            RuntimeEventSchema::default(),
                        );
                        mutations.insert(0, RuntimeMutation::Schema { registry });
                    }
                    let outcome = store.commit_runtime(&RuntimeCommit {
                        scope: authorization.scope,
                        at: now,
                        actor: format!("hook:{}", harness.unwrap_or("unknown")),
                        expected_cursor: store.runtime_cursor()?,
                        mutations,
                    })?;
                    details.push(format!(
                        "workflow {} committed atomically: status={:?} cursor={} audit={}",
                        observation.event,
                        observation.status,
                        outcome.last_cursor,
                        outcome.commit_id,
                    ));
                    return Ok(HookResponse {
                        detail: Some(details.join("; ")),
                        ..HookResponse::default()
                    });
                }
                WorkflowDecision::Deny(differential) => {
                    return Err(format!(
                        "post-tool package command has no trusted pre-tool declaration: {}",
                        differential.render()
                    )
                    .into())
                }
                WorkflowDecision::NotPackage => {}
            }
            let Some((subject, stack_name)) = stack::detect(root)
                .iter()
                .find_map(|s| s.run_subject(&command).map(|subj| (subj, s.name)))
            else {
                return Ok(HookResponse {
                    detail: (!details.is_empty()).then(|| details.join("; ")),
                    ..HookResponse::default()
                });
            };
            let object = match run_exit_code(input) {
                Some(0) => format!("passing: {}", first_line(&command)),
                Some(code) => format!("failing (exit {code}): {}", first_line(&command)),
                None => format!(
                    "ran (outcome unreported by harness): {}",
                    first_line(&command)
                ),
            };
            let claim = Claim::new(
                Subject::new(subject.clone())?,
                Predicate::new("status")?,
                object.clone(),
                now,
                now,
                Producer {
                    actor: format!("hook:{}", harness.unwrap_or("unknown")),
                    on_behalf_of: None,
                    session: None,
                },
            );
            store.assert(&claim)?;
            details.push(format!("journaled {stack_name} run: {subject} = {object}"));
            Ok(HookResponse {
                stdout: String::new(),
                effectiveness: None,
                detail: Some(details.join("; ")),
                ..HookResponse::default()
            })
        }

        // Turn boundaries are journaled through the invocation record itself
        // (`main` records every hook dispatch); no claims are asserted here.
        // Outcome auto-judging at Stop is D-4 — open, not sneaked in.
        HookEvent::Stop => Ok(HookResponse {
            detail: Some("turn ended".into()),
            ..HookResponse::default()
        }),
        HookEvent::PreCompact => Ok(HookResponse {
            detail: Some("compaction imminent; session-start re-injects after".into()),
            ..HookResponse::default()
        }),
        HookEvent::SessionEnd => Ok(HookResponse {
            detail: Some("session ended".into()),
            ..HookResponse::default()
        }),
    }
}

fn response_denied(response: &HookResponse) -> bool {
    serde_json::from_str::<Value>(&response.stdout)
        .ok()
        .and_then(|value| {
            value
                .pointer("/hookSpecificOutput/permissionDecision")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    value
                        .get("decision")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
        })
        .as_deref()
        == Some("deny")
}

fn superseding_claim_mutations<E: Engine>(
    store: &E,
    claim: &Claim,
) -> Result<Vec<RuntimeMutation>, Box<dyn std::error::Error>> {
    let candidates =
        store.versions_at_or_before(&claim.subject, &claim.predicate, claim.valid_from)?;
    let previous = resolve_as_of(&candidates, claim.valid_from).cloned();
    let claims = match previous {
        Some(previous) if previous.valid_from < claim.valid_from => {
            rrd_core::supersede(&previous, claim.clone())?.to_vec()
        }
        _ => vec![claim.clone()],
    };
    Ok(claims
        .into_iter()
        .map(|claim| RuntimeMutation::Claim { claim })
        .collect())
}

fn deny(harness: Option<&str>, reason: String, detail: &str) -> HookResponse {
    let decision = if harness == Some("gemini-cli") {
        serde_json::json!({"decision": "deny", "reason": reason})
    } else {
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        })
    };
    HookResponse {
        stdout: decision.to_string(),
        effectiveness: None,
        detail: Some(detail.to_owned()),
        ..HookResponse::default()
    }
}

fn context_output(harness: Option<&str>, event: HookEvent, context: String) -> String {
    if context.is_empty() || harness != Some("gemini-cli") {
        return context;
    }
    let native_event = match event {
        HookEvent::SessionStart => "SessionStart",
        HookEvent::UserPromptSubmit => "BeforeAgent",
        _ => return context,
    };
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": native_event,
            "additionalContext": context,
        }
    })
    .to_string()
}

fn tool_source(input: &Value) -> String {
    let tool = input
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let target = input
        .pointer("/tool_input/file_path")
        .or_else(|| input.pointer("/tool_input/notebook_path"))
        .or_else(|| input.pointer("/tool_input/command"))
        .or_else(|| input.pointer("/tool_input/exact_argv/0"))
        .and_then(Value::as_str)
        .unwrap_or("unreported target");
    format!("{tool}:{target}")
}

fn exact_exec_argv(input: &Value) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let argv = input
        .pointer("/tool_input/exact_argv")
        .and_then(Value::as_array)
        .ok_or("RRFlowExec input has no exact_argv array")?;
    let argv = argv
        .iter()
        .map(|argument| {
            argument
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "RRFlowExec argv contains a non-string argument".into())
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    if argv.is_empty() || argv.iter().any(|argument| argument.contains('\0')) {
        return Err("RRFlowExec argv is empty or contains NUL".into());
    }
    Ok(argv)
}

/// Subjects whose name appears in the prompt as a whole word
/// (case-insensitive). Substring matching would recall `repo` for
/// "repository"; word boundaries keep recall answerable for what it injects.
fn matched_subjects<E: Engine>(
    store: &E,
    prompt: &str,
) -> Result<Vec<Subject>, Box<dyn std::error::Error>> {
    let lowered = prompt.to_lowercase();
    let words: std::collections::BTreeSet<&str> = lowered
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
        .filter(|w| !w.is_empty())
        .collect();
    Ok(store
        .subjects()?
        .into_iter()
        .filter(|s| words.contains(s.as_str().to_lowercase().as_str()))
        .collect())
}

fn render_claim(claim: &Claim) -> String {
    format!(
        "{} {} = {}  [valid_from={} tx={} by {}]",
        claim.subject.as_str(),
        claim.predicate.as_str(),
        claim.object,
        claim.valid_from,
        claim.tx_time,
        claim.producer.actor,
    )
}

/// Exit code of a Bash run, wherever the harness put it. Absent means the
/// outcome is unreported, which the journal states rather than guesses.
fn run_exit_code(input: &Value) -> Option<i64> {
    let response = input.get("tool_response")?;
    for field in ["exitCode", "exit_code", "code"] {
        if let Some(code) = response.get(field).and_then(Value::as_i64) {
            return Some(code);
        }
    }
    match response.get("success").and_then(Value::as_bool) {
        Some(true) => Some(0),
        Some(false) => Some(1),
        None => None,
    }
}

fn first_line(command: &str) -> &str {
    command.lines().next().unwrap_or(command).trim()
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[test]
    fn denial_renders_the_native_provider_contract() {
        let codex: Value =
            serde_json::from_str(&deny(Some("codex-cli"), "wait".into(), "denied").stdout).unwrap();
        assert_eq!(codex["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(codex.get("decision").is_none());

        let gemini: Value =
            serde_json::from_str(&deny(Some("gemini-cli"), "wait".into(), "denied").stdout)
                .unwrap();
        assert_eq!(gemini["decision"], "deny");
        assert_eq!(gemini["reason"], "wait");
        assert!(gemini.get("hookSpecificOutput").is_none());
    }

    #[test]
    fn gemini_context_uses_before_agent_while_codex_accepts_plain_text() {
        let gemini: Value = serde_json::from_str(&context_output(
            Some("gemini-cli"),
            HookEvent::UserPromptSubmit,
            "recalled".into(),
        ))
        .unwrap();
        assert_eq!(gemini["hookSpecificOutput"]["hookEventName"], "BeforeAgent");
        assert_eq!(
            gemini["hookSpecificOutput"]["additionalContext"],
            "recalled"
        );
        assert_eq!(
            context_output(
                Some("codex-cli"),
                HookEvent::UserPromptSubmit,
                "recalled".into()
            ),
            "recalled"
        );
    }
}
