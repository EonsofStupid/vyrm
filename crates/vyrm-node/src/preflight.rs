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
use crate::stack;
use vyrm_core::{recall, Millis, Reader, RecallQuery};
use vyrm_store::{Effectiveness, ProjectionStatus, RecallOutcome, Store};

/// What a preflight produced. `context` is the injectable text; everything
/// else is the evidence behind it.
#[derive(Debug)]
pub struct Preflight {
    pub stacks: Vec<&'static str>,
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
pub fn preflight(
    store: &Store,
    root: &std::path::Path,
    harness: Option<&str>,
    reader: &Reader,
    now: Millis,
    budget: usize,
) -> Result<Preflight, Box<dyn std::error::Error>> {
    // Runtime stacks plus the framework facet: `bun+vite+tanstack-start`
    // tells the agent what it landed in before it reads a single file.
    let mut stacks: Vec<&'static str> = stack::detect(root).iter().map(|s| s.name).collect();
    stacks.extend(stack::frameworks(root));
    let mut warnings = Vec::new();

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
    let query = RecallQuery { subjects, predicates: None, as_of: now };
    let set = recall(store, &query, budget)?;
    for claim in &set.claims {
        store.observe(reader, &claim.subject, &claim.predicate, now)?;
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "[vyrm] preflight: stack={}; {} claim(s) in force, ~{} token(s){}",
        if stacks.is_empty() { "none detected".to_string() } else { stacks.join("+") },
        set.claims.len(),
        set.token_estimate,
        if set.truncated { ", TRUNCATED by budget" } else { "" },
    ));
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
        provider: harness.map(|h| format!("harness:{h}")).unwrap_or_else(|| "operator:cli".into()),
        outcome: RecallOutcome::Unknown,
    };

    Ok(Preflight { stacks, warnings, context: lines.join("\n"), effectiveness })
}
