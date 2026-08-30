//! Deterministic, provider-neutral project topology and attunement profile.
//!
//! Source routing and project discovery are two projections of the same tree:
//! routing answers where code lives, while this module retains every build
//! ecosystem, member, runner, task, CI workflow, exclusion, and evidence
//! fingerprint. The runtime persists both beside the routing index so no
//! provider adapter can publish a competing project model.

use crate::Profile;
use rrd_core::digest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

pub const PROJECT_TOPOLOGY_FORMAT: u16 = 1;
pub const PROJECT_PROFILE_FORMAT: u16 = 1;
const MAX_INSPECTED_FILE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ecosystem {
    Cargo,
    JavaScript,
    Python,
    Go,
    Jvm,
    Dotnet,
    Cmake,
    Ci,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runner {
    Cargo,
    Bun,
    Pnpm,
    Npm,
    Python,
    Uv,
    Go,
    Gradle,
    Maven,
    Dotnet,
    Cmake,
    Ctest,
    GitHubActions,
}

impl Runner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Bun => "bun",
            Self::Pnpm => "pnpm",
            Self::Npm => "npm",
            Self::Python => "python",
            Self::Uv => "uv",
            Self::Go => "go",
            Self::Gradle => "gradle",
            Self::Maven => "maven",
            Self::Dotnet => "dotnet",
            Self::Cmake => "cmake",
            Self::Ctest => "ctest",
            Self::GitHubActions => "github_actions",
        }
    }

    pub const fn executable(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Bun => "bun",
            Self::Pnpm => "pnpm",
            Self::Npm => "npm",
            Self::Python => "python",
            Self::Uv => "uv",
            Self::Go => "go",
            Self::Gradle => "gradle",
            Self::Maven => "mvn",
            Self::Dotnet => "dotnet",
            Self::Cmake => "cmake",
            Self::Ctest => "ctest",
            Self::GitHubActions => "github-actions",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Manifest,
    Lockfile,
    Workspace,
    CiWorkflow,
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopologyNodeKind {
    Repository,
    Member,
    Source,
    Manifest,
    Lockfile,
    Workspace,
    CiWorkflow,
    Policy,
    Oversized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopologyEdgeKind {
    Contains,
    Member,
    Owns,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskClass {
    Format,
    Lint,
    Check,
    Test,
    Build,
    Run,
    Package,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceFingerprint {
    pub path: String,
    pub kind: EvidenceKind,
    pub ecosystems: Vec<Ecosystem>,
    pub runners: Vec<Runner>,
    pub byte_len: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludedPath {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopologyNode {
    pub id: String,
    pub path: String,
    pub kind: TopologyNodeKind,
    pub language: Option<String>,
    pub owner_member: Option<String>,
    pub ecosystems: Vec<Ecosystem>,
    pub generated: bool,
    pub vendored: bool,
    pub byte_len: u64,
    pub sha256: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopologyEdge {
    pub from: String,
    pub to: String,
    pub kind: TopologyEdgeKind,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTopology {
    pub format: u16,
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
    pub excluded: Vec<ExcludedPath>,
    pub digest: String,
}

impl ProjectTopology {
    pub fn verify_digest(&self) -> bool {
        self.format == PROJECT_TOPOLOGY_FORMAT
            && self.digest == topology_digest(&self.nodes, &self.edges, &self.excluded)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectMember {
    pub id: String,
    pub path: String,
    pub ecosystems: Vec<Ecosystem>,
    pub runners: Vec<Runner>,
    pub manifests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTask {
    pub id: String,
    pub name: String,
    pub class: TaskClass,
    pub runner: Runner,
    pub executable: String,
    pub argv: Vec<String>,
    pub working_directory: String,
    pub source: String,
    pub allows_additional_args: bool,
    pub writes_generated_outputs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectProfile {
    pub format: u16,
    pub topology_sha256: String,
    pub ecosystems: Vec<Ecosystem>,
    pub runners: Vec<Runner>,
    pub members: Vec<ProjectMember>,
    pub tasks: Vec<ProjectTask>,
    pub ci_workflows: Vec<String>,
    pub evidence: Vec<EvidenceFingerprint>,
    pub digest: String,
}

impl ProjectProfile {
    pub fn verify_digest(&self, topology: &ProjectTopology) -> bool {
        self.format == PROJECT_PROFILE_FORMAT
            && topology.verify_digest()
            && self.topology_sha256 == topology.digest
            && self.digest
                == profile_digest(
                    &self.topology_sha256,
                    &self.ecosystems,
                    &self.runners,
                    &self.members,
                    &self.tasks,
                    &self.ci_workflows,
                    &self.evidence,
                )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectAttunement {
    pub routing_profile: Profile,
    pub topology: ProjectTopology,
    pub profile: ProjectProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopologyRefresh {
    pub topology_changed: bool,
    pub profile_changed: bool,
}

impl ProjectAttunement {
    /// Strictly materializes all project evidence without executing project
    /// commands or lifecycle scripts.
    pub fn materialize(root: &Path) -> io::Result<Self> {
        let root = std::fs::canonicalize(root).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "cannot establish project topology root {}: {error}",
                    root.display()
                ),
            )
        })?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "project topology root {} is not a directory",
                    root.display()
                ),
            ));
        }
        let scan = scan_project(&root)?;
        let (topology, profile) = derive_artifacts(&scan)?;
        let routing_profile = Profile::attune(&root)?;
        Ok(Self {
            routing_profile,
            topology,
            profile,
        })
    }

    /// Refreshes through the same deterministic materializer used by a clean
    /// full build. Differential tests require both graph and digest parity.
    pub fn refresh(&mut self, root: &Path) -> io::Result<TopologyRefresh> {
        let next = Self::materialize(root)?;
        let refresh = TopologyRefresh {
            topology_changed: self.topology.digest != next.topology.digest,
            profile_changed: self.profile.digest != next.profile.digest,
        };
        *self = next;
        Ok(refresh)
    }
}

#[derive(Debug, Clone)]
struct EvidenceDescriptor {
    kind: EvidenceKind,
    ecosystems: Vec<Ecosystem>,
    runners: Vec<Runner>,
}

#[derive(Debug)]
struct ScannedFile {
    path: String,
    byte_len: u64,
    sha256: String,
    bytes: Option<Vec<u8>>,
    evidence: Option<EvidenceDescriptor>,
    generated: bool,
    oversized: bool,
}

#[derive(Debug)]
struct ScannedProject {
    files: Vec<ScannedFile>,
    excluded: Vec<ExcludedPath>,
}

#[derive(Debug, Default)]
struct MemberBuilder {
    ecosystems: BTreeSet<Ecosystem>,
    runners: BTreeSet<Runner>,
    manifests: BTreeSet<String>,
}

fn scan_project(root: &Path) -> io::Result<ScannedProject> {
    let mut project = ScannedProject {
        files: Vec::new(),
        excluded: Vec::new(),
    };
    walk_project(root, root, &mut project)?;
    project
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    project.excluded.sort();
    project.excluded.dedup();
    Ok(project)
}

fn walk_project(root: &Path, directory: &Path, project: &mut ScannedProject) -> io::Result<()> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot establish project topology below {}: {error}",
                display_relative(root, directory)
            ),
        )
    })?;
    let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot enumerate project topology below {}: {error}",
                display_relative(root, directory)
            ),
        )
    })?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| io::Error::other("project topology contains a non-UTF-8 path"))?;
        let path = entry.path();
        let relative = relative_path(root, &path)?;
        let file_type = entry.file_type().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot inspect topology evidence {relative}: {error}"),
            )
        })?;
        let evidence = classify_evidence(&relative);

        if file_type.is_dir() {
            if evidence.is_some() {
                return Err(io::Error::other(format!(
                    "project evidence {relative} is a directory, not a readable regular file"
                )));
            }
            if let Some(reason) = excluded_directory(&relative, &name) {
                project.excluded.push(ExcludedPath {
                    path: relative,
                    reason: reason.into(),
                });
            } else {
                walk_project(root, &path, project)?;
            }
            continue;
        }

        if file_type.is_symlink() {
            if evidence.is_some() {
                return Err(io::Error::other(format!(
                    "project evidence {relative} is a symlink; attunement requires a regular file"
                )));
            }
            project.excluded.push(ExcludedPath {
                path: relative,
                reason: "symlink_not_followed".into(),
            });
            continue;
        }
        if !file_type.is_file() {
            if evidence.is_some() {
                return Err(io::Error::other(format!(
                    "project evidence {relative} is not a regular file"
                )));
            }
            project.excluded.push(ExcludedPath {
                path: relative,
                reason: "non_regular_file".into(),
            });
            continue;
        }

        let metadata = entry.metadata().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot inspect topology evidence {relative}: {error}"),
            )
        })?;
        let byte_len = metadata.len();
        let oversized = byte_len > MAX_INSPECTED_FILE_BYTES && evidence.is_none();
        let (sha256, bytes) = if oversized {
            let identity = format!("rrflow-oversized-v1\0{relative}\0{byte_len}");
            (digest::sha256_hex(identity.as_bytes()), None)
        } else {
            let bytes = std::fs::read(&path).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("cannot read project topology evidence {relative}: {error}"),
                )
            })?;
            if let Some(descriptor) = &evidence {
                validate_evidence(&relative, descriptor.kind, &bytes)?;
            }
            let sha256 = digest::sha256_hex(&bytes);
            (sha256, evidence.as_ref().map(|_| bytes))
        };
        project.files.push(ScannedFile {
            generated: generated_path(&relative),
            path: relative,
            byte_len,
            sha256,
            bytes,
            evidence,
            oversized,
        });
    }
    Ok(())
}

