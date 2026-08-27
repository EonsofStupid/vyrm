//! `rrflow init --harness <name>`: the preflight installs itself. Turnkey
//! means the wiring is written by the tool, from the registry, and a harness
//! the registry knows to be dead refuses with the retirement stated.

use super::registry::Harness;
use super::{InstanceManifest, INSTANCE_FILE};
use rrd_core::{
    Millis, RuntimeProperties, RuntimeTraceEvent, RuntimeValue, ScopeId, TraceDataClass,
    TraceDomain, TraceLink, TraceOutcome,
};
use rrd_store::Engine;
use std::path::{Path, PathBuf};

const AGENTS_BEGIN: &str = "<!-- rrflow:begin -->";
const AGENTS_END: &str = "<!-- rrflow:end -->";

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
        "instance {} uses {:?} topology with {} declared member(s)",
        instance.id,
        instance.mode,
        instance.members.len()
    ));

    let context_path = root.join(&harness.context_file);
    write_context_block(&context_path, harness)?;
    report.written.push(context_path);

    if harness.hooks {
        let settings_path = root.join(".claude/settings.json");
        if settings_path.exists() {
            report.notes.push(format!(
                "{} exists — not overwritten. Merge the hook wiring manually:\n{}",
                settings_path.display(),
                hook_settings_json()
            ));
        } else {
            std::fs::create_dir_all(settings_path.parent().expect("settings has a parent"))?;
            std::fs::write(&settings_path, hook_settings_json())?;
            report.written.push(settings_path);
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
         - Inspect the enforced board: `rrflow --db {STORE_DIR} work-plan status`\n\
         - Sync and activate only dependency-ready work: `rrflow --db {STORE_DIR} work-plan sync --root .`\n\
           then `rrflow --db {STORE_DIR} work-plan activate --item <id>`\n\
         - Attune before mutation: `rrflow --db {STORE_DIR} preflight --root .`\n\
         - Recall before searching: `rrflow --db {STORE_DIR} recall --subject <s>`\n\
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

/// The Claude Code hook wiring. `$CLAUDE_PROJECT_DIR` keeps the command
/// correct from any working directory; the `compact` matcher on SessionStart
/// is what makes injected memory survive compaction mechanically.
fn hook_settings_json() -> String {
    let rrflow = format!("rrflow --db \"$CLAUDE_PROJECT_DIR/{STORE_DIR}\"");
    serde_json::to_string_pretty(&serde_json::json!({
        "hooks": {
            "SessionStart": [{
                "matcher": "startup|resume|compact",
                "hooks": [{ "type": "command", "command": format!("{rrflow} hook session-start --harness claude-code") }]
            }],
            "UserPromptSubmit": [{
                "hooks": [{ "type": "command", "command": format!("{rrflow} hook user-prompt-submit --harness claude-code") }]
            }],
            "PreToolUse": [{
                "matcher": "Edit|Write|NotebookEdit|Bash",
                "hooks": [{ "type": "command", "command": format!("{rrflow} hook pre-tool-use --harness claude-code") }]
            }],
            "PostToolUse": [{
                "matcher": "Edit|Write|NotebookEdit|Bash",
                "hooks": [{ "type": "command", "command": format!("{rrflow} hook post-tool-use --harness claude-code") }]
            }],
            "Stop": [{
                "hooks": [{ "type": "command", "command": format!("{rrflow} hook stop --harness claude-code") }]
            }],
            "PreCompact": [{
                "hooks": [{ "type": "command", "command": format!("{rrflow} hook pre-compact --harness claude-code") }]
            }]
        }
    }))
    .expect("static JSON serializes")
}
