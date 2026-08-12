//! Attunement: deriving a project's shape from the project itself.
//!
//! Deployment into a project must not require hand configuration. The profile is
//! detected from manifests and directory shape, so pointing the index at a new
//! repository is the whole setup step.
//!
//! Detection is evidence-based: every field records what it was derived from, so
//! a wrong profile is diagnosable rather than mysterious.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    /// TypeScript with JSX. A distinct variant because the TSX grammar is not a
    /// superset of the TypeScript grammar: each rejects syntax the other
    /// accepts, so the file extension must select the parser.
    Tsx,
    JavaScript,
    Svelte,
    Python,
    Unknown,
}

impl Language {
    /// Language implied by a file extension.
    pub fn of_path(path: &Path) -> Language {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Language::Rust,
            Some("ts") | Some("mts") => Language::TypeScript,
            Some("tsx") => Language::Tsx,
            Some("js") | Some("mjs") | Some("cjs") | Some("jsx") => Language::JavaScript,
            Some("svelte") => Language::Svelte,
            Some("py") | Some("pyi") => Language::Python,
            _ => Language::Unknown,
        }
    }

    pub fn indexable(self) -> bool {
        self != Language::Unknown
    }
}

/// What the project looks like, and the evidence for each conclusion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub root: PathBuf,
    pub languages: Vec<Language>,
    /// Manifests that were found, as the evidence for `languages`.
    pub evidence: Vec<String>,
    /// Directories excluded from indexing.
    pub excluded: Vec<String>,
    /// Files considered during attunement.
    pub sampled_files: usize,
}

/// Directories never worth indexing. Kept explicit rather than inferred, because
/// a missed exclusion here costs an entire dependency tree of noise.
const ALWAYS_EXCLUDED: &[&str] = &[
    ".git", "node_modules", "target", "dist", "build", ".svelte-kit", "vendor",
    "__pycache__", ".venv", "coverage", ".next", "out", "deps",
];

impl Profile {
    /// Attunes to a project by inspecting its manifests and file extensions.
    pub fn attune(root: &Path) -> std::io::Result<Profile> {
        let mut evidence = Vec::new();
        let mut languages: BTreeSet<Language> = BTreeSet::new();

        for (manifest, language) in [
            ("Cargo.toml", Language::Rust),
            ("package.json", Language::TypeScript),
            ("pnpm-workspace.yaml", Language::TypeScript),
            ("svelte.config.js", Language::Svelte),
            ("tsconfig.json", Language::TypeScript),
            ("pyproject.toml", Language::Python),
            ("requirements.txt", Language::Python),
            ("setup.py", Language::Python),
        ] {
            if root.join(manifest).exists() {
                evidence.push(manifest.to_string());
                languages.insert(language);
            }
        }

        // Manifests establish intent; extensions establish what is actually
        // present. A repository may declare a stack it barely uses.
        let mut sampled = 0usize;
        let mut walked = Vec::new();
        walk(root, &mut walked, 6)?;
        for path in &walked {
            let language = Language::of_path(path);
            if language.indexable() {
                languages.insert(language);
                sampled += 1;
            }
        }

        Ok(Profile {
            root: root.to_path_buf(),
            languages: languages.into_iter().collect(),
            evidence,
            excluded: ALWAYS_EXCLUDED.iter().map(|s| s.to_string()).collect(),
            sampled_files: sampled,
        })
    }

    /// Files this profile considers indexable.
    pub fn indexable_files(&self) -> std::io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        walk(&self.root, &mut out, 32)?;
        out.retain(|p| Language::of_path(p).indexable());
        out.sort();
        Ok(out)
    }

    /// Rendered profile, for an operator confirming attunement was correct.
    pub fn render(&self) -> String {
        format!(
            "attuned to {}\n  languages: {}\n  evidence:  {}\n  indexable: {} file(s)",
            self.root.display(),
            self.languages
                .iter()
                .map(|l| format!("{l:?}"))
                .collect::<Vec<_>>()
                .join(", "),
            if self.evidence.is_empty() {
                "none (inferred from file extensions only)".to_string()
            } else {
                self.evidence.join(", ")
            },
            self.sampled_files,
        )
    }
}

fn excluded(name: &str) -> bool {
    ALWAYS_EXCLUDED.contains(&name) || name.starts_with('.') && name != "."
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) -> std::io::Result<()> {
    if depth == 0 {
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // An unreadable directory is skipped rather than failing the walk: a
        // permissions problem in one subtree must not prevent indexing the rest.
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !excluded(&name) {
                walk(&path, out, depth - 1)?;
            }
        } else {
            out.push(path);
        }
    }
    Ok(())
}