fn excluded_directory(relative: &str, name: &str) -> Option<&'static str> {
    if relative.starts_with(".rrflow/") {
        return Some("runtime_state");
    }
    if matches!(
        name,
        ".git"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "bin"
            | "obj"
            | ".svelte-kit"
            | "vendor"
            | "__pycache__"
            | ".venv"
            | "coverage"
            | ".next"
            | "out"
            | "deps"
            | ".gradle"
            | ".mypy_cache"
            | ".pytest_cache"
            | ".ruff_cache"
            | ".chat"
            | ".tools"
            | ".codex"
            | ".claude"
            | ".gemini"
            | ".idea"
            | ".vscode"
    ) {
        return Some("excluded_or_generated_tree");
    }
    if name.starts_with('.') && !matches!(name, ".github" | ".rrflow" | ".agents") {
        return Some("hidden_directory");
    }
    None
}

fn generated_path(path: &str) -> bool {
    path.split('/').any(|component| {
        matches!(
            component,
            "generated" | "dist" | "build" | "target" | "bin" | "obj" | "coverage"
        )
    })
}

fn classify_evidence(path: &str) -> Option<EvidenceDescriptor> {
    let file = path.rsplit('/').next()?;
    let descriptor = match file {
        "Cargo.toml" => evidence(
            EvidenceKind::Manifest,
            &[Ecosystem::Cargo],
            &[Runner::Cargo],
        ),
        "Cargo.lock" => evidence(
            EvidenceKind::Lockfile,
            &[Ecosystem::Cargo],
            &[Runner::Cargo],
        ),
        "package.json" => evidence(
            EvidenceKind::Manifest,
            &[Ecosystem::JavaScript],
            &[Runner::Npm],
        ),
        "package-lock.json" | "npm-shrinkwrap.json" => evidence(
            EvidenceKind::Lockfile,
            &[Ecosystem::JavaScript],
            &[Runner::Npm],
        ),
        "pnpm-workspace.yaml" => evidence(
            EvidenceKind::Workspace,
            &[Ecosystem::JavaScript],
            &[Runner::Pnpm],
        ),
        "pnpm-lock.yaml" => evidence(
            EvidenceKind::Lockfile,
            &[Ecosystem::JavaScript],
            &[Runner::Pnpm],
        ),
        "bun.lock" | "bun.lockb" => evidence(
            EvidenceKind::Lockfile,
            &[Ecosystem::JavaScript],
            &[Runner::Bun],
        ),
        "bunfig.toml" => evidence(
            EvidenceKind::Manifest,
            &[Ecosystem::JavaScript],
            &[Runner::Bun],
        ),
        "pyproject.toml" | "setup.py" | "requirements.txt" => evidence(
            EvidenceKind::Manifest,
            &[Ecosystem::Python],
            &[Runner::Python],
        ),
        "uv.lock" => evidence(EvidenceKind::Lockfile, &[Ecosystem::Python], &[Runner::Uv]),
        "go.mod" => evidence(EvidenceKind::Manifest, &[Ecosystem::Go], &[Runner::Go]),
        "go.work" => evidence(EvidenceKind::Workspace, &[Ecosystem::Go], &[Runner::Go]),
        "go.sum" => evidence(EvidenceKind::Lockfile, &[Ecosystem::Go], &[Runner::Go]),
        "settings.gradle" | "settings.gradle.kts" => evidence(
            EvidenceKind::Workspace,
            &[Ecosystem::Jvm],
            &[Runner::Gradle],
        ),
        "build.gradle" | "build.gradle.kts" | "gradlew" | "gradlew.bat" => {
            evidence(EvidenceKind::Manifest, &[Ecosystem::Jvm], &[Runner::Gradle])
        }
        "pom.xml" => evidence(EvidenceKind::Manifest, &[Ecosystem::Jvm], &[Runner::Maven]),
        "global.json" | "Directory.Build.props" | "Directory.Build.targets" => evidence(
            EvidenceKind::Workspace,
            &[Ecosystem::Dotnet],
            &[Runner::Dotnet],
        ),
        "CMakeLists.txt" | "CMakePresets.json" | "CMakeUserPresets.json" => evidence(
            EvidenceKind::Manifest,
            &[Ecosystem::Cmake],
            &[Runner::Cmake, Runner::Ctest],
        ),
        "AGENTS.md" | "CLAUDE.md" | "SKILL.md" | "rrflow.workplan.toml" => {
            evidence(EvidenceKind::Policy, &[], &[])
        }
        _ if matches!(
            Path::new(file).extension().and_then(|value| value.to_str()),
            Some("sln" | "slnx" | "csproj" | "fsproj" | "vbproj")
        ) =>
        {
            evidence(
                EvidenceKind::Manifest,
                &[Ecosystem::Dotnet],
                &[Runner::Dotnet],
            )
        }
        _ if path.starts_with(".github/workflows/")
            && matches!(
                Path::new(file).extension().and_then(|value| value.to_str()),
                Some("yml" | "yaml")
            ) =>
        {
            evidence(
                EvidenceKind::CiWorkflow,
                &[Ecosystem::Ci],
                &[Runner::GitHubActions],
            )
        }
        _ if path.starts_with(".rrflow/") && file.ends_with(".toml") => {
            evidence(EvidenceKind::Policy, &[], &[])
        }
        _ => return None,
    };
    Some(descriptor)
}

