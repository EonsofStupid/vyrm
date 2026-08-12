//! Integrity of the A/B fixtures. The harness (`examples/recall_ab.rs`) is
//! only reproducible while the checked-in corpus stays parseable and every
//! queried subject still has claims to recall.

use vyrm_core::{recall, Claim, RecallQuery, Subject};
use vyrm_core::reference::MemoryClaims;

const CLAIMS_JSON: &str = include_str!("../fixtures/ab/claims.json");

/// Mirrors the subjects used by `examples/recall_ab.rs`.
const SUBJECTS: &[&str] = &[
    "ranking", "persistence", "vyrm-store", "vyrm-graph", "step-r",
    "panel", "observatory", "tiers", "extraction", "entities",
];

#[test]
fn the_fixture_parses_and_every_queried_subject_recalls_claims() {
    let claims: Vec<Claim> = serde_json::from_str(CLAIMS_JSON).expect("fixture parses");
    assert!(claims.len() >= 30, "corpus shrank to {}", claims.len());

    let mut reference = MemoryClaims::new();
    for claim in claims {
        reference.insert(claim).expect("fixture claim validates");
    }

    for subject in SUBJECTS {
        let query = RecallQuery {
            subjects: vec![Subject::new(*subject).unwrap()],
            predicates: None,
            as_of: 20_000,
        };
        let set = recall(&reference, &query, 10_000).unwrap();
        assert!(!set.claims.is_empty(), "subject {subject} recalls nothing — fixture rot");
    }
}

#[test]
fn superseded_fixture_claims_resolve_bi_temporally() {
    let claims: Vec<Claim> = serde_json::from_str(CLAIMS_JSON).expect("fixture parses");
    let mut reference = MemoryClaims::new();
    for claim in claims {
        reference.insert(claim).unwrap();
    }
    // The ranking design was superseded at 8000: weighted centrality gave way
    // to the tie-breaker. Both readings must be recallable at their instants.
    let at = |as_of| {
        let query = RecallQuery {
            subjects: vec![Subject::new("ranking").unwrap()],
            predicates: None,
            as_of,
        };
        recall(&reference, &query, 10_000).unwrap()
    };
    let design_at = |as_of| {
        at(as_of)
            .claims
            .into_iter()
            .find(|c| c.predicate.as_str() == "design")
            .expect("a design claim is in force")
            .object
    };
    assert!(
        design_at(7_500).contains("weighted into the score"),
        "the superseded design must be readable at its instant"
    );
    assert!(
        design_at(20_000).contains("tie-breaker only"),
        "the current design, not the superseded one, must be recalled now"
    );
}
