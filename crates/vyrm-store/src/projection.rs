//! The current-state projection over the claim log. `SPEC.md` §8.2 and §8.3.
//!
//! The projection holds the newest recorded version per (subject, predicate)
//! pair — "newest" by (valid_from, tx_time), matching the key encoding's
//! ordering. It is derived state and MUST NOT be treated as authoritative
//! (§8.2): its whole value is that it can be rebuilt from the sequence index
//! and *differenced against that rebuild*.
//!
//! Three invariants carry this module:
//!
//! 1. **The watermark advances in the same write as the projection** (§8.2).
//!    Both live in one serialized blob under one key, so a crash mid-rebuild
//!    leaves the old blob with the old watermark and the interval is replayed,
//!    never skipped. Replay is safe because the fold is idempotent.
//! 2. **Grounding recomputes at the projection's own watermark**, not at the
//!    current sequence. Grounding verifies that incremental maintenance equals
//!    batch recomputation over the same interval; lag beyond the watermark is
//!    rebuild's dimension, and conflating the two would report honest lag as
//!    divergence. §8.3's `as_of = now` is reached by rebuilding first, which
//!    is what the operator `ground` command does.
//! 3. **Divergence halts; it is never repaired** (§8.3). A quarantined
//!    projection refuses reads and rebuilds until the operator explicitly
//!    resets it. The quarantine write is Authoritative — the one derived-state
//!    write that pays for an fsync — because a detected divergence that a
//!    crash could silently forget would defeat the detection.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use vyrm_core::{Claim, Millis, Predicate, Subject};

/// Name the projection is stored under in the projections keyspace.
pub const CURRENT_PROJECTION: &str = "current";

/// Health of the projection. Quarantine is entered by grounding and left only
/// by an explicit operator reset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectionStatus {
    Active,
    Quarantined {
        at: Millis,
        differences: Vec<String>,
    },
}

/// Evidence of the last successful grounding (§8.3: `grounded { at, sequence,
/// digest }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundedStamp {
    pub at: Millis,
    pub sequence: u64,
    pub digest: u64,
}

/// What one rebuild did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebuildOutcome {
    pub from: u64,
    pub to: u64,
    pub applied: usize,
}

/// What grounding found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroundingReport {
    Grounded(GroundedStamp),
    /// The projection diverged from its recomputation and is now quarantined.
    Divergence { differences: Vec<String> },
}

/// Persisted form: entries are the claims themselves, sorted by (subject,
/// predicate) — the map key is derivable from the claim, so storing pairs
/// would be a second copy that could drift.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredProjection {
    watermark: u64,
    status: ProjectionStatus,
    last_grounded: Option<GroundedStamp>,
    entries: Vec<Claim>,
}

/// The projection, loaded. Reads go through [`CurrentProjection::get`] so the
/// quarantine check cannot be skipped by reaching into the map.
#[derive(Debug, Clone)]
pub struct CurrentProjection {
    pub watermark: u64,
    pub status: ProjectionStatus,
    pub last_grounded: Option<GroundedStamp>,
    entries: BTreeMap<(String, String), Claim>,
}

impl CurrentProjection {
    pub(crate) fn empty() -> Self {
        CurrentProjection {
            watermark: 0,
            status: ProjectionStatus::Active,
            last_grounded: None,
            entries: BTreeMap::new(),
        }
    }

    /// Newest recorded version for the pair. Refuses when quarantined: a
    /// halted projection answers nothing rather than something stale (§8.3).
    pub fn get(&self, subject: &Subject, predicate: &Predicate) -> Result<Option<&Claim>> {
        if let ProjectionStatus::Quarantined { at, .. } = &self.status {
            return Err(Error::Quarantined(format!(
                "projection `{CURRENT_PROJECTION}` quarantined at {at}; reset to recover"
            )));
        }
        Ok(self
            .entries
            .get(&(subject.as_str().to_owned(), predicate.as_str().to_owned())))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Folds one interval of the claim log into the projection. Idempotent:
    /// `>=` means re-applying an interval reproduces the same state, which is
    /// what makes the §8.2 crash-replay safe. On an exact (valid_from,
    /// tx_time) tie the later occurrence wins, matching the claims keyspace,
    /// where the second write of one claim key overwrites the first.
    pub(crate) fn apply(&mut self, claims: &[Claim]) {
        for claim in claims {
            let key = (
                claim.subject.as_str().to_owned(),
                claim.predicate.as_str().to_owned(),
            );
            match self.entries.get(&key) {
                Some(existing)
                    if (claim.valid_from, claim.tx_time)
                        < (existing.valid_from, existing.tx_time) => {}
                _ => {
                    self.entries.insert(key, claim.clone());
                }
            }
        }
    }

    pub(crate) fn to_stored_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&self.to_stored())?)
    }

    pub(crate) fn from_stored_bytes(bytes: &[u8]) -> Result<CurrentProjection> {
        Ok(CurrentProjection::from_stored(serde_json::from_slice(bytes)?))
    }

    /// The map itself, for the grounding differential in `engine.rs`.
    pub(crate) fn entries(&self) -> &BTreeMap<(String, String), Claim> {
        &self.entries
    }

    fn to_stored(&self) -> StoredProjection {
        StoredProjection {
            watermark: self.watermark,
            status: self.status.clone(),
            last_grounded: self.last_grounded,
            entries: self.entries.values().cloned().collect(),
        }
    }

    fn from_stored(stored: StoredProjection) -> Self {
        let mut projection = CurrentProjection {
            watermark: stored.watermark,
            status: stored.status,
            last_grounded: stored.last_grounded,
            entries: BTreeMap::new(),
        };
        projection.apply(&stored.entries);
        projection
    }

    /// Content digest over the sorted entries. FNV-1a 64, the same
    /// construction §13.2 uses elsewhere; serde_json field order is
    /// struct-declared and therefore stable.
    pub(crate) fn digest(&self) -> Result<u64> {
        let bytes = serde_json::to_vec(&self.to_stored().entries)?;
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Ok(hash)
    }
}

/// Entry-level differential between the recomputed and incremental maps. Each
/// line names the pair and the disagreement, so the divergence report is
/// evidence rather than a boolean.
pub(crate) fn difference(
    recomputed: &BTreeMap<(String, String), Claim>,
    incremental: &BTreeMap<(String, String), Claim>,
) -> Vec<String> {
    let mut out = Vec::new();
    for ((subject, predicate), claim) in recomputed {
        match incremental.get(&(subject.clone(), predicate.clone())) {
            None => out.push(format!("{subject}/{predicate}: missing from projection")),
            Some(held) if held != claim => out.push(format!(
                "{subject}/{predicate}: projection holds {:?} at ({}, {}), recomputation holds {:?} at ({}, {})",
                held.object, held.valid_from, held.tx_time,
                claim.object, claim.valid_from, claim.tx_time,
            )),
            Some(_) => {}
        }
    }
    for (subject, predicate) in incremental.keys() {
        if !recomputed.contains_key(&(subject.clone(), predicate.clone())) {
            out.push(format!("{subject}/{predicate}: absent from recomputation"));
        }
    }
    out
}