fn evidence(
    kind: EvidenceKind,
    ecosystems: &[Ecosystem],
    runners: &[Runner],
) -> EvidenceDescriptor {
    EvidenceDescriptor {
        kind,
        ecosystems: ecosystems.to_vec(),
        runners: runners.to_vec(),
    }
}

fn validate_evidence(path: &str, kind: EvidenceKind, bytes: &[u8]) -> io::Result<()> {
    let file = path.rsplit('/').next().unwrap_or(path);
    if file == "bun.lockb" {
        return Ok(());
    }
    let text = std::str::from_utf8(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("project evidence {path} is not UTF-8: {error}"),
        )
    })?;
    if text.as_bytes().contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("project evidence {path} contains NUL bytes"),
        ));
    }
    let invalid = |detail: &str| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("project evidence {path} is malformed: {detail}"),
        )
    };

    if matches!(
        file,
        "package.json"
            | "package-lock.json"
            | "npm-shrinkwrap.json"
            | "global.json"
            | "CMakePresets.json"
            | "CMakeUserPresets.json"
    ) {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|error| invalid(&error.to_string()))?;
        if !value.is_object() {
            return Err(invalid("JSON evidence must be an object"));
        }
    } else if matches!(
        file,
        "Cargo.toml"
            | "Cargo.lock"
            | "pyproject.toml"
            | "uv.lock"
            | "bunfig.toml"
            | "rrflow.workplan.toml"
            | "instance.toml"
    ) {
        toml::from_str::<toml::Value>(text).map_err(|error| invalid(&error.to_string()))?;
    } else if matches!(file, "pnpm-workspace.yaml" | "pnpm-lock.yaml")
        || kind == EvidenceKind::CiWorkflow
    {
        if text.trim().is_empty() || !text.lines().any(|line| line.contains(':')) {
            return Err(invalid("YAML evidence has no mapping"));
        }
    } else if file == "pom.xml" && !text.contains("<project") {
        return Err(invalid("Maven manifest has no <project> root"));
    } else if matches!(
        Path::new(file).extension().and_then(|value| value.to_str()),
        Some("csproj" | "fsproj" | "vbproj")
    ) && !text.contains("<Project")
    {
        return Err(invalid(".NET project has no <Project> root"));
    }
    Ok(())
}

