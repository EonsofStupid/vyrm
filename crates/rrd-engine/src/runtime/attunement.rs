//! Durable evidence that planning inspected the current project state.

use super::{load_project_artifacts, load_routing, RoutingReady, REASONING_SCOPE};
use rrd_core::{digest, ReadStamp, ScopeId};
use rrd_graph::ProjectProfile;
use rrd_store::{ControlTransition, Engine};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const RECEIPT_FORMAT: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningSourceFingerprint {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAttunementReceipt {
    pub format: u16,
    pub project_root: PathBuf,
    pub project_root_sha256: String,
    pub topology_sha256: String,
    pub profile_sha256: String,
    pub source_tree_sha256: String,
    pub source_files: usize,
    pub planning_sources: Vec<PlanningSourceFingerprint>,
    pub routing_generation: u64,
    pub indexed_symbols: usize,
    pub read: ReadStamp,
    pub issued_at: u64,
    pub actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorized_tool_sha256: Option<String>,
    /// Set atomically before an exact tool is started. An authorization with
    /// this field set cannot start a second process, even when an adapter
    /// retries the same request after losing its response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorized_tool_started_at: Option<u64>,
    pub receipt_sha256: String,
}

impl ProjectAttunementReceipt {
    fn seal(&mut self) {
        self.receipt_sha256.clear();
        self.receipt_sha256 = digest::sha256_hex(
            &serde_json::to_vec(self).expect("attunement receipt fields serialize"),
        );
    }

    fn verify(&self) -> bool {
        let mut candidate = self.clone();
        let expected = candidate.receipt_sha256.clone();
        candidate.seal();
        candidate.receipt_sha256 == expected
    }
}

pub fn record_attunement_receipt<E: Engine>(
    store: &E,
    root: &Path,
    ready: &RoutingReady,
    now: u64,
    actor: &str,
    prompt_sha256: Option<String>,
) -> Result<ProjectAttunementReceipt, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let mut receipt = derive_receipt(store, &root, ready, now, actor, prompt_sha256)?;
    receipt.seal();
    let key = receipt_key(&receipt.project_root_sha256);
    let expected = store.control_record(&key)?;
    let replacement = serde_json::to_vec(&receipt)?;
    let operation = digest::sha256_hex(&replacement);
    store.commit_control_transition(&ControlTransition {
        key,
        expected,
        replacement: Some(replacement),
        at: now,
        actor: actor.to_owned(),
        action: "runtime.attunement.record".into(),
        request_id: format!("attunement-{}", &operation[..16]),
        operation_id: operation,
    })?;
    Ok(receipt)
}

pub fn require_fresh_attunement<E: Engine>(
    store: &E,
    root: &Path,
    ready: &RoutingReady,
) -> Result<ProjectAttunementReceipt, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let root_sha256 = digest::sha256_hex(root.to_string_lossy().as_bytes());
    let key = receipt_key(&root_sha256);
    let bytes = store
        .control_record(&key)?
        .ok_or("no durable project-attunement receipt; run RRFlow preflight before mutation")?;
    let receipt: ProjectAttunementReceipt = serde_json::from_slice(&bytes)
        .map_err(|error| format!("project-attunement receipt is unreadable: {error}"))?;
    if receipt.format != RECEIPT_FORMAT || !receipt.verify() {
        return Err("project-attunement receipt failed format or digest verification".into());
    }
    let current = derive_receipt(store, &root, ready, receipt.issued_at, &receipt.actor, None)?;
    let mut drift = Vec::new();
    if receipt.project_root != current.project_root {
        drift.push("project_root");
    }
    if receipt.project_root_sha256 != current.project_root_sha256 {
        drift.push("project_root_sha256");
    }
    if receipt.topology_sha256 != current.topology_sha256 {
        drift.push("topology_sha256");
    }
    if receipt.profile_sha256 != current.profile_sha256 {
        drift.push("profile_sha256");
    }
    if receipt.source_tree_sha256 != current.source_tree_sha256 {
        drift.push("source_tree_sha256");
    }
    if receipt.source_files != current.source_files {
        drift.push("source_files");
    }
    if receipt.planning_sources != current.planning_sources {
        drift.push("planning_sources");
    }
    if receipt.routing_generation != current.routing_generation {
        drift.push("routing_generation");
    }
    if receipt.indexed_symbols != current.indexed_symbols {
        drift.push("indexed_symbols");
    }
    if !drift.is_empty() {
        return Err(format!(
            "project files, manifests, policy, or routing changed after planning (drift: {}); run RRFlow preflight again",
            drift.join(", ")
        )
        .into());
    }
    Ok(receipt)
}

