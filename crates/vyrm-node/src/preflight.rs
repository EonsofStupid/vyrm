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

use crate::registry::{Registry, Verification};
use crate::routing::{ensure_routing_fresh, RoutingReady};
use crate::stack;
use crate::workflow::{WorkflowCatalog, WorkflowPreflight, WORKFLOW_FILE};
use crate::InstanceBinding;
use vyrm_core::{recall, Millis, Reader, RecallQuery};
use vyrm_store::{Effectiveness, Engine, ProjectionStatus, RecallOutcome};

/// What a preflight produced. `context` is the injectable text; everything
/// else is the evidence behind it.
#[derive(Debug)]
pub struct Preflight {
    pub stacks: Vec<&'static str>,
    /// Persisted source-routing state established before recall is injected.
    /// `None` means freshness could not be established and `warnings` says
    /// why; the pre-tool gate will deny mutation under the same condition.
    pub routing: Option<RoutingReady>,
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
                let scope = vyrm_core::ScopeId::new(rule.scope.clone())?;
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

    // Estate health: a quarantined projection is surfaced, and recall
    // proceeds from the authoritative claims keyspace regardless — the gate
    // (`hook.rs`) is what blocks, the preflight is what informs.
    if let ProjectionStatus::Quarantined { at, .. } = store.current_projection()?.status {
        warnings.push(format!(
            "memory projection quarantined at {at}: grounding found divergence; \
             mutations are gated until `vyrm reset-projection`"
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
                         re-audit with `vyrm harness audit`"
                    )),
                    Verification::Never => warnings.push(format!(
                        "harness adapter {name} has never been audited — \
                         record one with `vyrm harness audit`"
                    )),
                }
            }
        }
    }

    let subjects = store.subjects()?;
    let query = RecallQuery {
        subjects,
        predicates: None,
        as_of: now,
    };
    let set = recall(store, &query, budget)?;
    for claim in &set.claims {
        store.observe(reader, &claim.subject, &claim.predicate, now)?;
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "[vyrm] preflight: stack={}; {} claim(s) in force, ~{} token(s){}",
        if stacks.is_empty() {
            "none detected".to_string()
        } else {
            stacks.join("+")
        },
        set.claims.len(),
        set.token_estimate,
        if set.truncated {
            ", TRUNCATED by budget"
        } else {
            ""
        },
    ));
    if let Some(ready) = &routing {
        lines.push(format!("[vyrm] routing: {}", ready.render()));
    }
    for workflow in &workflows {
        lines.push(format!(
            "[vyrm] workflow: {} manifest={} read_cursor={} manifest_id={}",
            workflow.event,
            workflow.manifest_digest,
            workflow.read.commit_cursor,
            workflow.read.manifest_id,
        ));
    }
    for warning in &warnings {
        lines.push(format!("[vyrm] WARNING: {warning}"));
    }
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

    let effectiveness = Effectiveness {
        query: format!("preflight:{}", query.subjects.len()),
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
        stacks,
        routing,
        workflows,
        warnings,
        context: lines.join("\n"),
        effectiveness,
    })
}
