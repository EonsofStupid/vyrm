//! The Step 4 A/B: recall against unstructured context, measured in tokens.
//!
//! `SPEC.md` §13.1: token reduction is a measurement and must not be stated as
//! a property without one. This harness is that measurement, reproducible from
//! two checked-in fixtures derived from the same source of truth:
//!
//! - `fixtures/ab/unstructured.md` — a frozen snapshot of `PLAN.md` as of
//!   2026-08-12, the operator's real development journal. The baseline arm
//!   models what an agent without vyrm injects: every journal section that
//!   mentions a queried subject, stacked whole. That is the md-stacking
//!   behaviour this system exists to replace, reproduced mechanically
//!   (case-insensitive section match), not caricatured.
//! - `fixtures/ab/claims.json` — the same knowledge as bi-temporal claims,
//!   extracted from that snapshot. Each claim is auditable against the
//!   journal text.
//!
//! Both arms are counted with the same real tokenizer (o200k_base — a proxy,
//! since no Claude tokenizer is public; the reported quantity is the ratio and
//! the tokenizer cancels). The kernel's four-bytes-per-token estimate is
//! validated against the real count and its error reported, because an honest
//! estimate with a measured error beats an unmeasured claim of precision.
//!
//! Every recall is recorded in the effectiveness ledger with its baseline, and
//! the outcome distribution is reported. A non-reduction on any query is a
//! result, printed with the same weight as a reduction.
//!
//! ```text
//! cargo run --release -p vyrm-cli --example recall_ab -- <store-path>
//! ```

use vyrm_core::{recall, Claim, RecallQuery, Subject};
use vyrm_store::{Effectiveness, InvocationInput, Outcome, RecallOutcome, Store, Trigger};

const UNSTRUCTURED: &str = include_str!("../fixtures/ab/unstructured.md");
const CLAIMS_JSON: &str = include_str!("../fixtures/ab/claims.json");

/// The instant every query resolves at: after the newest fixture claim, so the
/// corpus is read in its final state. Fixed, because the kernel never reads a
/// clock and neither does a reproducible measurement.
const AS_OF: u64 = 20_000;
const TOKEN_BUDGET: usize = 1_500;

/// The fixed query set: subject sets an operator actually asks after.
const QUERIES: &[(&str, &[&str])] = &[
    ("ranking state", &["ranking"]),
    ("persistence state", &["persistence"]),
    ("store + flake", &["vyrm-store"]),
    ("routing projection", &["vyrm-graph", "step-r"]),
    ("panel + observatory", &["panel", "observatory", "tiers"]),
    ("extraction + entities", &["extraction", "entities"]),
];

/// The frontier adapter's rendering: one line per claim, the same shape the
/// CLI prints. Tokens are counted over this, because §10 makes rendering the
/// adapter's job and this is the adapter a frontier model gets.
fn render(claims: &[Claim]) -> String {
    claims
        .iter()
        .map(|c| {
            format!(
                "{} {} = {}  [valid_from={} tx={} by {}]",
                c.subject.as_str(),
                c.predicate.as_str(),
                c.object,
                c.valid_from,
                c.tx_time,
                c.producer.actor,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The baseline arm: every `##`/`###` section of the journal that mentions any
/// queried subject, stacked in document order.
fn unstructured_context(subjects: &[&str]) -> String {
    let mut sections: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in UNSTRUCTURED.lines() {
        if (line.starts_with("## ") || line.starts_with("### ")) && !current.is_empty() {
            sections.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.is_empty() {
        sections.push(current);
    }
    sections
        .into_iter()
        .filter(|section| {
            let lowered = section.to_lowercase();
            subjects.iter().any(|s| lowered.contains(&s.to_lowercase()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() {
    let store_path = std::env::args()
        .nth(1)
        .expect("usage: recall_ab <store-path>");
    let store = Store::open(std::path::Path::new(&store_path)).expect("open store");

    let claims: Vec<Claim> = serde_json::from_str(CLAIMS_JSON).expect("parse claims fixture");
    if store.sequence().expect("sequence") == 0 {
        store.append_batch(&claims).expect("load corpus");
    }
    let tokenizer = tiktoken_rs::o200k_base().expect("tokenizer");
    let count = |text: &str| tokenizer.encode_with_special_tokens(text).len();

    println!(
        "corpus: {} claims from fixtures/ab/claims.json; baseline: {} tokens of journal\n",
        claims.len(),
        count(UNSTRUCTURED),
    );
    println!(
        "{:<24} {:>7} {:>10} {:>10} {:>10} {:>9} {:>7}",
        "query", "claims", "est tok", "real tok", "baseline", "reduction", "trunc"
    );
    println!("{}", "-".repeat(84));

    let (mut total_real, mut total_baseline) = (0usize, 0usize);
    let mut estimate_errors: Vec<f64> = Vec::new();

    for (label, subjects) in QUERIES {
        let query = RecallQuery {
            subjects: subjects.iter().map(|s| Subject::new(*s).unwrap()).collect(),
            predicates: None,
            as_of: AS_OF,
        };
        let set = recall(&store, &query, TOKEN_BUDGET).expect("recall");
        let rendered = render(&set.claims);
        let real = count(&rendered);
        let baseline = count(&unstructured_context(subjects));

        total_real += real;
        total_baseline += baseline;
        if real > 0 {
            estimate_errors
                .push((set.token_estimate as f64 - real as f64).abs() / real as f64);
        }

        // §13.1: the record carries the controlled baseline. Outcome starts
        // unknown; judgement is the operator's, later.
        store
            .record_invocation(InvocationInput {
                at: AS_OF,
                trigger: Trigger::Manual,
                command: "recall",
                arguments: &subjects.iter().map(|s| format!("subject={s}")).collect::<Vec<_>>(),
                outcome: Outcome::Ok,
                duration_ms: 0,
                detail: Some(format!("A/B harness: {label}")),
                effectiveness: Some(Effectiveness {
                    query: subjects.join(","),
                    claims_returned: set.claims.len(),
                    tokens_emitted: real as u64,
                    baseline_tokens: Some(baseline as u64),
                    baseline_mode: Some("unstructured_context".into()),
                    provider: "frontier:claude".into(),
                    outcome: RecallOutcome::Unknown,
                }),
            })
            .expect("record ledger entry");

        println!(
            "{:<24} {:>7} {:>10} {:>10} {:>10} {:>8.2}x {:>7}",
            label,
            set.claims.len(),
            set.token_estimate,
            real,
            baseline,
            baseline as f64 / real.max(1) as f64,
            if set.truncated { "YES" } else { "no" },
        );
    }

    println!("{}", "-".repeat(84));
    println!(
        "{:<24} {:>7} {:>10} {:>10} {:>10} {:>8.2}x",
        "TOTAL",
        "",
        "",
        total_real,
        total_baseline,
        total_baseline as f64 / total_real.max(1) as f64,
    );

    let mean_error = estimate_errors.iter().sum::<f64>() / estimate_errors.len().max(1) as f64;
    println!(
        "\nkernel token estimate vs o200k_base: mean absolute error {:.1}%",
        mean_error * 100.0
    );

    // The ledger now holds one record per query, each with its baseline; the
    // distribution is all-unknown until an operator judges recalls in use.
    let records = store.invocations_since(0).expect("ledger");
    let with_baseline = records
        .iter()
        .filter(|r| {
            r.effectiveness
                .as_ref()
                .is_some_and(|e| e.baseline_tokens.is_some())
        })
        .count();
    println!(
        "ledger: {} recall record(s) with controlled baselines; outcomes all `unknown` until judged",
        with_baseline
    );
}
