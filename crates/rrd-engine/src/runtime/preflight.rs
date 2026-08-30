//! The moment of attunement. `PLAN.md` Step P: an agent lands in a
//! repository and the memory layer is already in the loop — the preflight
//! detects the stack, checks the estate's health and the adapter's
//! verification, and emits a budgeted recall of everything currently held
//! true, rendered for injection into the model's context before reasoning
//! starts.
//!
//! Wired to a harness's session-start event, this is what makes "memory
//! survives compaction" mechanical: the harness re-fires the event after
//! compaction and the preflight re-injects.

use super::context::{assemble_task_context, select_task_subjects};
use super::registry::{Registry, Verification};
use super::routing::{ensure_routing_fresh, RoutingReady};
use super::stack;
use super::workflow::{WorkflowCatalog, WorkflowPreflight, WORKFLOW_FILE};
use super::InstanceBinding;
use super::{
    read_work_plan, record_attunement_receipt, sync_project_work_plan, TaskPreflightReceipt,
};
use rrd_core::{digest, recall, Millis, Reader, RecallQuery};
use rrd_store::{Effectiveness, Engine, ProjectionStatus, RecallOutcome};

/// What a preflight produced. `context` is the injectable text; everything
/// else is the evidence behind it.
#[derive(Debug)]
pub struct Preflight {
    /// Effective task used for source and knowledge routing. Explicit prompts
    /// win; otherwise the active work item supplies the task.
    pub task: String,
    pub stacks: Vec<&'static str>,
    /// Persisted source-routing state established before recall is injected.
    /// `None` means freshness could not be established and `warnings` says
    /// why; the pre-tool gate will deny mutation under the same condition.
    pub routing: Option<RoutingReady>,
    /// Durable evidence binding the inspected project tree and planning inputs
    /// to the RRD state visible during this preflight.
    pub attunement: Option<super::ProjectAttunementReceipt>,
    /// Persisted receipt for the bounded task-specific context packet.
    pub context_receipt: Option<TaskPreflightReceipt>,
    /// Authoritative execution board when the project carries a checked-in
    /// RRFlow work plan.
    pub work_plan: Option<super::WorkPlanSnapshot>,
    /// Declared workflow events and the exact runtime read state captured
    /// before context injection.
    pub workflows: Vec<WorkflowPreflight>,
    /// Estate and adapter warnings, already included in `context`.
    pub warnings: Vec<String>,
    /// The rendered injection: warnings, then recalled claims with
    /// provenance.
    pub context: String,
    /// `SPEC.md` §13.1 fields for the invocation record. A preflight has no
    /// baseline arm, so the reduction is unverified by definition.
    pub effectiveness: Effectiveness,
}

/// Runs the preflight. `harness` names the adapter in use, if known, so its
/// verification state can make noise; `budget` is the recall token budget.
#[tracing::instrument(level = "debug", skip_all)]
pub fn preflight<E: Engine>(
    store: &E,
    root: &std::path::Path,
    harness: Option<&str>,
    reader: &Reader,
    now: Millis,
    budget: usize,
) -> Result<Preflight, Box<dyn std::error::Error>> {
    preflight_task(store, root, harness, reader, now, budget, None)
}

