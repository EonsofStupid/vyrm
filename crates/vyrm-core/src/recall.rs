//! Recall: resolving the claims in force for a subject set into a recall set.
//!
//! `SPEC.md` §10: a recall set is **semantic content with provenance** — the
//! claims themselves, each carrying its producer and both timelines — and MUST
//! NOT be a rendered prompt string. Rendering belongs to the adapter, because a
//! local adapter may inject below the token layer while a frontier adapter must
//! materialize into tokens.
//!
//! Defined over [`ClaimSource`] so that every adapter recalls identically; the
//! grounding rule (`SPEC.md` §8.3) makes [`crate::reference::MemoryClaims`] the
//! arbiter of what a correct recall returns.

use crate::claim::{Claim, Millis};
use crate::ident::{Predicate, Subject};
use crate::temporal::{resolve_as_of, ClaimSource};
use serde::{Deserialize, Serialize};

/// What to recall: the claims in force at `as_of` for these subjects,
/// optionally narrowed to a predicate set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallQuery {
    pub subjects: Vec<Subject>,
    /// `None` recalls every predicate the subjects carry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicates: Option<Vec<Predicate>>,
    /// The instant to resolve at. The kernel never reads a clock.
    pub as_of: Millis,
}

/// The result of a recall: claims with provenance, a content digest, and a
/// token estimate for the adapter that must render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecallSet {
    pub claims: Vec<Claim>,
    /// SHA-256 over canonical bytes of every included claim, in order.
    /// Two recalls with the same digest carry the same knowledge, so an
    /// unchanged recall can be retransmitted as a digest (`SPEC.md` §13.2).
    pub digest: String,
    /// Estimated token cost if an adapter renders every included claim. An
    /// estimate by declaration: the A/B harness measures its error against a
    /// real tokenizer, and the measurement is recorded in `PLAN.md`, not here.
    pub token_estimate: usize,
    /// True when the budget excluded claims that matched the query. A truncated
    /// recall is not wrong, but a consumer must be able to see that it is
    /// partial rather than assume it is the whole truth.
    pub truncated: bool,
}

/// The documented estimation model: one token per four bytes of semantic
/// content, the long-standing published rule of thumb for English and code.
/// Chosen for having zero dependencies, not for accuracy — the harness
/// measures the error, and a measured error on an honest estimate beats an
/// unmeasured claim of precision.
pub const ESTIMATED_BYTES_PER_TOKEN: usize = 4;

/// Estimated render cost of one claim: its semantic fields under the
/// four-bytes-per-token model.
pub fn estimate_claim_tokens(claim: &Claim) -> usize {
    let bytes = claim.subject.as_str().len()
        + claim.predicate.as_str().len()
        + claim.object.len()
        + claim.producer.actor.len();
    bytes.div_ceil(ESTIMATED_BYTES_PER_TOKEN)
}

fn digest(claims: &[Claim]) -> String {
    let mut bytes = b"vyrm-recall-v1\0".to_vec();
    bytes.extend_from_slice(&(claims.len() as u64).to_be_bytes());
    for claim in claims {
        let canonical = claim.canonical_bytes();
        bytes.extend_from_slice(&(canonical.len() as u64).to_be_bytes());
        bytes.extend_from_slice(&canonical);
    }
    crate::digest::sha256_hex(&bytes)
}

