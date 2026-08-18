use vyrm_core::{
    Check, CheckStatus, DecisionKind, Evidence, ReasoningPayload, ReasoningState, RunOutcome,
};
use vyrm_node::{active_reasoning_run, reasoning_run, record_reasoning};
use vyrm_store::MemoryEngine;

fn evidence(summary: &str) -> Evidence {
    Evidence {
        source: "cargo test --workspace".into(),
        digest: "b".repeat(64),
        summary: summary.into(),
    }
}

#[test]
fn the_runtime_persists_and_replays_one_authoritative_active_run() {
    let store = MemoryEngine::new();
    record_reasoning(
        &store,
        "step-3",
        1,
        "agent:test",
        ReasoningPayload::Goal {
            statement: "ship contract".into(),
            acceptance: vec!["ordered evidence".into()],
        },
    )
    .unwrap();
    record_reasoning(
        &store,
        "step-3",
        2,
        "agent:test",
        ReasoningPayload::Plan {
            hypothesis: "a replayed state machine is enforceable".into(),
            steps: vec!["implement".into(), "verify".into()],
        },
    )
    .unwrap();
    assert_eq!(
        active_reasoning_run(&store).unwrap().unwrap().state(),
        ReasoningState::NeedsAttempt
    );

    for (at, payload) in [
        (
            3,
            ReasoningPayload::Attempt {
                summary: "implemented".into(),
                actions: vec!["edit reasoning.rs".into()],
            },
        ),
        (
            4,
            ReasoningPayload::Observation {
                summary: "tests exercised transitions".into(),
                evidence: vec![evidence("tests passed")],
            },
        ),
        (
            5,
            ReasoningPayload::Decision {
                decision: DecisionKind::Verify,
                rationale: "acceptance appears met".into(),
            },
        ),
        (
            6,
            ReasoningPayload::Verification {
                checks: vec![Check {
                    name: "workspace".into(),
                    status: CheckStatus::Passed,
                    evidence: vec![evidence("green")],
                }],
            },
        ),
        (
            7,
            ReasoningPayload::Outcome {
                outcome: RunOutcome::Succeeded,
                summary: "contract complete".into(),
            },
        ),
    ] {
        record_reasoning(&store, "step-3", at, "agent:test", payload).unwrap();
    }

    assert!(active_reasoning_run(&store).unwrap().is_none());
    let run = reasoning_run(&store, "step-3").unwrap().unwrap();
    assert!(run.is_complete());
    assert_eq!(run.events().len(), 7);
}

#[test]
fn a_second_run_and_out_of_order_events_are_refused() {
    let store = MemoryEngine::new();
    let goal = ReasoningPayload::Goal {
        statement: "first".into(),
        acceptance: vec!["done".into()],
    };
    record_reasoning(&store, "first", 1, "agent", goal.clone()).unwrap();
    assert!(record_reasoning(&store, "second", 2, "agent", goal).is_err());
    assert!(record_reasoning(
        &store,
        "first",
        2,
        "agent",
        ReasoningPayload::Attempt { summary: "skipped plan".into(), actions: vec![] }
    )
    .is_err());
}