/// Runs the same preflight with an explicit prompt/task. All adapters call this
/// path rather than maintaining their own subject matching or source routing.
#[tracing::instrument(level = "debug", skip_all)]
pub fn preflight_task<E: Engine>(
    store: &E,
    root: &std::path::Path,
    harness: Option<&str>,
    reader: &Reader,
    now: Millis,
    budget: usize,
    requested_task: Option<&str>,
) -> Result<Preflight, Box<dyn std::error::Error>> {
    let binding = InstanceBinding::discover(root)?;
    binding.require_runtime_ready()?;
    let root = binding.project_root.as_path();

    // Runtime stacks plus the framework facet: `bun+vite+tanstack-start`
    // tells the agent what it landed in before it reads a single file.
    let mut stacks: Vec<&'static str> = stack::detect(root).iter().map(|s| s.name).collect();
    stacks.extend(stack::frameworks(root));
    let mut warnings = Vec::new();
    let mut workflows = Vec::new();

    match WorkflowCatalog::load(root) {
        Ok(Some(catalog)) => {
            for rule in &catalog.manifest.workflows {
                if rule.scope != binding.manifest.id {
                    warnings.push(format!(
                        "workflow {} declares scope {:?}, but this instance is {:?}",
                        rule.event, rule.scope, binding.manifest.id
                    ));
                    continue;
                }
                let scope = rrd_core::ScopeId::new(rule.scope.clone())?;
                workflows.push(WorkflowPreflight {
                    event: rule.event.clone(),
                    manifest_digest: catalog.digest.clone(),
                    read: store.runtime_read_stamp(&scope)?,
                });
            }
        }
        Ok(None) if stacks.iter().any(|stack| matches!(*stack, "bun" | "node")) => {
            warnings.push(format!(
                "package workflow policy is absent; package commands are denied until {WORKFLOW_FILE} declares them"
            ));
        }
        Ok(None) => {}
        Err(error) => warnings.push(format!(
            "package workflow policy cannot be trusted and package commands are denied: {error}"
        )),
    }

    // Source routing is part of attunement, not an optional command the model
    // must remember to run. Preflight remains available for recall if this
    // fails, but makes the failure loud; the pre-tool barrier fails closed.
    let routing = match ensure_routing_fresh(store, root) {
        Ok(ready) => Some(ready),
        Err(error) => {
            warnings.push(format!(
                "source-routing freshness could not be established: {error}"
            ));
            None
        }
    };
    let attunement = match routing.as_ref() {
        Some(ready) => {
            match record_attunement_receipt(store, root, ready, now, reader.as_str(), None) {
                Ok(receipt) => Some(receipt),
                Err(error) => {
                    warnings.push(format!(
                        "project-attunement receipt could not be persisted: {error}"
                    ));
                    None
                }
            }
        }
        None => None,
    };
    let work_plan = match sync_project_work_plan(store, root, now, reader.as_str()) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            warnings.push(format!(
                "checked-in work plan is unreadable or changed without a reviewed revision: {error}"
            ));
            None
        }
    };
    let task = resolve_task(root, requested_task, work_plan.as_ref());

    // Estate health: a quarantined projection is surfaced, and recall
    // proceeds from the authoritative claims keyspace regardless — the gate
    // (`hook.rs`) is what blocks, the preflight is what informs.
    if let ProjectionStatus::Quarantined { at, .. } = store.current_projection()?.status {
        warnings.push(format!(
            "memory projection quarantined at {at}: grounding found divergence; \
             mutations are gated until `rrflow reset-projection`"
        ));
    }

    // The drift alarm: the adapter's verification claim either resolves at
    // `now` or it makes noise. `resolve_as_of` is the scheduler.
    let registry = Registry::builtin();
    if let Some(name) = harness {
        match registry.get(name) {
            None => warnings.push(format!("harness {name:?} is not in the registry")),
            Some(adapter) => {
                if let Some(when) = &adapter.retired {
                    warnings.push(format!("harness {name} was retired ({when})"));
                }
                match registry.verification(store, adapter, now)? {
                    Verification::Current { .. } => {}
                    Verification::Expired { days } => warnings.push(format!(
                        "harness adapter {name} unverified for {days} day(s) — \
                         re-audit with `rrflow harness audit`"
                    )),
                    Verification::Never => warnings.push(format!(
                        "harness adapter {name} has never been audited — \
                         record one with `rrflow harness audit`"
                    )),
                }
            }
        }
    }

    let subject_selection = select_task_subjects(store, &task)?;
    let query = RecallQuery {
        subjects: subject_selection.subjects.clone(),
        predicates: None,
        as_of: now,
    };
    let set = recall(store, &query, budget)?;
    for claim in &set.claims {
        store.observe(reader, &claim.subject, &claim.predicate, now)?;
    }
    let context_assembly = match (routing.as_ref(), attunement.as_ref()) {
        (Some(ready), Some(receipt)) => match assemble_task_context(
            store,
            root,
            &task,
            ready,
            receipt,
            &workflows,
            &set,
            subject_selection.truncated,
            now,
            budget,
        ) {
            Ok(assembly) => Some(assembly),
            Err(error) => {
                warnings.push(format!(
                    "task-specific context receipt could not be persisted: {error}"
                ));
                None
            }
        },
        _ => {
            warnings.push(
                "task-specific context receipt is unavailable because project attunement is incomplete"
                    .into(),
            );
            None
        }
    };
    if let Some(assembly) = &context_assembly {
        warnings.extend(assembly.warnings.iter().cloned());
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "[rrflow] preflight: task={} stack={}; {} selected claim(s), ~{} token(s){}",
        &digest::sha256_hex(task.as_bytes())[..12],
        if stacks.is_empty() {
            "none detected".to_string()
        } else {
            stacks.join("+")
        },
        set.claims.len(),
        set.token_estimate,
        if set.truncated || subject_selection.truncated {
            ", TRUNCATED by budget"
        } else {
            ""
        },
    ));
    if let Some(ready) = &routing {
        lines.push(format!("[rrflow] routing: {}", ready.render()));
    }
    if let Some(receipt) = &attunement {
        lines.push(format!(
            "[rrflow] attunement: receipt={} topology={} profile={} tree={} planning_sources={}",
            receipt.receipt_sha256,
            receipt.topology_sha256,
            receipt.profile_sha256,
            receipt.source_tree_sha256,
            receipt.planning_sources.len()
        ));
    }
    if let Some(assembly) = &context_assembly {
        lines.push(format!(
            "[rrflow] context receipt: {} route={} pattern={} policy={} projection={} reads={}",
            assembly.receipt.receipt_sha256,
            assembly.receipt.route_sha256,
            assembly.receipt.pattern_sha256,
            assembly.receipt.policy_sha256,
            assembly.receipt.projection_sha256,
            assembly.receipt.rrd_reads_sha256,
        ));
    }
    if let Some(snapshot) = &work_plan {
        let verified = snapshot
            .items
            .iter()
            .filter(|item| item.status == super::WorkItemStatus::Verified)
            .count();
        lines.push(format!(
            "[rrflow] work plan: {} verified={}/{} active={} revision={} digest={}",
            snapshot.plan_id,
            verified,
            snapshot.items.len(),
            snapshot.active_item_id.as_deref().unwrap_or("none"),
            snapshot.revision,
            snapshot.plan_sha256,
        ));
    }
    for workflow in &workflows {
        lines.push(format!(
            "[rrflow] workflow: {} manifest={} read_cursor={} manifest_id={}",
            workflow.event,
            workflow.manifest_digest,
            workflow.read.commit_cursor,
            workflow.read.manifest_id,
        ));
    }
    for warning in &warnings {
        lines.push(format!("[rrflow] WARNING: {warning}"));
    }
    if let Some(assembly) = &context_assembly {
        lines.push(assembly.rendered.clone());
    } else {
        for claim in &set.claims {
            lines.push(format!(
                "{} {} = {}  [valid_from={} tx={} by {}]",
                claim.subject.as_str(),
                claim.predicate.as_str(),
                claim.object,
                claim.valid_from,
                claim.tx_time,
                claim.producer.actor,
            ));
        }
        if set.claims.is_empty() {
            lines.push("[rrflow] EXCLUDED recall: no task-relevant claim".into());
        }
    }

    let effectiveness = Effectiveness {
        query: format!(
            "preflight-task:{}:{}",
            &digest::sha256_hex(task.as_bytes())[..12],
            query.subjects.len()
        ),
        claims_returned: set.claims.len(),
        tokens_emitted: set.token_estimate as u64,
        baseline_tokens: None,
        baseline_mode: None,
        provider: harness
            .map(|h| format!("harness:{h}"))
            .unwrap_or_else(|| "operator:cli".into()),
        outcome: RecallOutcome::Unknown,
    };

    tracing::debug!(
        stacks = stacks.join("+"),
        claims = set.claims.len(),
        tokens = set.token_estimate,
        warnings = warnings.len(),
        "preflight"
    );
    Ok(Preflight {
        task,
        stacks,
        routing,
        attunement,
        context_receipt: context_assembly.map(|assembly| assembly.receipt),
        work_plan,
        workflows,
        warnings,
        context: lines.join("\n"),
        effectiveness,
    })
}

fn resolve_task(
    root: &std::path::Path,
    requested: Option<&str>,
    work_plan: Option<&super::WorkPlanSnapshot>,
) -> String {
    if let Some(task) = requested.map(str::trim).filter(|task| !task.is_empty()) {
        return task.to_owned();
    }
    if let Some(active) = work_plan.and_then(|snapshot| snapshot.active_item_id.as_deref()) {
        if let Ok(definition) = read_work_plan(root) {
            if let Some(item) = definition.item.iter().find(|item| item.id == active) {
                return format!("{}: {}", item.title, item.acceptance.join("; "));
            }
        }
    }
    "project session attunement".into()
}
