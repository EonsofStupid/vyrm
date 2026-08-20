//! `vyrm init --harness <name>`: the preflight installs itself. Turnkey
//! means the wiring is written by the tool, from the registry, and a harness
//! the registry knows to be dead refuses with the retirement stated.

use crate::registry::Harness;
use crate::{InstanceManifest, INSTANCE_FILE};
use std::path::{Path, PathBuf};

const AGENTS_BEGIN: &str = "<!-- vyrm:begin -->";
const AGENTS_END: &str = "<!-- vyrm:end -->";

/// Store directory, relative to the project root. A vyrm store is a
/// directory, so isolation per project is the filesystem.
pub const STORE_DIR: &str = ".vyrm/store";

#[derive(Debug, Default)]
pub struct InitReport {
    pub written: Vec<PathBuf>,
    /// Degradations and manual steps, stated rather than silent.
    pub notes: Vec<String>,
}

/// Wires `root` for `harness`. Errors on a retired harness; otherwise writes
/// the context-file block, plus hook wiring where the harness supports it.
pub fn init(root: &Path, harness: &Harness) -> Result<InitReport, Box<dyn std::error::Error>> {
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
        "add `{}` (and `.vyrm/`) to .gitignore: the store is per-checkout state",
        STORE_DIR
    ));
    Ok(report)
}

/// Writes or replaces the marker-delimited vyrm block in the harness's
/// context file. Idempotent: a second init replaces the block in place.
fn write_context_block(path: &Path, harness: &Harness) -> std::io::Result<()> {
    let block = format!(
        "{AGENTS_BEGIN}\n\
         ## vyrm memory\n\n\
         This project has a vyrm memory store at `{STORE_DIR}`: bi-temporal\n\
         claims with provenance, recalled by subject. Current facts cost ~10x\n\
         fewer tokens than re-reading documents (measured; see the vyrm repo).\n\n\
         - Recall before searching: `vyrm --db {STORE_DIR} recall --subject <s>`\n\
         - Record what you decide: `vyrm --db {STORE_DIR} assert --subject <s> --predicate <p> --object <text>`\n\
         - If a vyrm gate denies a tool call, the memory projection is\n\
           quarantined: wait, then run `vyrm --db {STORE_DIR} ground` and follow\n\
           its instructions rather than working around the denial.\n\
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
    let vyrm = format!("vyrm --db \"$CLAUDE_PROJECT_DIR/{STORE_DIR}\"");
    serde_json::to_string_pretty(&serde_json::json!({
        "hooks": {
            "SessionStart": [{
                "matcher": "startup|resume|compact",
                "hooks": [{ "type": "command", "command": format!("{vyrm} hook session-start --harness claude-code") }]
            }],
            "UserPromptSubmit": [{
                "hooks": [{ "type": "command", "command": format!("{vyrm} hook user-prompt-submit --harness claude-code") }]
            }],
            "PreToolUse": [{
                "matcher": "Edit|Write|NotebookEdit|Bash",
                "hooks": [{ "type": "command", "command": format!("{vyrm} hook pre-tool-use --harness claude-code") }]
            }],
            "PostToolUse": [{
                "matcher": "Bash",
                "hooks": [{ "type": "command", "command": format!("{vyrm} hook post-tool-use --harness claude-code") }]
            }],
            "Stop": [{
                "hooks": [{ "type": "command", "command": format!("{vyrm} hook stop --harness claude-code") }]
            }],
            "PreCompact": [{
                "hooks": [{ "type": "command", "command": format!("{vyrm} hook pre-compact --harness claude-code") }]
            }]
        }
    }))
    .expect("static JSON serializes")
}
