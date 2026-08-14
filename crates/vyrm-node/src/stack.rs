//! Stack profiles: what the project runs on, detected from marker files, and
//! which commands count as application runs worth journaling.
//!
//! A profile is data about a toolchain, not a fork of behaviour: every stack
//! flows through the same journaling path, differing only in which commands
//! it claims. Detection is evidence-based the same way
//! `vyrm_graph::Profile::attune` is — a marker file names the conclusion.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackProfile {
    pub name: &'static str,
    /// Marker file whose presence is the detection evidence.
    pub marker: &'static str,
    /// Command prefixes journaled as application runs (`PLAN.md` Step P): a
    /// run's outcome becomes a claim, and a later run supersedes it.
    pub run_prefixes: &'static [&'static str],
}

const CARGO: StackProfile = StackProfile {
    name: "cargo",
    marker: "Cargo.toml",
    run_prefixes: &["cargo test", "cargo build", "cargo run", "cargo clippy", "cargo check"],
};

const BUN: StackProfile = StackProfile {
    name: "bun",
    marker: "bun.lock",
    run_prefixes: &["bun test", "bun run", "bun build", "bunx"],
};

const NODE: StackProfile = StackProfile {
    name: "node",
    marker: "package.json",
    run_prefixes: &[
        "npm test", "npm run", "pnpm test", "pnpm run", "yarn test", "yarn run",
    ],
};

/// Detects the stacks present at `root`. `bun.lockb` counts as the bun
/// marker too (the binary lockfile predates the textual one). A `package.json`
/// alongside a bun lockfile is bun, not node — the lockfile names the runtime.
pub fn detect(root: &Path) -> Vec<StackProfile> {
    let mut out = Vec::new();
    if root.join(CARGO.marker).exists() {
        out.push(CARGO);
    }
    let bun = root.join("bun.lock").exists() || root.join("bun.lockb").exists();
    if bun {
        out.push(BUN);
    } else if root.join(NODE.marker).exists() {
        out.push(NODE);
    }
    out
}

impl StackProfile {
    /// If `command` is one of this stack's journaled runs, the claim subject
    /// for it: the first two words joined by a dash (`cargo test --lib` →
    /// `cargo-test`), so re-runs of the same kind supersede each other.
    pub fn run_subject(&self, command: &str) -> Option<String> {
        let trimmed = command.trim_start();
        self.run_prefixes
            .iter()
            .find(|prefix| trimmed.starts_with(**prefix))
            .map(|prefix| prefix.replace(' ', "-"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_decide_and_bun_outranks_node() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect(dir.path()).is_empty());

        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let names: Vec<_> = detect(dir.path()).iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["cargo", "node"]);

        std::fs::write(dir.path().join("bun.lock"), "").unwrap();
        let names: Vec<_> = detect(dir.path()).iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["cargo", "bun"], "the lockfile names the runtime");
    }

    #[test]
    fn run_subjects_group_by_command_kind() {
        assert_eq!(CARGO.run_subject("cargo test --lib"), Some("cargo-test".into()));
        assert_eq!(CARGO.run_subject("  cargo build --release"), Some("cargo-build".into()));
        assert_eq!(CARGO.run_subject("cargo publish"), None, "unlisted commands are not runs");
        assert_eq!(BUN.run_subject("bun test src/"), Some("bun-test".into()));
    }
}
