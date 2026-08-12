//! In-memory reference implementation of [`ClaimSource`].
//!
//! This implementation serves two functions:
//!
//! 1. It allows `vyrm-core` to verify its key encoding and resolution
//!    independently of the substrate, satisfying the modularity criterion in
//!    `SPEC.md` §5.
//! 2. It is the grounding reference of `SPEC.md` §8.3. A substrate adapter is
//!    correct if and only if it returns what this implementation returns for the
//!    same claims. Divergence must halt rather than be repaired.
//!
//! `BTreeMap` orders keys byte-lexicographically, which is the property the key
//! encoding relies on in the substrate.

use crate::claim::{Claim, Millis};
use crate::error::Result;
use crate::ident::{Predicate, Subject};
use crate::key;
use crate::temporal::ClaimSource;
use std::collections::BTreeMap;
use std::convert::Infallible;

#[derive(Debug, Default, Clone)]
pub struct MemoryClaims {
    rows: BTreeMap<Vec<u8>, Claim>,
}

impl MemoryClaims {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, claim: Claim) -> Result<()> {
        claim.validate()?;
        let k = key::claim_key(&claim.subject, &claim.predicate, claim.valid_from, claim.tx_time);
        self.rows.insert(k, claim);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Every claim in key order. Used by grounding to recompute from scratch.
    pub fn iter(&self) -> impl Iterator<Item = &Claim> {
        self.rows.values()
    }

    fn scan(&self, subject: &Subject, predicate: &Predicate, from: Vec<u8>) -> Vec<Claim> {
        let prefix = key::version_prefix(subject, predicate);
        match key::prefix_end(&prefix) {
            Some(end) => self.rows.range(from..end).map(|(_, c)| c.clone()).collect(),
            None => self.rows.range(from..).map(|(_, c)| c.clone()).collect(),
        }
    }
}

impl ClaimSource for MemoryClaims {
    type Error = Infallible;

    fn versions_at_or_before(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        as_of: Millis,
    ) -> std::result::Result<Vec<Claim>, Self::Error> {
        Ok(self.scan(subject, predicate, key::seek_key(subject, predicate, as_of)))
    }

    fn all_versions(
        &self,
        subject: &Subject,
        predicate: &Predicate,
    ) -> std::result::Result<Vec<Claim>, Self::Error> {
        Ok(self.scan(subject, predicate, key::version_prefix(subject, predicate)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::Producer;
    use crate::temporal::ClaimReader;

    fn store() -> MemoryClaims {
        let mut s = MemoryClaims::new();
        let p = Producer { actor: "test".into(), on_behalf_of: None, session: None };
        let subj = Subject::new("wp3").unwrap();
        let pred = Predicate::new("status").unwrap();
        for (obj, vf, vt) in [("v1", 100u64, Some(200u64)), ("v2", 200, Some(300)), ("v3", 300, None)] {
            let mut c = Claim::new(subj.clone(), pred.clone(), obj, vf, vf, p.clone());
            c.valid_to = vt;
            s.insert(c).unwrap();
        }
        // adversarial neighbours that must never appear in wp3/status results
        for (s_, p_) in [("wp3x", "status"), ("wp3", "statusx"), ("wp", "status")] {
            let c = Claim::new(
                Subject::new(s_).unwrap(),
                Predicate::new(p_).unwrap(),
                "WRONG",
                250,
                250,
                p.clone(),
            );
            s.insert(c).unwrap();
        }
        s
    }

    #[test]
    fn as_of_matches_the_fjall_verified_expectations() {
        // Same seven cases proven against Fjall 3.1.8 on 2026-08-09.
        let s = store();
        let subj = Subject::new("wp3").unwrap();
        let pred = Predicate::new("status").unwrap();
        let at = |t| s.as_of(&subj, &pred, t).unwrap().map(|c| c.object);
        assert_eq!(at(99), None);
        assert_eq!(at(100), Some("v1".into()));
        assert_eq!(at(150), Some("v1".into()));
        assert_eq!(at(200), Some("v2".into()));
        assert_eq!(at(250), Some("v2".into()));
        assert_eq!(at(300), Some("v3".into()));
        assert_eq!(at(9999), Some("v3".into()));
    }

    #[test]
    fn history_is_newest_first_and_isolated() {
        let s = store();
        let subj = Subject::new("wp3").unwrap();
        let pred = Predicate::new("status").unwrap();
        let objs: Vec<_> = s.history(&subj, &pred).unwrap().into_iter().map(|c| c.object).collect();
        assert_eq!(objs, vec!["v3", "v2", "v1"]);
        assert!(!objs.iter().any(|o| o == "WRONG"), "neighbour leaked into results");
    }
}