pub fn load_attunement_receipt<E: Engine>(
    store: &E,
    root: &Path,
) -> Result<Option<ProjectAttunementReceipt>, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let root_sha256 = digest::sha256_hex(root.to_string_lossy().as_bytes());
    store
        .control_record(&receipt_key(&root_sha256))?
        .map(|bytes| {
            let receipt: ProjectAttunementReceipt = serde_json::from_slice(&bytes)?;
            if receipt.format != RECEIPT_FORMAT || !receipt.verify() {
                return Err(
                    "project-attunement receipt failed format or digest verification".into(),
                );
            }
            Ok(receipt)
        })
        .transpose()
}

pub fn authorize_attuned_tool<E: Engine>(
    store: &E,
    mut receipt: ProjectAttunementReceipt,
    reasoning_run_id: &str,
    tool_sha256: &str,
    now: u64,
    actor: &str,
) -> Result<ProjectAttunementReceipt, Box<dyn std::error::Error>> {
    if receipt
        .reasoning_run_id
        .as_deref()
        .is_some_and(|bound| bound != reasoning_run_id)
    {
        return Err(
            "attunement receipt belongs to an earlier reasoning run; run RRFlow preflight again"
                .into(),
        );
    }
    if let Some(bound) = receipt.authorized_tool_sha256.as_deref() {
        if bound == tool_sha256 {
            if receipt.authorized_tool_started_at.is_some() {
                return Err(
                    "exact-tool authorization was already consumed before execution".into(),
                );
            }
            return Ok(receipt);
        }
        return Err("attunement receipt already authorizes another unobserved tool request".into());
    }
    let expected = serde_json::to_vec(&receipt)?;
    receipt.reasoning_run_id = Some(reasoning_run_id.to_owned());
    receipt.authorized_tool_sha256 = Some(tool_sha256.to_owned());
    receipt.authorized_tool_started_at = None;
    receipt.seal();
    commit_receipt(
        store,
        receipt_key(&receipt.project_root_sha256),
        Some(expected),
        &receipt,
        now,
        actor,
        "runtime.attunement.authorize",
    )?;
    Ok(receipt)
}

/// Atomically consumes an exact-tool authorization before the external side
/// effect begins. This closes the retry race left by completing only after a
/// process returns: a crash may leave a deliberately stuck authorization, but
/// it can never cause the same permit to launch the command twice.
pub fn consume_attuned_tool_authorization<E: Engine>(
    store: &E,
    root: &Path,
    tool_sha256: &str,
    now: u64,
    actor: &str,
) -> Result<ProjectAttunementReceipt, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let root_sha256 = digest::sha256_hex(root.to_string_lossy().as_bytes());
    let key = receipt_key(&root_sha256);
    let expected = store
        .control_record(&key)?
        .ok_or("tool execution has no durable project-attunement receipt")?;
    let mut receipt: ProjectAttunementReceipt = serde_json::from_slice(&expected)?;
    if receipt.format != RECEIPT_FORMAT || !receipt.verify() {
        return Err("project-attunement receipt failed format or digest verification".into());
    }
    if receipt.authorized_tool_sha256.as_deref() != Some(tool_sha256) {
        return Err("tool request does not match the outstanding exact-tool authorization".into());
    }
    if receipt.authorized_tool_started_at.is_some() {
        return Err("exact-tool authorization was already consumed before execution".into());
    }
    receipt.authorized_tool_started_at = Some(now);
    receipt.seal();
    commit_receipt(
        store,
        key,
        Some(expected),
        &receipt,
        now,
        actor,
        "runtime.attunement.consume",
    )?;
    Ok(receipt)
}

pub fn complete_attuned_tool<E: Engine>(
    store: &E,
    root: &Path,
    tool_sha256: &str,
    now: u64,
    actor: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let root_sha256 = digest::sha256_hex(root.to_string_lossy().as_bytes());
    let key = receipt_key(&root_sha256);
    let expected = store
        .control_record(&key)?
        .ok_or("post-tool observation has no attunement authorization")?;
    let mut receipt: ProjectAttunementReceipt = serde_json::from_slice(&expected)?;
    if !receipt.verify() || receipt.authorized_tool_sha256.as_deref() != Some(tool_sha256) {
        return Err(
            "post-tool request does not match the authorized attunement tool digest".into(),
        );
    }
    receipt.authorized_tool_sha256 = None;
    receipt.authorized_tool_started_at = None;
    receipt.seal();
    commit_receipt(
        store,
        key,
        Some(expected),
        &receipt,
        now,
        actor,
        "runtime.attunement.complete",
    )?;
    Ok(())
}

