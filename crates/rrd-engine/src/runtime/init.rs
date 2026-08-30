//! `rrflow init --harness <name>`: the preflight installs itself. Turnkey
//! means the wiring is written by the tool, from the registry, and a harness
//! the registry knows to be dead refuses with the retirement stated.

use super::registry::{Harness, HookProtocol};
use super::{InstanceManifest, INSTANCE_FILE};
use rrd_core::{
    Millis, RuntimeProperties, RuntimeTraceEvent, RuntimeValue, ScopeId, TraceDataClass,
    TraceDomain, TraceLink, TraceOutcome,
};
use rrd_store::Engine;
use std::path::{Path, PathBuf};

const AGENTS_BEGIN: &str = "<!-- rrflow:begin -->";
const AGENTS_END: &str = "<!-- rrflow:end -->";
const ENGINE_SKILL: &str = include_str!("templates/rrflow-engine-development.SKILL.md");
const ENGINE_SKILL_PATH: &str = ".agents/skills/rrflow-engine-development/SKILL.md";

/// Store directory, relative to the project root. RRD state is a directory,
/// so isolation per project is the filesystem.
pub const STORE_DIR: &str = ".rrflow/rrd";

#[derive(Debug, Default)]
pub struct InitReport {
    pub written: Vec<PathBuf>,
    /// Degradations and manual steps, stated rather than silent.
    pub notes: Vec<String>,
}

/// Wires `root` for `harness`. Errors on a retired harness; otherwise writes
/// the context-file block, plus hook wiring where the harness supports it.
pub fn init<E: Engine>(
    store: &E,
    root: &Path,
    harness: &Harness,
    at: Millis,
) -> Result<InitReport, Box<dyn std::error::Error>> {
    if let Some(when) = &harness.retired {
        return Err(format!(
            "harness {} was retired ({when}); refusing to wire a dead harness. \
             The registry keeps it as history only.",
            harness.name
        )
        .into());
    }

    let mut report = InitReport::default();

    let (instance, created) = InstanceManifest::ensure_dedicated(root)?;
    if created {
        report.written.push(root.join(INSTANCE_FILE));
    }
    report.notes.push(format!(
        "instance {} is bound to exactly one project root",
        instance.id
    ));

    let context_path = root.join(&harness.context_file);
    write_context_block(&context_path, harness)?;
    report.written.push(context_path);

    let skill_path = root.join(ENGINE_SKILL_PATH);
    write_file_with_parents(&skill_path, ENGINE_SKILL)?;
    report.written.push(skill_path);
    if harness.name == "claude-code" {
        let claude_skill = root.join(".claude/skills/rrflow-engine-development/SKILL.md");
        write_file_with_parents(&claude_skill, ENGINE_SKILL)?;
        report.written.push(claude_skill);
    }

    if let Some(protocol) = harness.hook_protocol {
        let relative = harness
            .config_path
            .as_deref()
            .ok_or("hooks-capable harness has no project config_path")?;
        let settings_path = root.join(relative);
        merge_hook_settings(
            &settings_path,
            &hook_settings(protocol, &harness.name),
            &harness.name,
        )?;
        report.written.push(settings_path.clone());
        report.notes.push(format!(
            "{} native lifecycle installed in {}",
            harness.display,
            settings_path.display()
        ));
        if protocol == HookProtocol::Codex {
            report.notes.push(
                "Codex project hooks are hash-trusted: open `/hooks`, review this project definition, and trust it; changed hooks are skipped until re-trusted"
                    .into(),
            );
        }
        if protocol == HookProtocol::Copilot {
            report.notes.push(
                "Copilot command-hook timeouts are fail-open; use rrflow exec for full mutation enforcement"
                    .into(),
            );
        }
    } else {
        for degradation in harness.degradations() {
            report.notes.push(degradation);
        }
    }

    report.notes.push(format!(
        "add `/{}/` and `/.rrflow/verification/` to .gitignore: keep `{}` tracked, but keep the store and verification output per-checkout",
        STORE_DIR,
        INSTANCE_FILE,
    ));

    let scope = ScopeId::new(super::REASONING_SCOPE)?;
    let read = store.runtime_read_stamp(&scope)?;
    let at_bytes = at.to_be_bytes();
    let cursor_bytes = read.commit_cursor.to_be_bytes();
    let identity = super::TraceIdentity::derive(&[
        instance.id.as_bytes(),
        harness.name.as_bytes(),
        &at_bytes,
        &cursor_bytes,
    ])?;
    let trace = RuntimeTraceEvent::annotation(
        identity.trace_id,
        identity.span_id,
        None,
        TraceDomain::Lifecycle,
        "instance.init",
        at,
        TraceOutcome::Ok,
        TraceDataClass::Control,
        vec![TraceLink::Read { stamp: read }],
        RuntimeProperties::from([
            (
                "instance_id".into(),
                RuntimeValue::String(instance.id.clone()),
            ),
            ("harness".into(), RuntimeValue::String(harness.name.clone())),
            ("manifest_created".into(), RuntimeValue::Bool(created)),
            (
                "files_written".into(),
                RuntimeValue::Unsigned(report.written.len() as u64),
            ),
        ]),
    )?;
    let outcome = super::record_runtime_trace(store, &scope, "operator:init", trace)?;
    report.notes.push(format!(
        "runtime trace contract ready at cursor {}",
        outcome.last_cursor
    ));
    Ok(report)
}