fn derive_artifacts(scan: &ScannedProject) -> io::Result<(ProjectTopology, ProjectProfile)> {
    let mut members: BTreeMap<String, MemberBuilder> = BTreeMap::new();
    members.entry(".".into()).or_default();
    let mut evidence_fingerprints = Vec::new();
    let mut ecosystems = BTreeSet::new();
    let mut runners = BTreeSet::new();
    let mut ci_workflows = Vec::new();

    for file in &scan.files {
        let Some(descriptor) = &file.evidence else {
            continue;
        };
        ecosystems.extend(descriptor.ecosystems.iter().copied());
        runners.extend(descriptor.runners.iter().copied());
        if descriptor.kind == EvidenceKind::CiWorkflow {
            ci_workflows.push(file.path.clone());
        }
        evidence_fingerprints.push(EvidenceFingerprint {
            path: file.path.clone(),
            kind: descriptor.kind,
            ecosystems: descriptor.ecosystems.clone(),
            runners: descriptor.runners.clone(),
            byte_len: file.byte_len,
            sha256: file.sha256.clone(),
        });
        if is_member_manifest(&file.path, descriptor.kind) {
            let directory = parent_relative(&file.path);
            let member = members.entry(directory).or_default();
            member
                .ecosystems
                .extend(descriptor.ecosystems.iter().copied());
            member.runners.extend(descriptor.runners.iter().copied());
            member.manifests.insert(file.path.clone());
        }
    }

    // Workspace-level runners apply to every nested member of the same
    // ecosystem, but unrelated runners never erase or overwrite one another.
    for (member_path, member) in &mut members {
        for evidence in &evidence_fingerprints {
            let evidence_root = parent_relative(&evidence.path);
            if !path_within(member_path, &evidence_root) {
                continue;
            }
            for ecosystem in &evidence.ecosystems {
                if member.ecosystems.contains(ecosystem) {
                    member.runners.extend(
                        evidence
                            .runners
                            .iter()
                            .copied()
                            .filter(|runner| runner_ecosystem(*runner) == Some(*ecosystem)),
                    );
                }
            }
        }
    }

    let tree_identity = scan
        .files
        .iter()
        .map(|file| (&file.path, file.byte_len, &file.sha256))
        .collect::<Vec<_>>();
    let tree_sha256 = sealed_digest(b"rrflow-project-tree-v1\0", &tree_identity);
    let mut nodes = vec![TopologyNode {
        id: "repository:.".into(),
        path: ".".into(),
        kind: TopologyNodeKind::Repository,
        language: None,
        owner_member: None,
        ecosystems: ecosystems.iter().copied().collect(),
        generated: false,
        vendored: false,
        byte_len: scan.files.iter().map(|file| file.byte_len).sum(),
        sha256: tree_sha256,
        labels: vec!["canonical_project_root".into()],
    }];
    let mut edges = Vec::new();

    for (path, builder) in &members {
        let id = member_id(path);
        let manifest_identity = builder
            .manifests
            .iter()
            .filter_map(|manifest| {
                scan.files
                    .iter()
                    .find(|file| &file.path == manifest)
                    .map(|file| (&file.path, &file.sha256))
            })
            .collect::<Vec<_>>();
        nodes.push(TopologyNode {
            id: id.clone(),
            path: path.clone(),
            kind: TopologyNodeKind::Member,
            language: None,
            owner_member: None,
            ecosystems: builder.ecosystems.iter().copied().collect(),
            generated: false,
            vendored: false,
            byte_len: 0,
            sha256: sealed_digest(b"rrflow-project-member-v1\0", &manifest_identity),
            labels: vec!["build_member".into()],
        });
        edges.push(TopologyEdge {
            from: "repository:.".into(),
            to: id,
            kind: TopologyEdgeKind::Member,
            provenance: builder
                .manifests
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(","),
        });
    }

    let member_paths = members.keys().cloned().collect::<Vec<_>>();
    for file in &scan.files {
        let owner_path = owning_member(&file.path, &member_paths);
        let owner = member_id(&owner_path);
        let (kind, file_ecosystems, evidence_label) = match &file.evidence {
            Some(descriptor) => (
                match descriptor.kind {
                    EvidenceKind::Manifest => TopologyNodeKind::Manifest,
                    EvidenceKind::Lockfile => TopologyNodeKind::Lockfile,
                    EvidenceKind::Workspace => TopologyNodeKind::Workspace,
                    EvidenceKind::CiWorkflow => TopologyNodeKind::CiWorkflow,
                    EvidenceKind::Policy => TopologyNodeKind::Policy,
                },
                descriptor.ecosystems.clone(),
                Some(format!("evidence:{:?}", descriptor.kind).to_lowercase()),
            ),
            None if file.oversized => (TopologyNodeKind::Oversized, Vec::new(), None),
            None => (TopologyNodeKind::Source, Vec::new(), None),
        };
        let mut labels = Vec::new();
        if let Some(label) = evidence_label {
            labels.push(label);
        }
        if file.generated {
            labels.push("generated".into());
        }
        if file.oversized {
            labels.push("content_not_inspected".into());
        }
        let id = format!("file:{}", file.path);
        nodes.push(TopologyNode {
            id: id.clone(),
            path: file.path.clone(),
            kind,
            language: language_name(&file.path).map(str::to_owned),
            owner_member: Some(owner.clone()),
            ecosystems: file_ecosystems,
            generated: file.generated,
            vendored: false,
            byte_len: file.byte_len,
            sha256: file.sha256.clone(),
            labels,
        });
        edges.push(TopologyEdge {
            from: owner,
            to: id,
            kind: TopologyEdgeKind::Owns,
            provenance: file.path.clone(),
        });
    }
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    edges.sort();
    let digest = topology_digest(&nodes, &edges, &scan.excluded);
    let topology = ProjectTopology {
        format: PROJECT_TOPOLOGY_FORMAT,
        nodes,
        edges,
        excluded: scan.excluded.clone(),
        digest,
    };

    let project_members = members
        .into_iter()
        .map(|(path, builder)| ProjectMember {
            id: member_id(&path),
            path,
            ecosystems: builder.ecosystems.into_iter().collect(),
            runners: builder.runners.into_iter().collect(),
            manifests: builder.manifests.into_iter().collect(),
        })
        .collect::<Vec<_>>();
    let tasks = derive_tasks(scan, &project_members)?;
    ci_workflows.sort();
    ci_workflows.dedup();
    evidence_fingerprints.sort_by(|left, right| left.path.cmp(&right.path));
    let ecosystems = ecosystems.into_iter().collect::<Vec<_>>();
    let runners = runners.into_iter().collect::<Vec<_>>();
    let profile_digest = profile_digest(
        &topology.digest,
        &ecosystems,
        &runners,
        &project_members,
        &tasks,
        &ci_workflows,
        &evidence_fingerprints,
    );
    let profile = ProjectProfile {
        format: PROJECT_PROFILE_FORMAT,
        topology_sha256: topology.digest.clone(),
        ecosystems,
        runners,
        members: project_members,
        tasks,
        ci_workflows,
        evidence: evidence_fingerprints,
        digest: profile_digest,
    };
    Ok((topology, profile))
}

