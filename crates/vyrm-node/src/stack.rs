//! Stack profiles: what the project runs on, detected from marker files, and
//! which commands count as application runs worth journaling.
//!
//! A profile is data about a toolchain, not a fork of behaviour: every stack
//! flows through the same journaling path, differing only in which commands
//! it claims. Detection is evidence-based the same way
//! `vyrm_graph::Profile::attune` is — a marker file names the conclusion.
//!
//! The profile set and its priority order are **measured, not assumed**: a
//! census of the operator's 258 repositories (2026-08-14, recorded in
//! `PLAN.md` Step P) found Rust the most active stack (50 of 52 pushed in
//! 2026), React+Vite the dominant frontend (33 active repos, `vite.config.*`
//! in 51 roots), bun the leading JS package manager (39 lockfiles vs 18 npm,
//! 16 pnpm), TanStack Start already at 11 repos, Python at 13 active with
//! `uv.lock` appearing, CMake C++ at 4 and Go at 3. Next.js appeared twice
//! and gets no profile yet — a stated omission, not an oversight.
//!
//! Command vocabularies follow the 2026 toolchains: `uv run pytest`/`ruff`
//! for Python (the uv+ruff consolidation), `vitest`/`vite build` for the
//! Vite ecosystem (TanStack Start is a Vite plugin since the vinxi
//! migration; Vite 8 stable 2026-03), `ctest`/`cmake --build`/`--workflow`
//! presets for C++, `go test`/`gotestsum` for Go.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackProfile {
    pub name: &'static str,
    /// Marker files any one of which is the detection evidence.
    pub markers: &'static [&'static str],
    /// Command prefixes journaled as application runs (`PLAN.md` Step P): a
    /// run's outcome becomes a claim, and a later run supersedes it.
    pub run_prefixes: &'static [&'static str],
}

const CARGO: StackProfile = StackProfile {
    name: "cargo",
    markers: &["Cargo.toml"],
    run_prefixes: &["cargo test", "cargo build", "cargo run", "cargo clippy", "cargo check", "cargo nextest"],
};

const BUN: StackProfile = StackProfile {
    name: "bun",
    markers: &["bun.lock", "bun.lockb"],
    run_prefixes: &["bun test", "bun run", "bun build", "bunx"],
};

const NODE: StackProfile = StackProfile {
    name: "node",
    markers: &["package.json"],
    run_prefixes: &[
        "npm test", "npm run", "pnpm test", "pnpm run", "yarn test", "yarn run", "npx",
    ],
};

const PYTHON: StackProfile = StackProfile {
    name: "python",
    markers: &["pyproject.toml", "uv.lock", "requirements.txt", "setup.py"],
    run_prefixes: &[
        "uv run", "uv sync", "pytest", "python -m pytest", "ruff check", "ruff format", "mypy",
    ],
};

const GO: StackProfile = StackProfile {
    name: "go",
    markers: &["go.mod"],
    run_prefixes: &["go test", "go build", "go run", "go vet", "gotestsum"],
};

const CPP: StackProfile = StackProfile {
    name: "cpp",
    markers: &["CMakeLists.txt", "meson.build"],
    run_prefixes: &["cmake --build", "cmake --workflow", "ctest", "ninja", "meson test", "make"],
};

/// Not a runtime: the Vite tool layer, which rides on bun or node. Detected
/// separately so its runners are journaled even when invoked bare.
const VITE: StackProfile = StackProfile {
    name: "vite",
    markers: &["vite.config.ts", "vite.config.js", "vite.config.mts", "vitest.config.ts"],
    run_prefixes: &["vitest", "vite build", "vite dev", "vite preview", "playwright test", "tsc"],
};

/// Detects the stacks present at `root`, in census priority order. A
/// `package.json` alongside a bun lockfile is bun, not node — the lockfile
/// names the runtime.
pub fn detect(root: &Path) -> Vec<StackProfile> {
    let present = |profile: &StackProfile| profile.markers.iter().any(|m| root.join(m).exists());
    let mut out = Vec::new();
    if present(&CARGO) {
        out.push(CARGO);
    }
    if present(&BUN) {
        out.push(BUN);
    } else if present(&NODE) {
        out.push(NODE);
    }
    if present(&VITE) {
        out.push(VITE);
    }
    if present(&PYTHON) {
        out.push(PYTHON);
    }
    if present(&GO) {
        out.push(GO);
    }
    if present(&CPP) {
        out.push(CPP);
    }
    out
}