/// Writes or replaces the marker-delimited RRFlow block in the harness's
/// context file. Idempotent: a second init replaces the block in place.
fn write_context_block(path: &Path, harness: &Harness) -> std::io::Result<()> {
    let block = format!(
        "{AGENTS_BEGIN}\n\
         ## RRFlow project context\n\n\
         This project has provider-neutral RRFlow runtime state at `{STORE_DIR}`.\n\
         RRD persists bi-temporal claims with provenance, project routing,\n\
         reasoning lifecycle evidence, and exact tool authorization.\n\n\
         - Load the `rrflow-engine-development` skill from `{ENGINE_SKILL_PATH}` for project work.\n\
         - Recall before broad search, then inspect the current code, tests, worktree diff, and enforced board.\n\
         - Sync and activate only dependency-ready work: `rrflow --db {STORE_DIR} work-plan sync --root .`\n\
           then `rrflow --db {STORE_DIR} work-plan activate --item <id>`.\n\
         - Persist a reviewed goal and plan, then attune with `rrflow --db {STORE_DIR} preflight --root .`.\n\
         - Record one exact attempt before each mutation; observe and decide before the next attempt.\n\
         - Record what you decide: `rrflow --db {STORE_DIR} assert --subject <s> --predicate <p> --object <text>`\n\
         - If an RRFlow gate denies a tool call, follow the reported recovery\n\
           action. For projection divergence, run `rrflow --db {STORE_DIR} ground`\n\
           and review its evidence before `rrflow --db {STORE_DIR} reset-projection`.\n\
         {AGENTS_END}"
    );
    let content = match std::fs::read_to_string(path) {
        Ok(existing) => match (existing.find(AGENTS_BEGIN), existing.find(AGENTS_END)) {
            (Some(start), Some(end)) if end > start => {
                let mut updated = existing.clone();
                updated.replace_range(start..end + AGENTS_END.len(), &block);
                updated
            }
            _ => format!("{existing}\n\n{block}\n"),
        },
        Err(_) => format!("# {}\n\n{block}\n", harness.display),
    };
    std::fs::write(path, content)
}

fn write_file_with_parents(path: &Path, content: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().expect("generated file has a parent"))?;
    std::fs::write(path, content)
}

/// Merge only RRFlow-owned matcher groups. Unrelated settings and hooks are
/// preserved, while re-running init replaces stale RRFlow commands exactly.
fn merge_hook_settings(
    path: &Path,
    generated: &serde_json::Value,
    harness: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut document = if path.exists() {
        serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(path)?)?
    } else {
        serde_json::json!({})
    };
    let document_object = document
        .as_object_mut()
        .ok_or("hook settings root must be a JSON object")?;
    let hooks = document_object
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or("hook settings `hooks` must be a JSON object")?;
    let generated_hooks = generated
        .get("hooks")
        .and_then(serde_json::Value::as_object)
        .expect("generated hooks are an object");
    let owned = format!("--harness {harness}");
    for (event, groups) in generated_hooks {
        let destination = hooks
            .entry(event.clone())
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("hook event {event} must be an array"))?;
        destination.retain(|group| !group.to_string().contains(&owned));
        destination.extend(
            groups
                .as_array()
                .expect("generated groups are arrays")
                .clone(),
        );
    }
    write_file_with_parents(path, &serde_json::to_string_pretty(&document)?)?;
    Ok(())
}