fn derive_tasks(scan: &ScannedProject, members: &[ProjectMember]) -> io::Result<Vec<ProjectTask>> {
    let mut tasks = BTreeMap::new();
    for member in members {
        for runner in &member.runners {
            for (name, class, argv) in default_tasks(*runner) {
                insert_task(
                    &mut tasks,
                    member,
                    *runner,
                    name,
                    class,
                    argv.iter().map(|value| (*value).to_owned()).collect(),
                    member
                        .manifests
                        .first()
                        .cloned()
                        .unwrap_or_else(|| ".".into()),
                );
            }
        }
    }

    for file in &scan.files {
        if file.path.rsplit('/').next() != Some("package.json") {
            continue;
        }
        let bytes = file
            .bytes
            .as_deref()
            .ok_or_else(|| io::Error::other("package manifest bytes were not retained"))?;
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(io::Error::other)?;
        let Some(scripts) = value.get("scripts").and_then(serde_json::Value::as_object) else {
            continue;
        };
        let member_path = owning_member(
            &file.path,
            &members
                .iter()
                .map(|member| member.path.clone())
                .collect::<Vec<_>>(),
        );
        let Some(member) = members.iter().find(|member| member.path == member_path) else {
            continue;
        };
        for name in scripts.keys() {
            for runner in member
                .runners
                .iter()
                .copied()
                .filter(|runner| matches!(runner, Runner::Bun | Runner::Pnpm | Runner::Npm))
            {
                insert_task(
                    &mut tasks,
                    member,
                    runner,
                    name,
                    classify_script(name),
                    vec!["run".into(), name.clone()],
                    file.path.clone(),
                );
            }
        }
    }
    Ok(tasks.into_values().collect())
}