pub fn require_attuned_tool_authorization<E: Engine>(
    store: &E,
    root: &Path,
    tool_sha256: &str,
) -> Result<ProjectAttunementReceipt, Box<dyn std::error::Error>> {
    let root = canonical_root(root)?;
    let root_sha256 = digest::sha256_hex(root.to_string_lossy().as_bytes());
    let bytes = store
        .control_record(&receipt_key(&root_sha256))?
        .ok_or("tool execution has no durable project-attunement receipt")?;
    let receipt: ProjectAttunementReceipt = serde_json::from_slice(&bytes)?;
    if receipt.format != RECEIPT_FORMAT || !receipt.verify() {
        return Err("project-attunement receipt failed format or digest verification".into());
    }
    if receipt.authorized_tool_sha256.as_deref() != Some(tool_sha256) {
        return Err("tool request does not match the outstanding exact-tool authorization".into());
    }
    Ok(receipt)
}

fn derive_receipt<E: Engine>(
    store: &E,
    root: &Path,
    ready: &RoutingReady,
    now: u64,
    actor: &str,
    prompt_sha256: Option<String>,
) -> Result<ProjectAttunementReceipt, Box<dyn std::error::Error>> {
    let (topology, profile) =
        load_project_artifacts(store, root)?.ok_or("project topology/profile is absent")?;
    let index = load_routing(store, root)?.ok_or("routing projection is absent")?;
    if index.generation() != ready.generation {
        return Err("routing generation changed while creating attunement receipt".into());
    }
    let root_text = root.to_string_lossy();
    let project_root_sha256 = digest::sha256_hex(root_text.as_bytes());
    if topology.digest != ready.topology_sha256 || profile.digest != ready.profile_sha256 {
        return Err("project topology/profile changed while creating attunement receipt".into());
    }
    let planning_sources = planning_source_fingerprints(root, &profile)?;
    let mut source_bytes = b"rrflow-source-tree-v1\0".to_vec();
    for file in index.files() {
        let relative = file.path.strip_prefix(root).unwrap_or(&file.path);
        let path = relative.to_string_lossy();
        source_bytes.extend_from_slice(&(path.len() as u64).to_be_bytes());
        source_bytes.extend_from_slice(path.as_bytes());
        source_bytes.extend_from_slice(&file.digest.to_be_bytes());
        source_bytes.extend_from_slice(&file.byte_len.to_be_bytes());
    }
    Ok(ProjectAttunementReceipt {
        format: RECEIPT_FORMAT,
        project_root: root.to_path_buf(),
        project_root_sha256,
        topology_sha256: topology.digest,
        profile_sha256: profile.digest,
        source_tree_sha256: digest::sha256_hex(&source_bytes),
        source_files: index.file_count(),
        planning_sources,
        routing_generation: ready.generation,
        indexed_symbols: ready.symbols,
        read: store.runtime_read_stamp(&ScopeId::new(REASONING_SCOPE)?)?,
        issued_at: now,
        actor: actor.to_owned(),
        prompt_sha256,
        reasoning_run_id: None,
        authorized_tool_sha256: None,
        authorized_tool_started_at: None,
        receipt_sha256: String::new(),
    })
}

fn planning_source_fingerprints(
    root: &Path,
    profile: &ProjectProfile,
) -> Result<Vec<PlanningSourceFingerprint>, Box<dyn std::error::Error>> {
    let mut paths = profile
        .evidence
        .iter()
        .map(|evidence| root.join(&evidence.path))
        .collect::<Vec<_>>();
    for relative in [
        "Cargo.lock",
        "bun.lock",
        "bun.lockb",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "biome.json",
        "biome.jsonc",
        "AGENTS.md",
        "CLAUDE.md",
        ".rrflow/instance.toml",
        ".rrflow/workflows.toml",
    ] {
        let path = root.join(relative);
        if path.is_file() {
            paths.push(path);
        }
    }
    let workflows = root.join(".github/workflows");
    if let Ok(entries) = std::fs::read_dir(workflows) {
        paths.extend(entries.flatten().map(|entry| entry.path()).filter(|path| {
            path.is_file()
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("yml" | "yaml")
                )
        }));
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            Ok(PlanningSourceFingerprint {
                path: relative.to_string_lossy().into_owned(),
                sha256: digest::sha256_hex(&std::fs::read(path)?),
            })
        })
        .collect()
}

fn canonical_root(root: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(std::fs::canonicalize(root)?)
}

fn receipt_key(root_sha256: &str) -> String {
    format!("server/state/runtime/attunement/{root_sha256}")
}

fn commit_receipt<E: Engine>(
    store: &E,
    key: String,
    expected: Option<Vec<u8>>,
    receipt: &ProjectAttunementReceipt,
    now: u64,
    actor: &str,
    action: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let replacement = serde_json::to_vec(receipt)?;
    let operation = digest::sha256_hex(&replacement);
    store.commit_control_transition(&ControlTransition {
        key,
        expected,
        replacement: Some(replacement),
        at: now,
        actor: actor.to_owned(),
        action: action.into(),
        request_id: format!("attunement-{}", &operation[..16]),
        operation_id: operation,
    })?;
    Ok(())
}
