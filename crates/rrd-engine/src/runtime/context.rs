//! Provider-neutral task context assembly and its persisted evidence receipt.
//!
//! This module consumes the topology and source index already owned by
//! rrd-graph plus the claim/runtime state already owned by RRD. It is derived
//! projection state, not another project model or storage authority.

use super::{
    load_project_artifacts, load_routing, LifecycleRiskV1, LifecycleTaskKindV1,
    ProjectAttunementReceipt, RoutingReady, WorkflowPreflight, REASONING_SCOPE,
};
use rrd_core::{digest, Claim, ReadStamp, RecallSet, ScopeId, Subject};
use rrd_graph::{EvidenceKind, RoutedFile};
use rrd_store::{Engine, ProjectionStatus};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const TASK_PREFLIGHT_RECEIPT_FORMAT: u16 = 1;
pub const TASK_PREFLIGHT_RECEIPT_PROJECTION: &str = "task-preflight-receipt-v1";
pub const GOLDEN_PATTERN_SELECTION_PROJECTION: &str = "golden-pattern-selection-v1";
pub const DEFAULT_SOURCE_LINE_BUDGET: usize = 2_000;
const MAX_TASK_SUBJECTS: usize = 64;
const RECEIPT_TTL_MS: u64 = 15 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceDisposition {
    Current,
    Empty,
    Truncated,
    Stale,
    Unavailable,
}