fn hook_settings(protocol: HookProtocol, harness: &str) -> serde_json::Value {
    let root = match protocol {
        HookProtocol::ClaudeCode => "$CLAUDE_PROJECT_DIR".to_owned(),
        HookProtocol::Codex | HookProtocol::Gemini | HookProtocol::Copilot => {
            "$(git rev-parse --show-toplevel)".to_owned()
        }
    };
    let rrflow = format!("{} --db \"{root}/{STORE_DIR}\"", hook_executable());
    let command =
        |event: &str| format!("{rrflow} hook {event} --harness {harness} --root \"{root}\"");
    let codex_handler = |event: &str, status: &str, timeout: u64| {
        serde_json::json!({
            "type": "command",
            "command": command(event),
            "timeout": timeout,
            "statusMessage": status
        })
    };
    let claude_handler = |event: &str, timeout: u64| serde_json::json!({"type": "command", "command": command(event), "timeout": timeout});
    let gemini_handler = |event: &str, name: &str, timeout_ms: u64| {
        serde_json::json!({
            "type": "command",
            "command": command(event),
            "timeout": timeout_ms,
            "name": name,
            "description": "RRFlow provider-neutral lifecycle enforcement"
        })
    };
    let copilot_handler = |event: &str, timeout_sec: u64| {
        serde_json::json!({
            "type": "command",
            "bash": command(event),
            "timeoutSec": timeout_sec
        })
    };
    match protocol {
        HookProtocol::ClaudeCode => serde_json::json!({"hooks": {
            "SessionStart": [{"matcher": "startup|resume|compact", "hooks": [claude_handler("session-start", 90)]}],
            "UserPromptSubmit": [{"hooks": [claude_handler("user-prompt-submit", 60)]}],
            "PreToolUse": [{"matcher": "Edit|Write|NotebookEdit|Bash|mcp__.*", "hooks": [claude_handler("pre-tool-use", 60)]}],
            "PostToolUse": [{"matcher": "Edit|Write|NotebookEdit|Bash|mcp__.*", "hooks": [claude_handler("post-tool-use", 90)]}],
            "Stop": [{"hooks": [claude_handler("stop", 30)]}],
            "PreCompact": [{"hooks": [claude_handler("pre-compact", 30)]}],
            "SessionEnd": [{"hooks": [claude_handler("session-end", 3)]}]
        }}),
        HookProtocol::Codex => serde_json::json!({"hooks": {
            "SessionStart": [{"matcher": "startup|resume|compact", "hooks": [codex_handler("session-start", "Loading RRFlow project state", 90)]}],
            "UserPromptSubmit": [{"hooks": [codex_handler("user-prompt-submit", "Recalling RRFlow evidence", 60)]}],
            "PreToolUse": [{"matcher": "Bash|apply_patch|Edit|Write|RRFlowExec|mcp__.*", "hooks": [codex_handler("pre-tool-use", "Authorizing project mutation", 60)]}],
            "PostToolUse": [{"matcher": "Bash|apply_patch|Edit|Write|RRFlowExec|mcp__.*", "hooks": [codex_handler("post-tool-use", "Recording tool evidence", 90)]}],
            "Stop": [{"hooks": [codex_handler("stop", "Checking RRFlow turn state", 30)]}],
            "PreCompact": [{"hooks": [codex_handler("pre-compact", "Preserving RRFlow state", 30)]}],
            "SessionEnd": [{"hooks": [codex_handler("session-end", "Closing RRFlow session", 3)]}]
        }}),
        HookProtocol::Gemini => serde_json::json!({"hooks": {
            "SessionStart": [{"hooks": [gemini_handler("session-start", "rrflow-session-start", 90_000)]}],
            "BeforeAgent": [{"hooks": [gemini_handler("user-prompt-submit", "rrflow-before-agent", 60_000)]}],
            "BeforeTool": [{"matcher": ".*", "hooks": [gemini_handler("pre-tool-use", "rrflow-before-tool", 60_000)]}],
            "AfterTool": [{"matcher": ".*", "hooks": [gemini_handler("post-tool-use", "rrflow-after-tool", 90_000)]}],
            "AfterAgent": [{"hooks": [gemini_handler("stop", "rrflow-after-agent", 30_000)]}],
            "PreCompress": [{"hooks": [gemini_handler("pre-compact", "rrflow-pre-compress", 30_000)]}],
            "SessionEnd": [{"hooks": [gemini_handler("session-end", "rrflow-session-end", 3_000)]}]
        }}),
        HookProtocol::Copilot => serde_json::json!({"hooks": {
            "sessionStart": [copilot_handler("session-start", 90)],
            "userPromptSubmitted": [copilot_handler("user-prompt-submit", 60)],
            "preToolUse": [{"type": "command", "matcher": ".*", "bash": command("pre-tool-use"), "timeoutSec": 60}],
            "postToolUse": [{"type": "command", "matcher": ".*", "bash": command("post-tool-use"), "timeoutSec": 90}],
            "agentStop": [copilot_handler("stop", 30)],
            "preCompact": [copilot_handler("pre-compact", 30)],
            "sessionEnd": [copilot_handler("session-end", 3)]
        }}),
    }
}

/// Prefer the exact running rrflow executable. Source checkouts frequently
/// build into a shared Cargo target directory that is intentionally absent
/// from PATH; emitting bare `rrflow` there would make a blocking hook fail
/// open at the harness edge. Packaged embedders can set an explicit path.
fn hook_executable() -> String {
    if let Ok(path) = std::env::var("RRFLOW_HOOK_EXECUTABLE") {
        if !path.trim().is_empty() {
            return shell_quote(&path);
        }
    }
    if let Ok(path) = std::env::current_exe() {
        if path
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "rrflow")
        {
            return shell_quote(&path.to_string_lossy());
        }
    }
    "rrflow".into()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_engine_skill_matches_the_initializer_template() {
        let checked_in = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.agents/skills/rrflow-engine-development/SKILL.md");
        assert_eq!(std::fs::read_to_string(checked_in).unwrap(), ENGINE_SKILL);
    }
}
