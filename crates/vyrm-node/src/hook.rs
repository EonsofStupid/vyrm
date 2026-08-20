//! Harness lifecycle dispatch. One entrypoint (`vyrm hook <event>`) reads
//! the harness's JSON on stdin and answers on stdout — for injection events
//! the text goes into the model's context, for gate events a JSON decision
//! blocks or allows the tool call. `PLAN.md` Step P.
//!
//! Field names follow the Claude Code hook contract (the registry's one
//! hooks-capable adapter); extraction is tolerant of absent fields, because
//! a hook that panics on a shape change would take the operator's session
//! down with it. Unknown shapes degrade to "do nothing", never to an error.

use crate::preflight::{preflight, Preflight};
use crate::reasoning::active_reasoning_run;
use crate::routing::ensure_routing_fresh;
use crate::stack;
use crate::workflow::{resolve_package_command, WorkflowDecision, WorkflowObservation};
use crate::{evaluate_tool, ToolPolicy};
use serde_json::Value;
use std::time::Instant;
use vyrm_core::{
    digest, recall, resolve_as_of, Check, CheckStatus, Claim, Evidence, Millis, Predicate,
    Producer, Reader, ReasoningPayload, ReasoningState, RecallQuery, RuntimeCommit,
    RuntimeEventSchema, RuntimeMutation, RuntimeProperties, RuntimeSchemaRegistry,
    RuntimeTraceEvent, RuntimeType, RuntimeValue, ScopeId, Subject, TraceDataClass, TraceDomain,
    TraceLink, TraceOutcome,
};
use vyrm_store::{Effectiveness, Engine, ProjectionStatus, RecallOutcome};