fn default_tasks(runner: Runner) -> Vec<(&'static str, TaskClass, &'static [&'static str])> {
    match runner {
        Runner::Cargo => vec![
            ("fmt", TaskClass::Format, &["fmt", "--all"]),
            ("clippy", TaskClass::Lint, &["clippy", "--all-targets"]),
            ("check", TaskClass::Check, &["check"]),
            ("test", TaskClass::Test, &["test"]),
            ("build", TaskClass::Build, &["build"]),
        ],
        Runner::Bun => vec![("test", TaskClass::Test, &["test"])],
        Runner::Pnpm | Runner::Npm => Vec::new(),
        Runner::Python => vec![("test", TaskClass::Test, &["-m", "pytest"])],
        Runner::Uv => vec![
            ("sync", TaskClass::Build, &["sync"]),
            ("test", TaskClass::Test, &["run", "pytest"]),
        ],
        Runner::Go => vec![
            ("test", TaskClass::Test, &["test", "./..."]),
            ("build", TaskClass::Build, &["build", "./..."]),
        ],
        Runner::Gradle => vec![
            ("test", TaskClass::Test, &["test"]),
            ("build", TaskClass::Build, &["build"]),
        ],
        Runner::Maven => vec![
            ("test", TaskClass::Test, &["test"]),
            ("package", TaskClass::Package, &["package"]),
        ],
        Runner::Dotnet => vec![
            ("test", TaskClass::Test, &["test"]),
            ("build", TaskClass::Build, &["build"]),
        ],
        Runner::Cmake => vec![("build", TaskClass::Build, &["--build", "build"])],
        Runner::Ctest => vec![("test", TaskClass::Test, &["--test-dir", "build"])],
        Runner::GitHubActions => Vec::new(),
    }
}

fn insert_task(
    tasks: &mut BTreeMap<String, ProjectTask>,
    member: &ProjectMember,
    runner: Runner,
    name: &str,
    class: TaskClass,
    argv: Vec<String>,
    source: String,
) {
    let id = format!(
        "{}:{}:{}",
        member.id,
        runner.as_str(),
        canonical_fragment(name)
    );
    tasks.insert(
        id.clone(),
        ProjectTask {
            id,
            name: name.to_owned(),
            class,
            runner,
            executable: runner.executable().into(),
            argv,
            working_directory: member.path.clone(),
            source,
            allows_additional_args: true,
            writes_generated_outputs: matches!(
                class,
                TaskClass::Build | TaskClass::Run | TaskClass::Package
            ),
        },
    );
}

fn classify_script(name: &str) -> TaskClass {
    let lowered = name.to_ascii_lowercase();
    if lowered.contains("format") || lowered == "fmt" {
        TaskClass::Format
    } else if lowered.contains("lint") {
        TaskClass::Lint
    } else if lowered.contains("typecheck") || lowered == "check" {
        TaskClass::Check
    } else if lowered.contains("test") {
        TaskClass::Test
    } else if lowered.contains("build") {
        TaskClass::Build
    } else if lowered.contains("pack") || lowered.contains("publish") {
        TaskClass::Package
    } else {
        TaskClass::Run
    }
}

fn is_member_manifest(path: &str, kind: EvidenceKind) -> bool {
    if kind != EvidenceKind::Manifest {
        return false;
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    matches!(
        file,
        "Cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "go.mod"
            | "build.gradle"
            | "build.gradle.kts"
            | "pom.xml"
            | "CMakeLists.txt"
    ) || matches!(
        Path::new(file).extension().and_then(|value| value.to_str()),
        Some("sln" | "slnx" | "csproj" | "fsproj" | "vbproj")
    )
}

fn runner_ecosystem(runner: Runner) -> Option<Ecosystem> {
    match runner {
        Runner::Cargo => Some(Ecosystem::Cargo),
        Runner::Bun | Runner::Pnpm | Runner::Npm => Some(Ecosystem::JavaScript),
        Runner::Python | Runner::Uv => Some(Ecosystem::Python),
        Runner::Go => Some(Ecosystem::Go),
        Runner::Gradle | Runner::Maven => Some(Ecosystem::Jvm),
        Runner::Dotnet => Some(Ecosystem::Dotnet),
        Runner::Cmake | Runner::Ctest => Some(Ecosystem::Cmake),
        Runner::GitHubActions => Some(Ecosystem::Ci),
    }
}

fn member_id(path: &str) -> String {
    format!("member:{path}")
}

fn parent_relative(path: &str) -> String {
    path.rsplit_once('/')
        .map_or_else(|| ".".into(), |(parent, _)| parent.into())
}

fn path_within(path: &str, ancestor: &str) -> bool {
    ancestor == "." || path == ancestor || path.starts_with(&format!("{ancestor}/"))
}

fn owning_member(file: &str, members: &[String]) -> String {
    let parent = parent_relative(file);
    members
        .iter()
        .filter(|member| path_within(&parent, member))
        .max_by_key(|member| member.len())
        .cloned()
        .unwrap_or_else(|| ".".into())
}

fn relative_path(root: &Path, path: &Path) -> io::Result<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::Error::other("project topology path escaped its canonical root"))?;
    let mut components = Vec::new();
    for component in relative.components() {
        let std::path::Component::Normal(value) = component else {
            return Err(io::Error::other(
                "project topology contains a non-canonical path",
            ));
        };
        components.push(
            value
                .to_str()
                .ok_or_else(|| io::Error::other("project topology contains a non-UTF-8 path"))?,
        );
    }
    Ok(components.join("/"))
}