/// The framework facet, read from `package.json` dependencies: which
/// frontend shape this repo takes, orthogonal to the runtime that serves it
/// (the census found TanStack Start on bun and React+Vite on npm alike).
/// Unreadable or absent manifests yield nothing — detection degrades, never
/// errors.
pub fn frameworks(root: &Path) -> Vec<&'static str> {
    let Ok(raw) = std::fs::read_to_string(root.join("package.json")) else {
        return Vec::new();
    };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let has = |name: &str| {
        ["dependencies", "devDependencies"]
            .iter()
            .filter_map(|k| pkg.get(k))
            .filter_map(|d| d.as_object())
            .any(|d| d.contains_key(name))
    };
    let mut out = Vec::new();
    if has("@tanstack/react-start") || has("@tanstack/start") {
        out.push("tanstack-start");
    } else if has("next") {
        out.push("next");
    } else if has("react") && has("vite") {
        out.push("react-vite");
    } else if has("react") {
        out.push("react");
    }
    out
}

impl StackProfile {
    /// If `command` is one of this stack's journaled runs, the claim subject
    /// for it: the matched prefix with spaces dashed (`cargo test --lib` →
    /// `cargo-test`), so re-runs of the same kind supersede each other. A
    /// prefix matches only at a word boundary — `make` must not claim
    /// `makepkg`.
    pub fn run_subject(&self, command: &str) -> Option<String> {
        let trimmed = command.trim_start();
        self.run_prefixes
            .iter()
            .find(|prefix| {
                trimmed.strip_prefix(**prefix).is_some_and(|rest| {
                    rest.is_empty() || rest.starts_with(char::is_whitespace)
                })
            })
            .map(|prefix| prefix.replace(' ', "-").replace("--", ""))
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
    fn the_census_stacks_detect_from_their_markers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("uv.lock"), "").unwrap();
        std::fs::write(dir.path().join("go.mod"), "module x").unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();
        std::fs::write(dir.path().join("vite.config.ts"), "").unwrap();
        let names: Vec<_> = detect(dir.path()).iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["vite", "python", "go", "cpp"]);
    }

    #[test]
    fn run_subjects_group_by_command_kind_at_word_boundaries() {
        assert_eq!(CARGO.run_subject("cargo test --lib"), Some("cargo-test".into()));
        assert_eq!(CARGO.run_subject("  cargo build --release"), Some("cargo-build".into()));
        assert_eq!(CARGO.run_subject("cargo publish"), None, "unlisted commands are not runs");
        assert_eq!(PYTHON.run_subject("uv run pytest -x"), Some("uv-run".into()));
        assert_eq!(PYTHON.run_subject("pytest tests/"), Some("pytest".into()));
        assert_eq!(GO.run_subject("go test ./..."), Some("go-test".into()));
        assert_eq!(CPP.run_subject("cmake --build build"), Some("cmake-build".into()));
        assert_eq!(CPP.run_subject("make -j8"), Some("make".into()));
        assert_eq!(CPP.run_subject("makepkg -si"), None, "word boundary: make must not claim makepkg");
        assert_eq!(VITE.run_subject("vitest run"), Some("vitest".into()));
        assert_eq!(BUN.run_subject("bun test src/"), Some("bun-test".into()));
    }

    #[test]
    fn the_framework_facet_reads_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        assert!(frameworks(dir.path()).is_empty(), "no manifest, no facet");

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"react": "^19", "vite": "^8"}}"#,
        )
        .unwrap();
        assert_eq!(frameworks(dir.path()), vec!["react-vite"]);

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"@tanstack/react-start": "^1", "react": "^19", "vite": "^8"}}"#,
        )
        .unwrap();
        assert_eq!(
            frameworks(dir.path()),
            vec!["tanstack-start"],
            "the meta-framework outranks its own parts"
        );

        std::fs::write(dir.path().join("package.json"), "not json").unwrap();
        assert!(frameworks(dir.path()).is_empty(), "corrupt manifests degrade, never error");
    }
}