/// Lifecycle events the dispatcher answers. Kebab-case names match the CLI
/// (`vyrm hook session-start`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    Stop,
    PreCompact,
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
    let binding = crate::InstanceBinding::discover(ctx.root)?;
    binding.require_runtime_ready()?;
    let scope = ScopeId::new(crate::REASONING_SCOPE)?;
    let read = ctx.store.runtime_read_stamp(&scope)?;
    let input_bytes = serde_json::to_vec(input)?;
    let input_digest = digest::sha256_hex(&input_bytes);
    let at_bytes = ctx.now.to_be_bytes();
    let cursor_bytes = read.commit_cursor.to_be_bytes();
    let identity = crate::TraceIdentity::derive(&[
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
    let common = RuntimeProperties::from([
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
    let start = RuntimeTraceEvent::start(
        identity.trace_id.clone(),
        identity.span_id.clone(),
        None,
        TraceDomain::Lifecycle,
        format!("lifecycle.{}", event.name()),
        ctx.now,
        TraceDataClass::Control,
        links.clone(),
        common.clone(),
    )?;
    let start_outcome = crate::record_runtime_trace(ctx.store, &scope, &actor, start)?;

    let started = Instant::now();
    let dispatch = handle_inner(ctx, event, input, &binding);
    let elapsed = started.elapsed();
    let trace_outcome = match &dispatch {
        Ok(response) if response_denied(response) => TraceOutcome::Denied,
        Ok(_) => TraceOutcome::Ok,
        Err(_) => TraceOutcome::Error,
    };
    let mut finish_links = links;
    finish_links.push(TraceLink::RuntimeCursor {
        cursor: ctx.store.runtime_cursor()?,
    });
    let mut finish_attributes = common;
    finish_attributes.insert(
        "start_cursor".into(),
        RuntimeValue::Unsigned(start_outcome.last_cursor),
    );
    finish_attributes.insert(
        "response_bytes".into(),
        RuntimeValue::Unsigned(
            dispatch
                .as_ref()
                .map_or(0, |response| response.stdout.len() as u64),
        ),
    );
    let duration_micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
    let finished_at = ctx
        .now
        .saturating_add(elapsed.as_millis().min(u128::from(u64::MAX)) as u64);
    let finish = RuntimeTraceEvent::finish(
        identity.trace_id,
        identity.span_id,
        None,
        TraceDomain::Lifecycle,
        format!("lifecycle.{}", event.name()),
        finished_at,
        duration_micros,
        trace_outcome,
        TraceDataClass::Control,
        finish_links,
        finish_attributes,
    )?;
    let trace_finish = crate::record_runtime_trace(ctx.store, &scope, &actor, finish);
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
    binding: &crate::InstanceBinding,
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
                stdout: context,
                effectiveness: Some(effectiveness),
                detail: (!warnings.is_empty()).then(|| format!("{} warning(s)", warnings.len())),
            })
        }

        HookEvent::UserPromptSubmit => {
            let prompt = input.get("prompt").and_then(Value::as_str).unwrap_or("");
            let matched = matched_subjects(store, prompt)?;
            if matched.is_empty() {
                return Ok(HookResponse::default());
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
                "[vyrm] recall for this prompt ({} claim(s), ~{} token(s)):",
                set.claims.len(),
                set.token_estimate
            )];
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
                stdout: lines.join("\n"),
                effectiveness: Some(effectiveness),
                detail: None,
            })
        }

        HookEvent::PreToolUse => {
            // Read-only and vyrm control-plane calls bypass project-mutation
            // policy. The latter is deliberately narrow so recording the
            // contract or recovering a quarantine cannot deadlock itself.
            match evaluate_tool(None, input) {
                ToolPolicy::ReadOnly => return Ok(HookResponse::default()),
                ToolPolicy::ControlPlane => {
                    return Ok(HookResponse {
                        detail: Some("allowed: vyrm control plane".into()),
                        ..HookResponse::default()
                    })
                }
                ToolPolicy::Allow { .. } | ToolPolicy::Deny { .. } => {}
            }

            let run = match active_reasoning_run(store) {
                Ok(run) => run,
                Err(error) => {
                    return Ok(deny(
                        format!("vyrm: reasoning contract cannot be trusted. Wait: {error}"),
                        "denied: reasoning ledger unavailable",
                    ))
                }
            };
            let policy_evidence = match evaluate_tool(run.as_ref(), input) {
                ToolPolicy::Allow { differential } => differential.render(),
                ToolPolicy::Deny { differential } => {
                    let rendered = differential.render();
                    return Ok(deny(
                        format!("vyrm: mutation denied by reasoning policy. Wait: {rendered}"),
                        &format!("denied: {rendered}"),
                    ));
                }
                ToolPolicy::ReadOnly | ToolPolicy::ControlPlane => unreachable!("handled above"),
            };

            // The estate wait gate follows the contract gate: a quarantined
            // projection makes even a properly declared attempt wait.
            if let ProjectionStatus::Quarantined { at, .. } = store.current_projection()?.status {
                let decision = serde_json::json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "deny",
                        "permissionDecisionReason": format!(
                            "vyrm: memory projection quarantined at {at} — grounding found \
                             divergence. Wait: resolve it first (`vyrm ground` to see the \
                             differential, `vyrm reset-projection` to recover)."
                        ),
                    }
                });
                return Ok(HookResponse {
                    stdout: decision.to_string(),
                    effectiveness: None,
                    detail: Some("denied: projection quarantined".into()),
                });
            }
            // The second wait gate is source evidence. It refreshes immediately
            // before the mutation, persists any new generation, and denies if
            // the project tree cannot be read or the stored routing state
            // cannot be trusted.
            let ready = match ensure_routing_fresh(store, root) {
                Ok(ready) => ready,
                Err(error) => {
                    let decision = serde_json::json!({
                        "hookSpecificOutput": {
                            "hookEventName": "PreToolUse",
                            "permissionDecision": "deny",
                            "permissionDecisionReason": format!(
                                "vyrm: source-routing freshness could not be established. Wait: {error}"
                            ),
                        }
                    });
                    return Ok(HookResponse {
                        stdout: decision.to_string(),
                        effectiveness: None,
                        detail: Some("denied: routing freshness unavailable".into()),
                    });
                }
            };

            let workflow_evidence =
                if input.get("tool_name").and_then(Value::as_str) == Some("Bash") {
                    let command = input
                        .pointer("/tool_input/command")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    match resolve_package_command(root, binding, command) {
                        Ok(WorkflowDecision::NotPackage) => None,
                        Ok(WorkflowDecision::Allow(authorization)) => {
                            match authorization.establish_freshness(&ready) {
                                Ok(evidence) => Some(evidence),
                                Err(differential) => {
                                    let rendered = differential.render();
                                    return Ok(deny(
                                        format!("vyrm: package workflow denied. Wait: {rendered}"),
                                        &format!("denied: {rendered}"),
                                    ));
                                }
                            }
                        }
                        Ok(WorkflowDecision::Deny(differential)) => {
                            let rendered = differential.render();
                            return Ok(deny(
                                format!("vyrm: package workflow denied. Wait: {rendered}"),
                                &format!("denied: {rendered}"),
                            ));
                        }
                        Err(error) => {
                            return Ok(deny(
                                format!(
                                "vyrm: package workflow policy cannot be trusted. Wait: {error}"
                            ),
                                "denied: workflow manifest unavailable",
                            ));
                        }
                    }
                } else {
                    None
                };
            let mut detail = format!(
                "policy allowed ({policy_evidence}); routing freshness established: {}",
                ready.render()
            );
            if let Some(evidence) = workflow_evidence {
                detail.push_str("; ");
                detail.push_str(&evidence);
            }
            Ok(HookResponse {
                detail: Some(detail),
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
                    detail: Some("ignored: vyrm control plane".into()),
                    ..HookResponse::default()
                });
            }
            let mut details = Vec::new();

            // Close the pre-tool authorization with immutable evidence. One
            // declared attempt authorizes one tool result; the resulting
            // observation moves the run to NeedsDecision, so another mutation
            // cannot ride the same declaration. Verification Bash calls are
            // similarly converted into typed pass/fail checks from exit code.
            if matches!(tool, "Edit" | "Write" | "NotebookEdit" | "Bash") {
                if let Some(run) = active_reasoning_run(store)? {
                    let encoded = serde_json::to_vec(input)?;
                    let evidence = Evidence {
                        source: tool_source(input),
                        digest: vyrm_core::digest::sha256_hex(&encoded),
                        summary: format!("{tool} hook result captured"),
                    };
                    let payload = match run.state() {
                        ReasoningState::NeedsObservation => Some(ReasoningPayload::Observation {
                            summary: format!("observed result of declared {tool} attempt"),
                            evidence: vec![evidence],
                        }),
                        ReasoningState::NeedsVerification if tool == "Bash" => {
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
                        let event = crate::reasoning::record_reasoning(
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
            if tool != "Bash" {
                return Ok(HookResponse {
                    detail: (!details.is_empty()).then(|| details.join("; ")),
                    ..HookResponse::default()
                });
            }
            let command = input
                .pointer("/tool_input/command")
                .and_then(Value::as_str)
                .unwrap_or("");
            match resolve_package_command(root, binding, command)? {
                WorkflowDecision::Allow(authorization) => {
                    let response = input.get("tool_response").unwrap_or(&Value::Null);
                    let observation = WorkflowObservation::capture(
                        &authorization,
                        command,
                        response,
                        run_exit_code(input),
                        now,
                    )?;
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
                .find_map(|s| s.run_subject(command).map(|subj| (subj, s.name)))
            else {
                return Ok(HookResponse {
                    detail: (!details.is_empty()).then(|| details.join("; ")),
                    ..HookResponse::default()
                });
            };
            let object = match run_exit_code(input) {
                Some(0) => format!("passing: {}", first_line(command)),
                Some(code) => format!("failing (exit {code}): {}", first_line(command)),
                None => format!(
                    "ran (outcome unreported by harness): {}",
                    first_line(command)
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
            vyrm_core::supersede(&previous, claim.clone())?.to_vec()
        }
        _ => vec![claim.clone()],
    };
    Ok(claims
        .into_iter()
        .map(|claim| RuntimeMutation::Claim { claim })
        .collect())
}

fn deny(reason: String, detail: &str) -> HookResponse {
    let decision = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    });
    HookResponse {
        stdout: decision.to_string(),
        effectiveness: None,
        detail: Some(detail.to_owned()),
    }
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
        .and_then(Value::as_str)
        .unwrap_or("unreported target");
    format!("{tool}:{target}")
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