impl EvidenceDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Empty => "empty",
            Self::Truncated => "truncated",
            Self::Stale => "stale",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskClassification {
    pub task: LifecycleTaskKindV1,
    pub risk: LifecycleRiskV1,
    pub classification_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSourceRoute {
    pub path: String,
    pub lines: usize,
    pub matched_terms: Vec<String>,
    pub justification: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedReadStamp {
    pub purpose: String,
    pub read: ReadStamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskPreflightReceipt {
    pub format: u16,
    pub project_root: String,
    pub task_sha256: String,
    pub classification: TaskClassification,
    pub topology_sha256: String,
    pub profile_sha256: String,
    pub source_tree_sha256: String,
    pub route_sha256: String,
    pub routing_generation: u64,
    pub route_disposition: EvidenceDisposition,
    pub selected_routes: usize,
    pub route_candidates: usize,
    pub pattern_sha256: String,
    pub pattern_disposition: EvidenceDisposition,
    pub policy_sha256: String,
    pub policy_disposition: EvidenceDisposition,
    pub projection_sha256: String,
    pub projection_disposition: EvidenceDisposition,
    pub projection_watermark: u64,
    pub claim_sequence: u64,
    pub recall_sha256: String,
    pub recall_disposition: EvidenceDisposition,
    pub selected_claims: usize,
    pub token_budget: usize,
    pub source_line_budget: usize,
    pub rrd_reads_sha256: String,
    pub rrd_reads: Vec<NamedReadStamp>,
    pub context_sha256: String,
    pub context_bytes: usize,
    pub issued_at: u64,
    pub expires_at: u64,
    pub receipt_sha256: String,
}

impl TaskPreflightReceipt {
    fn seal(&mut self) {
        self.receipt_sha256.clear();
        self.receipt_sha256 = sealed_digest(b"rrflow-task-preflight-receipt-v1\0", self);
    }

    pub fn verify(&self) -> bool {
        if self.format != TASK_PREFLIGHT_RECEIPT_FORMAT
            || self.project_root.is_empty()
            || self.task_sha256.len() != 64
            || self.context_sha256.len() != 64
            || self.expires_at <= self.issued_at
            || self.rrd_reads.is_empty()
        {
            return false;
        }
        let expected = self.receipt_sha256.clone();
        let mut candidate = self.clone();
        candidate.seal();
        expected == candidate.receipt_sha256
    }
}

#[derive(Debug)]
pub(crate) struct TaskSubjectSelection {
    pub subjects: Vec<Subject>,
    pub truncated: bool,
}

#[derive(Debug)]
pub(crate) struct TaskContextAssembly {
    pub rendered: String,
    pub receipt: TaskPreflightReceipt,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
struct RouteCandidate {
    route: RoutedFile,
    relative_path: String,
    matched_terms: BTreeSet<String>,
}

#[derive(Debug)]
struct TaskRouteResult {
    routes: Vec<ContextSourceRoute>,
    candidate_count: usize,
    disposition: EvidenceDisposition,
    sha256: String,
}

pub fn classify_task(task: &str) -> TaskClassification {
    let terms = task_terms(task);
    let contains = |values: &[&str]| values.iter().any(|value| terms.contains(*value));
    let task_kind = if contains(&[
        "implement",
        "build",
        "change",
        "edit",
        "fix",
        "refactor",
        "code",
        "patch",
    ]) {
        LifecycleTaskKindV1::CodeChange
    } else if contains(&["diagnose", "investigate", "inspect", "review", "research"]) {
        LifecycleTaskKindV1::Investigation
    } else if contains(&["query", "search", "recall", "lookup", "find"]) {
        LifecycleTaskKindV1::Query
    } else if contains(&["deploy", "start", "stop", "backup", "restore", "operate"]) {
        LifecycleTaskKindV1::Operation
    } else if contains(&["maintain", "upgrade", "migrate", "cleanup"]) {
        LifecycleTaskKindV1::Maintenance
    } else {
        LifecycleTaskKindV1::Other
    };
    let risk = if contains(&[
        "delete",
        "drop",
        "destroy",
        "force",
        "secret",
        "credential",
        "reset",
        "purge",
    ]) {
        LifecycleRiskV1::High
    } else {
        match task_kind {
            LifecycleTaskKindV1::Investigation | LifecycleTaskKindV1::Query => {
                LifecycleRiskV1::ReadOnly
            }
            LifecycleTaskKindV1::CodeChange
            | LifecycleTaskKindV1::Operation
            | LifecycleTaskKindV1::Maintenance => LifecycleRiskV1::Medium,
            LifecycleTaskKindV1::Other => LifecycleRiskV1::Low,
        }
    };
    let classification_sha256 = sealed_digest(
        b"rrflow-task-classification-v1\0",
        &(task_kind, risk, &terms),
    );
    TaskClassification {
        task: task_kind,
        risk,
        classification_sha256,
    }
}

pub(crate) fn select_task_subjects<E: Engine>(
    store: &E,
    task: &str,
) -> Result<TaskSubjectSelection, Box<dyn std::error::Error>> {
    let terms = task_terms(task);
    let mut ranked = store
        .subjects()?
        .into_iter()
        .filter_map(|subject| {
            let subject_terms = task_terms(subject.as_str());
            let score = subject_terms.intersection(&terms).count();
            (score > 0).then_some((score, subject))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.as_str().cmp(right.1.as_str()))
    });
    let truncated = ranked.len() > MAX_TASK_SUBJECTS;
    ranked.truncate(MAX_TASK_SUBJECTS);
    Ok(TaskSubjectSelection {
        subjects: ranked.into_iter().map(|(_, subject)| subject).collect(),
        truncated,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn assemble_task_context<E: Engine>(
    store: &E,
    root: &Path,
    task: &str,
    ready: &RoutingReady,
    attunement: &ProjectAttunementReceipt,
    workflows: &[WorkflowPreflight],
    recall: &RecallSet,
    subject_selection_truncated: bool,
    now: u64,
    token_budget: usize,
) -> Result<TaskContextAssembly, Box<dyn std::error::Error>> {
    let root = std::fs::canonicalize(root)?;
    let classification = classify_task(task);
    let task_sha256 = sealed_digest(b"rrflow-task-v1\0", &task);
    let (topology, profile) =
        load_project_artifacts(store, &root)?.ok_or("project topology/profile is absent")?;
    if topology.digest != attunement.topology_sha256
        || profile.digest != attunement.profile_sha256
        || ready.topology_sha256 != attunement.topology_sha256
        || ready.profile_sha256 != attunement.profile_sha256
    {
        return Err("task context topology/profile is stale against attunement".into());
    }
    let index = load_routing(store, &root)?.ok_or("source routing index is absent")?;
    if index.generation() != ready.generation {
        return Err("source routing generation changed during context assembly".into());
    }

    let TaskRouteResult {
        routes,
        candidate_count: route_candidates,
        disposition: route_disposition,
        sha256: route_sha256,
    } = route_task(
        &index,
        &root,
        task,
        ready.generation,
        DEFAULT_SOURCE_LINE_BUDGET,
    )?;
    let recall_disposition = if recall.truncated || subject_selection_truncated {
        EvidenceDisposition::Truncated
    } else if recall.claims.is_empty() {
        EvidenceDisposition::Empty
    } else {
        EvidenceDisposition::Current
    };

    let policies = profile
        .evidence
        .iter()
        .filter(|evidence| evidence.kind == EvidenceKind::Policy)
        .cloned()
        .collect::<Vec<_>>();
    let policy_disposition = if policies.is_empty() {
        EvidenceDisposition::Empty
    } else {
        EvidenceDisposition::Current
    };
    let policy_sha256 = sealed_digest(
        b"rrflow-task-policy-evidence-v1\0",
        &(&profile.digest, &policies),
    );

    let (pattern_sha256, pattern_disposition) =
        match store.get_projection(GOLDEN_PATTERN_SELECTION_PROJECTION)? {
            Some(bytes) if !bytes.is_empty() => (
                sealed_digest(b"rrflow-pattern-selection-binding-v1\0", &bytes),
                EvidenceDisposition::Current,
            ),
            Some(_) => (
                sealed_digest(b"rrflow-pattern-selection-binding-v1\0", &"empty"),
                EvidenceDisposition::Stale,
            ),
            None => (
                sealed_digest(b"rrflow-pattern-selection-binding-v1\0", &"unavailable"),
                EvidenceDisposition::Unavailable,
            ),
        };

    let projection = store.current_projection()?;
    let claim_sequence = store.sequence()?;
    let projection_disposition =
        if matches!(&projection.status, ProjectionStatus::Quarantined { .. })
            || projection.watermark < claim_sequence
        {
            EvidenceDisposition::Stale
        } else {
            EvidenceDisposition::Current
        };
    let projection_sha256 = sealed_digest(
        b"rrflow-current-projection-binding-v1\0",
        &(
            &projection.status,
            projection.watermark,
            projection.last_grounded,
            claim_sequence,
        ),
    );

    let mut rrd_reads = vec![
        NamedReadStamp {
            purpose: "attunement".into(),
            read: attunement.read.clone(),
        },
        NamedReadStamp {
            purpose: "task-context".into(),
            read: store.runtime_read_stamp(&ScopeId::new(REASONING_SCOPE)?)?,
        },
    ];
    rrd_reads.extend(workflows.iter().map(|workflow| NamedReadStamp {
        purpose: format!("workflow:{}", workflow.event),
        read: workflow.read.clone(),
    }));
    rrd_reads.sort_by(|left, right| {
        left.purpose
            .cmp(&right.purpose)
            .then_with(|| left.read.manifest_id.cmp(&right.read.manifest_id))
    });
    rrd_reads.dedup();
    let rrd_reads_sha256 = sealed_digest(b"rrflow-task-read-stamps-v1\0", &rrd_reads);

    let mut lines = vec![
        format!(
            "[rrflow] task context: task={} class={:?} risk={:?}",
            &task_sha256[..12],
            classification.task,
            classification.risk
        ),
        format!(
            "[rrflow] evidence: route={} recall={} pattern={} policy={} projection={}",
            route_disposition.as_str(),
            recall_disposition.as_str(),
            pattern_disposition.as_str(),
            policy_disposition.as_str(),
            projection_disposition.as_str(),
        ),
    ];
    for route in &routes {
        lines.push(format!(
            "[rrflow] source route: {} ({} line(s); terms={}; {})",
            route.path,
            route.lines,
            route.matched_terms.join(","),
            route.justification
        ));
    }
    if routes.is_empty() {
        lines.push("[rrflow] EXCLUDED source route: no task-relevant indexed file".into());
    }
    if route_disposition == EvidenceDisposition::Truncated {
        lines.push(format!(
            "[rrflow] EXCLUDED source route: budget retained {} of {} candidate file(s)",
            routes.len(),
            route_candidates
        ));
    }
    for claim in &recall.claims {
        lines.push(render_claim(claim));
    }
    if recall.claims.is_empty() {
        lines.push("[rrflow] EXCLUDED recall: no task-relevant claim".into());
    }
    if recall_disposition == EvidenceDisposition::Truncated {
        lines.push("[rrflow] EXCLUDED recall: token or subject budget truncated evidence".into());
    }
    if pattern_disposition != EvidenceDisposition::Current {
        lines.push(format!(
            "[rrflow] EXCLUDED pattern: selection evidence is {}",
            pattern_disposition.as_str()
        ));
    }
    if policy_disposition == EvidenceDisposition::Empty {
        lines.push("[rrflow] EXCLUDED policy: no project policy evidence".into());
    }
    if projection_disposition == EvidenceDisposition::Stale {
        lines.push(format!(
            "[rrflow] STALE projection: watermark={} claim_sequence={}",
            projection.watermark, claim_sequence
        ));
    }
    let rendered = lines.join("\n");
    let context_sha256 = sealed_digest(b"rrflow-task-context-v1\0", &rendered);
    let mut receipt = TaskPreflightReceipt {
        format: TASK_PREFLIGHT_RECEIPT_FORMAT,
        project_root: root.to_string_lossy().into_owned(),
        task_sha256,
        classification,
        topology_sha256: attunement.topology_sha256.clone(),
        profile_sha256: attunement.profile_sha256.clone(),
        source_tree_sha256: attunement.source_tree_sha256.clone(),
        route_sha256,
        routing_generation: ready.generation,
        route_disposition,
        selected_routes: routes.len(),
        route_candidates,
        pattern_sha256,
        pattern_disposition,
        policy_sha256,
        policy_disposition,
        projection_sha256,
        projection_disposition,
        projection_watermark: projection.watermark,
        claim_sequence,
        recall_sha256: recall.digest.clone(),
        recall_disposition,
        selected_claims: recall.claims.len(),
        token_budget,
        source_line_budget: DEFAULT_SOURCE_LINE_BUDGET,
        rrd_reads_sha256,
        rrd_reads,
        context_sha256,
        context_bytes: rendered.len(),
        issued_at: now,
        expires_at: now.saturating_add(RECEIPT_TTL_MS),
        receipt_sha256: String::new(),
    };
    receipt.seal();
    store.put_projection(
        TASK_PREFLIGHT_RECEIPT_PROJECTION,
        &serde_json::to_vec(&receipt)?,
    )?;

    let mut warnings = Vec::new();
    if route_disposition == EvidenceDisposition::Truncated {
        warnings.push("task source-route evidence was truncated by the line budget".into());
    }
    if recall_disposition == EvidenceDisposition::Truncated {
        warnings.push("task recall evidence was truncated by the context budget".into());
    }
    if projection_disposition == EvidenceDisposition::Stale {
        warnings.push(format!(
            "current-state projection is stale at {} behind claim sequence {}; authoritative recall remains explicit",
            projection.watermark, claim_sequence
        ));
    }
    Ok(TaskContextAssembly {
        rendered,
        receipt,
        warnings,
    })
}

pub fn load_task_preflight_receipt<E: Engine>(
    store: &E,
) -> Result<Option<TaskPreflightReceipt>, Box<dyn std::error::Error>> {
    store
        .get_projection(TASK_PREFLIGHT_RECEIPT_PROJECTION)?
        .map(|bytes| {
            let receipt: TaskPreflightReceipt = serde_json::from_slice(&bytes)
                .map_err(|error| format!("task-preflight receipt is unreadable: {error}"))?;
            if !receipt.verify() {
                return Err("task-preflight receipt failed digest verification".into());
            }
            Ok(receipt)
        })
        .transpose()
}

fn route_task(
    index: &rrd_graph::Index,
    root: &Path,
    task: &str,
    generation: u64,
    line_budget: usize,
) -> Result<TaskRouteResult, Box<dyn std::error::Error>> {
    let terms = task_terms(task);
    let mut by_path: BTreeMap<String, RouteCandidate> = BTreeMap::new();
    for term in &terms {
        for route in index.route(term, usize::MAX) {
            let relative = route.path.strip_prefix(root).unwrap_or(&route.path);
            let relative_path = relative.to_string_lossy().replace('\\', "/");
            let entry = by_path
                .entry(relative_path.clone())
                .or_insert_with(|| RouteCandidate {
                    route: route.clone(),
                    relative_path,
                    matched_terms: BTreeSet::new(),
                });
            entry.matched_terms.insert(term.clone());
            if route.score > entry.route.score
                || route.score == entry.route.score && route.centrality > entry.route.centrality
            {
                entry.route = route;
            }
        }
    }
    let mut candidates = by_path.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .route
            .score
            .total_cmp(&left.route.score)
            .then_with(|| right.route.centrality.total_cmp(&left.route.centrality))
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    let all_routes = candidates.iter().map(normalize_route).collect::<Vec<_>>();
    let candidate_count = all_routes.len();
    let mut selected = Vec::new();
    let mut spent = 0usize;
    for candidate in candidates {
        if selected.is_empty() || spent.saturating_add(candidate.route.lines) <= line_budget {
            spent = spent.saturating_add(candidate.route.lines);
            selected.push(normalize_route(&candidate));
        }
    }
    let disposition = if selected.len() < candidate_count {
        EvidenceDisposition::Truncated
    } else if selected.is_empty() {
        EvidenceDisposition::Empty
    } else {
        EvidenceDisposition::Current
    };
    let route_sha256 = sealed_digest(
        b"rrflow-task-source-route-v1\0",
        &(
            generation,
            digest::sha256_hex(&index.to_bytes()),
            &terms,
            &all_routes,
            &selected,
            line_budget,
            disposition,
        ),
    );
    Ok(TaskRouteResult {
        routes: selected,
        candidate_count,
        disposition,
        sha256: route_sha256,
    })
}

fn normalize_route(candidate: &RouteCandidate) -> ContextSourceRoute {
    ContextSourceRoute {
        path: candidate.relative_path.clone(),
        lines: candidate.route.lines,
        matched_terms: candidate.matched_terms.iter().cloned().collect(),
        justification: candidate.route.justification.render(),
    }
}

fn task_terms(value: &str) -> BTreeSet<String> {
    value
        .to_lowercase()
        .split(|character: char| !(character.is_alphanumeric() || matches!(character, '-' | '_')))
        .filter(|term| term.len() >= 3)
        .map(str::to_owned)
        .collect()
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

fn sealed_digest(prefix: &[u8], value: &impl Serialize) -> String {
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(
        &serde_json::to_vec(value).expect("task-context evidence fields serialize"),
    );
    digest::sha256_hex(&bytes)
}
