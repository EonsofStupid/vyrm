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
use crate::stack;
use serde_json::Value;
use vyrm_core::{recall, Claim, Millis, Predicate, Producer, Reader, RecallQuery, Subject};
use vyrm_store::{Effectiveness, ProjectionStatus, RecallOutcome, Store};

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
pub struct HookContext<'a> {
    pub store: &'a Store,
    pub root: &'a std::path::Path,
    pub harness: Option<&'a str>,
    pub reader: &'a Reader,
    pub now: Millis,
    pub budget: usize,
}

/// Dispatches one event. `input` is the harness JSON from stdin.
pub fn handle(
    ctx: &HookContext<'_>,
    event: HookEvent,
    input: &Value,
) -> Result<HookResponse, Box<dyn std::error::Error>> {
    let HookContext { store, root, harness, reader, now, budget } = *ctx;
    match event {
        HookEvent::SessionStart => {
            let Preflight { context, effectiveness, warnings, .. } =
                preflight(store, root, harness, reader, now, budget)?;
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
            let query = RecallQuery { subjects: matched, predicates: None, as_of: now };
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
                query: query.subjects.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","),
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
            // The wait gate: a quarantined projection makes mutation wait,
            // as an enforced decision rather than advice the model may skip.
            let mutating = matches!(
                input.get("tool_name").and_then(Value::as_str),
                Some("Edit" | "Write" | "NotebookEdit" | "Bash")
            );
            if !mutating {
                return Ok(HookResponse::default());
            }
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
            Ok(HookResponse::default())
        }

        HookEvent::PostToolUse => {
            // The application journal: a run's outcome becomes a claim, and
            // the next run of the same kind supersedes it. Retirement by
            // supersession, exactly as every other claim.
            if input.get("tool_name").and_then(Value::as_str) != Some("Bash") {
                return Ok(HookResponse::default());
            }
            let command = input
                .pointer("/tool_input/command")
                .and_then(Value::as_str)
                .unwrap_or("");
            let Some((subject, stack_name)) = stack::detect(root)
                .iter()
                .find_map(|s| s.run_subject(command).map(|subj| (subj, s.name)))
            else {
                return Ok(HookResponse::default());
            };
            let object = match run_exit_code(input) {
                Some(0) => format!("passing: {}", first_line(command)),
                Some(code) => format!("failing (exit {code}): {}", first_line(command)),
                None => format!("ran (outcome unreported by harness): {}", first_line(command)),
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
            Ok(HookResponse {
                stdout: String::new(),
                effectiveness: None,
                detail: Some(format!("journaled {stack_name} run: {subject} = {object}")),
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

/// Subjects whose name appears in the prompt as a whole word
/// (case-insensitive). Substring matching would recall `repo` for
/// "repository"; word boundaries keep recall answerable for what it injects.
fn matched_subjects(
    store: &Store,
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