/// Resolves the claims in force for the query's subjects and fills the token
/// budget.
///
/// Deterministic by construction: subjects are visited in query order (repeats
/// skipped), predicates in the order the subject scan yields them, and the
/// budget fill takes claims in that order, stopping at the first claim that
/// does not fit. The first resolved claim is always included even when it alone
/// exceeds the budget — an empty answer is worse than an oversized one, the
/// same rule the routing projection applies to files.
pub fn recall<S: ClaimSource>(
    source: &S,
    query: &RecallQuery,
    token_budget: usize,
) -> Result<RecallSet, S::Error> {
    let mut included: Vec<Claim> = Vec::new();
    let mut spent = 0usize;
    let mut truncated = false;
    let mut visited: Vec<&Subject> = Vec::new();

    for subject in &query.subjects {
        if visited.contains(&subject) {
            continue;
        }
        visited.push(subject);

        let versions = source.subject_versions(subject)?;
        // The scan yields claims grouped by predicate, newest first within
        // each group. Consecutive-run grouping preserves that contract.
        let mut index = 0;
        while index < versions.len() {
            let predicate = versions[index].predicate.clone();
            let mut end = index;
            while end < versions.len() && versions[end].predicate == predicate {
                end += 1;
            }
            let group = &versions[index..end];
            index = end;

            if let Some(wanted) = &query.predicates {
                if !wanted.contains(&predicate) {
                    continue;
                }
            }
            let Some(current) = resolve_as_of(group, query.as_of) else {
                continue; // retired without successor, or not yet valid
            };

            let cost = estimate_claim_tokens(current);
            if included.is_empty() || spent + cost <= token_budget {
                spent += cost;
                included.push(current.clone());
            } else {
                truncated = true;
            }
        }
    }

    Ok(RecallSet {
        digest: digest(&included),
        token_estimate: spent,
        truncated,
        claims: included,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::Producer;
    use crate::reference::MemoryClaims;

    fn producer() -> Producer {
        Producer { actor: "test".into(), on_behalf_of: None, session: None }
    }

    fn claim(subject: &str, predicate: &str, object: &str, from: Millis) -> Claim {
        Claim::new(
            Subject::new(subject).unwrap(),
            Predicate::new(predicate).unwrap(),
            object,
            from,
            from,
            producer(),
        )
    }

    fn corpus() -> MemoryClaims {
        let mut m = MemoryClaims::new();
        // wp3 status: superseded chain — only v2 is current at 250.
        let mut v1 = claim("wp3", "status", "authored", 100);
        v1.valid_to = Some(200);
        m.insert(v1).unwrap();
        m.insert(claim("wp3", "status", "tested", 200)).unwrap();
        m.insert(claim("wp3", "owner", "jessay", 100)).unwrap();
        // A retired claim with no successor must not be recalled.
        let mut lapsed = claim("wp3", "blocker", "flaky-ci", 100);
        lapsed.valid_to = Some(150);
        m.insert(lapsed).unwrap();
        // A different subject, untouched by wp3 queries.
        m.insert(claim("wp9", "status", "shipped", 100)).unwrap();
        m
    }

    fn query(subjects: &[&str], as_of: Millis) -> RecallQuery {
        RecallQuery {
            subjects: subjects.iter().map(|s| Subject::new(*s).unwrap()).collect(),
            predicates: None,
            as_of,
        }
    }

    #[test]
    fn recalls_only_current_claims_of_the_subject_set() {
        let set = recall(&corpus(), &query(&["wp3"], 250), 10_000).unwrap();
        let facts: Vec<_> =
            set.claims.iter().map(|c| (c.predicate.as_str(), c.object.as_str())).collect();
        assert_eq!(
            facts,
            vec![("owner", "jessay"), ("status", "tested")],
            "superseded, lapsed, and foreign claims must be excluded"
        );
        assert!(!set.truncated);
    }

    #[test]
    fn resolution_is_as_of_not_latest() {
        let set = recall(&corpus(), &query(&["wp3"], 149), 10_000).unwrap();
        let status = set.claims.iter().find(|c| c.predicate.as_str() == "status").unwrap();
        assert_eq!(status.object, "authored", "at 149 the superseded version was in force");
        assert!(
            set.claims.iter().any(|c| c.predicate.as_str() == "blocker"),
            "the blocker was still open at 149"
        );

        // valid_to is exclusive: at exactly 150 the blocker has lapsed.
        let at_boundary = recall(&corpus(), &query(&["wp3"], 150), 10_000).unwrap();
        assert!(
            !at_boundary.claims.iter().any(|c| c.predicate.as_str() == "blocker"),
            "a half-open interval excludes its end instant"
        );
    }

    #[test]
    fn predicate_filter_narrows_the_set() {
        let q = RecallQuery {
            subjects: vec![Subject::new("wp3").unwrap()],
            predicates: Some(vec![Predicate::new("status").unwrap()]),
            as_of: 250,
        };
        let set = recall(&corpus(), &q, 10_000).unwrap();
        assert_eq!(set.claims.len(), 1);
        assert_eq!(set.claims[0].object, "tested");
    }

    #[test]
    fn budget_truncates_and_says_so_but_never_returns_empty() {
        let set = recall(&corpus(), &query(&["wp3"], 250), 1).unwrap();
        assert_eq!(set.claims.len(), 1, "the first claim is included even over budget");
        assert!(set.truncated, "exclusion by budget must be visible");
    }

    #[test]
    fn digest_identifies_content_not_invocation() {
        let a = recall(&corpus(), &query(&["wp3"], 250), 10_000).unwrap();
        let b = recall(&corpus(), &query(&["wp3", "wp3"], 250), 10_000).unwrap();
        assert_eq!(a.digest, b.digest, "a repeated subject must not change the content");
        let c = recall(&corpus(), &query(&["wp3", "wp9"], 250), 10_000).unwrap();
        assert_ne!(a.digest, c.digest, "different knowledge must not share a digest");
    }

    #[test]
    fn digest_changes_when_recalled_provenance_changes() {
        let original = corpus();
        let a = recall(&original, &query(&["wp3"], 250), 10_000).unwrap();
        let mut changed = MemoryClaims::new();
        for mut claim in original.iter().cloned() {
            claim.producer.session = Some("new-session".into());
            changed.insert(claim).unwrap();
        }
        let b = recall(&changed, &query(&["wp3"], 250), 10_000).unwrap();
        assert_ne!(a.digest, b.digest, "provenance is part of recalled knowledge");
    }

    #[test]
    fn token_estimate_is_the_sum_of_included_claim_estimates() {
        let set = recall(&corpus(), &query(&["wp3"], 250), 10_000).unwrap();
        let expected: usize = set.claims.iter().map(estimate_claim_tokens).sum();
        assert_eq!(set.token_estimate, expected);
    }
}