fn display_relative(root: &Path, path: &Path) -> String {
    relative_path(root, path).unwrap_or_else(|_| path.display().to_string())
}

fn language_name(path: &str) -> Option<&'static str> {
    match Path::new(path).extension().and_then(|value| value.to_str()) {
        Some("rs") => Some("rust"),
        Some("ts" | "mts" | "tsx") => Some("typescript"),
        Some("js" | "mjs" | "cjs" | "jsx") => Some("javascript"),
        Some("py" | "pyi") => Some("python"),
        Some("go") => Some("go"),
        Some("java") => Some("java"),
        Some("kt" | "kts") => Some("kotlin"),
        Some("cs") => Some("csharp"),
        Some("fs" | "fsx") => Some("fsharp"),
        Some("c" | "h") => Some("c"),
        Some("cc" | "cpp" | "cxx" | "hpp" | "hxx") => Some("cpp"),
        Some("svelte") => Some("svelte"),
        Some("toml") => Some("toml"),
        Some("json") => Some("json"),
        Some("yml" | "yaml") => Some("yaml"),
        Some("xml") => Some("xml"),
        _ => None,
    }
}

fn canonical_fragment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
            out.push(character);
            separator = false;
        } else if !separator && !out.is_empty() {
            out.push('-');
            separator = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "unknown".into()
    } else {
        out
    }
}

fn topology_digest(
    nodes: &[TopologyNode],
    edges: &[TopologyEdge],
    excluded: &[ExcludedPath],
) -> String {
    sealed_digest(
        b"rrflow-project-topology-v1\0",
        &(PROJECT_TOPOLOGY_FORMAT, nodes, edges, excluded),
    )
}

#[allow(clippy::too_many_arguments)]
fn profile_digest(
    topology_sha256: &str,
    ecosystems: &[Ecosystem],
    runners: &[Runner],
    members: &[ProjectMember],
    tasks: &[ProjectTask],
    ci_workflows: &[String],
    evidence: &[EvidenceFingerprint],
) -> String {
    sealed_digest(
        b"rrflow-project-profile-v1\0",
        &(
            PROJECT_PROFILE_FORMAT,
            topology_sha256,
            ecosystems,
            runners,
            members,
            tasks,
            ci_workflows,
            evidence,
        ),
    )
}

fn sealed_digest(prefix: &[u8], value: &impl Serialize) -> String {
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(
        &serde_json::to_vec(value).expect("canonical project artifact fields serialize"),
    );
    digest::sha256_hex(&bytes)
}
