//! Bi-temporal resolution and the storage port.
//!
//! Core defines the port; adapters implement it. Core never learns what a
//! keyspace, a transport, or a tier policy is.

use crate::claim::{Claim, Millis};
use crate::ident::{Predicate, Subject};

/// Substrate port. An adapter supplies newest-first candidates; all bi-temporal
/// resolution remains in this module, so that every adapter resolves
/// identically. This is the property the grounding reference in
/// [`crate::reference`] verifies.
pub trait ClaimSource {
    type Error;

    /// Versions of `subject`+`predicate` with `valid_from <= as_of`, **newest
    /// first**. Implementations do this as one seek from `key::seek_key` to
    /// `key::prefix_end`.
    fn versions_at_or_before(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        as_of: Millis,
    ) -> Result<Vec<Claim>, Self::Error>;

    /// Every version, newest first.
    fn all_versions(
        &self,
        subject: &Subject,
        predicate: &Predicate,
    ) -> Result<Vec<Claim>, Self::Error>;

    /// Every claim of `subject` across all predicates, ordered by predicate and
    /// newest first within each predicate. Implementations do this as one seek
    /// over `key::subject_prefix`. This is the port recall stands on: a
    /// subject-set recall costs one seek per subject, never a store scan.
    fn subject_versions(&self, subject: &Subject) -> Result<Vec<Claim>, Self::Error>;
}

/// Resolves the claim in force at `as_of` from newest-first candidates.
///
/// Selects the first candidate that is valid at `as_of`, not the first candidate
/// unconditionally. The newest claim with `valid_from <= as_of` may have been
/// retired without a successor, in which case the correct result is `None`. The
/// operation remains a single seek and may read past a retired head.
///
/// See `SPEC.md` §6.2.
pub fn resolve_as_of(candidates: &[Claim], as_of: Millis) -> Option<&Claim> {
    candidates.iter().find(|claim| claim.valid_at(as_of))
}

/// Claims recorded after `since`, ordered by transaction time.
///
/// Filters on `tx_time` rather than `valid_from` by design: a claim backdated
/// into the past is still unseen by a reader whose watermark precedes its
/// transaction time.
pub fn changed_since(claims: &[Claim], since: Millis) -> Vec<&Claim> {
    claims.iter().filter(|c| c.tx_time > since).collect()
}

/// Convenience reads layered on the port.
pub trait ClaimReader: ClaimSource {
    fn as_of(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        at: Millis,
    ) -> Result<Option<Claim>, Self::Error> {
        let candidates = self.versions_at_or_before(subject, predicate, at)?;
        Ok(resolve_as_of(&candidates, at).cloned())
    }

    /// The claim in force at `now`. The kernel never reads a clock; `now` is
    /// supplied so results stay reproducible.
    fn current(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        now: Millis,
    ) -> Result<Option<Claim>, Self::Error> {
        self.as_of(subject, predicate, now)
    }

    fn history(
        &self,
        subject: &Subject,
        predicate: &Predicate,
    ) -> Result<Vec<Claim>, Self::Error> {
        self.all_versions(subject, predicate)
    }
}

impl<T: ClaimSource> ClaimReader for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::Producer;

    fn producer() -> Producer {
        Producer { actor: "test".into(), on_behalf_of: None, session: None }
    }

    fn claim(object: &str, valid_from: Millis, valid_to: Option<Millis>) -> Claim {
        let mut c = Claim::new(
            Subject::new("wp3").unwrap(),
            Predicate::new("status").unwrap(),
            object,
            valid_from,
            valid_from,
            producer(),
        );
        c.valid_to = valid_to;
        c
    }

    /// Newest-first, as the port contract requires.
    fn versions() -> Vec<Claim> {
        vec![
            claim("v3", 300, None),
            claim("v2", 200, Some(300)),
            claim("v1", 100, Some(200)),
        ]
    }

    #[test]
    fn resolves_each_boundary() {
        let v = versions();
        let at = |t| resolve_as_of(&v, t).map(|c| c.object.as_str());
        assert_eq!(at(99), None);
        assert_eq!(at(100), Some("v1"));
        assert_eq!(at(199), Some("v1"));
        assert_eq!(at(200), Some("v2"));
        assert_eq!(at(299), Some("v2"));
        assert_eq!(at(300), Some("v3"));
        assert_eq!(at(9999), Some("v3"));
    }

    #[test]
    fn retired_head_with_no_successor_resolves_to_none() {
        // The seek would return this claim, but it was retired and nothing
        // replaced it. Take-first would wrongly report it as current.
        let v = vec![claim("only", 100, Some(200))];
        assert_eq!(resolve_as_of(&v, 250), None);
        assert_eq!(resolve_as_of(&v, 150).map(|c| c.object.as_str()), Some("only"));
    }

    #[test]
    fn falls_through_a_lapsed_newer_claim_to_a_still_open_older_one() {
        // Overlapping valid-time intervals are not expected in well-formed data,
        // but resolution must remain deterministic rather than returning none.
        let v = vec![claim("temporary", 200, Some(210)), claim("standing", 100, None)];
        assert_eq!(resolve_as_of(&v, 205).map(|c| c.object.as_str()), Some("temporary"));
        assert_eq!(resolve_as_of(&v, 250).map(|c| c.object.as_str()), Some("standing"));
    }

    #[test]
    fn changed_since_uses_transaction_time_not_valid_time() {
        let mut backdated = claim("backdated", 50, None);
        backdated.tx_time = 500; // happened long ago, learned about it just now
        let recent = claim("recent", 400, None); // tx_time == 400
        let all = vec![backdated, recent];

        let news: Vec<_> = changed_since(&all, 450).iter().map(|c| c.object.as_str()).collect();
        assert_eq!(news, vec!["backdated"], "a backdated fact is still news");
    }
}
