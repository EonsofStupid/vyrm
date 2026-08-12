//! Removal candidacy. `SPEC.md` §7 and §12.
//!
//! Removal must be decidable by query rather than by argument. This module
//! derives candidates by joining the stored subject-predicate pairs against the
//! access records within a stated interval.
//!
//! Analysis only. No removal path exists, and none is specified: what becomes of
//! a retired claim, and how removal interacts with promotion, are not settled.
//! The report states what is unreferenced; the decision remains the operator's.

use std::collections::BTreeMap;
use vyrm_core::{Millis, Predicate, Reader, Subject};

/// Whether a subject-predicate pair is unreferenced over the interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// No access record within the interval.
    Candidate,
    /// Accessed within the interval, and therefore retained.
    Retained,
}

/// One pair, its verdict, and the evidence for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairStatus {
    pub subject: Subject,
    pub predicate: Predicate,
    pub verdict: Verdict,
    /// Claim versions stored for this pair.
    pub claim_count: usize,
    /// Most recent access within the interval, if any. This is the evidence.
    pub last_access: Option<Millis>,
    /// Reader responsible for that access.
    pub last_reader: Option<Reader>,
    /// Accesses observed within the interval.
    pub access_count: usize,
}

impl PairStatus {
    /// Human-readable justification. Every verdict cites its evidence.
    pub fn reason(&self) -> String {
        match self.verdict {
            Verdict::Retained => format!(
                "retained: {} access(es) in interval, most recent at {} by {}",
                self.access_count,
                self.last_access.unwrap_or_default(),
                self.last_reader
                    .as_ref()
                    .map(|r| r.as_str())
                    .unwrap_or("unknown"),
            ),
            Verdict::Candidate => format!(
                "candidate: {} claim version(s), no access in interval",
                self.claim_count
            ),
        }
    }
}

/// Result of a candidacy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalReport {
    /// Instant the evaluation was performed. Supplied by the caller; the kernel
    /// reads no clock.
    pub evaluated_at: Millis,
    /// Inclusive lower bound of the interval considered.
    pub since: Millis,
    /// Every stored pair, in key order, with its verdict.
    pub pairs: Vec<PairStatus>,
}

impl RemovalReport {
    pub fn candidates(&self) -> impl Iterator<Item = &PairStatus> {
        self.pairs.iter().filter(|p| p.verdict == Verdict::Candidate)
    }

    pub fn retained(&self) -> impl Iterator<Item = &PairStatus> {
        self.pairs.iter().filter(|p| p.verdict == Verdict::Retained)
    }

    /// Rendered report, one line per pair, each citing its evidence.
    pub fn render(&self) -> String {
        let mut out = format!(
            "removal candidacy: interval [{}, {}], {} pair(s), {} candidate(s)\n",
            self.since,
            self.evaluated_at,
            self.pairs.len(),
            self.candidates().count()
        );
        for pair in &self.pairs {
            out.push_str(&format!(
                "  {}/{}  {}\n",
                pair.subject,
                pair.predicate,
                pair.reason()
            ));
        }
        out
    }
}

/// Accumulates per-pair evidence during a scan.
#[derive(Default)]
pub(crate) struct Tally {
    pub claim_count: usize,
    pub access_count: usize,
    pub last_access: Option<Millis>,
    pub last_reader: Option<Reader>,
}

/// Joins claim and access tallies into a report.
///
/// A pair with claims but no access in the interval is a candidate. A pair with
/// any access is retained. Pairs are ordered by subject then predicate so the
/// report is stable across runs.
pub(crate) fn build_report(
    tallies: BTreeMap<(String, String), Tally>,
    since: Millis,
    evaluated_at: Millis,
) -> vyrm_core::Result<RemovalReport> {
    let mut pairs = Vec::with_capacity(tallies.len());
    for ((subject, predicate), tally) in tallies {
        // A pair present only in access records, with no stored claim, is not
        // reported: there is nothing to remove.
        if tally.claim_count == 0 {
            continue;
        }
        let verdict = if tally.access_count == 0 {
            Verdict::Candidate
        } else {
            Verdict::Retained
        };
        pairs.push(PairStatus {
            subject: Subject::new(subject)?,
            predicate: Predicate::new(predicate)?,
            verdict,
            claim_count: tally.claim_count,
            last_access: tally.last_access,
            last_reader: tally.last_reader,
            access_count: tally.access_count,
        });
    }
    Ok(RemovalReport {
        evaluated_at,
        since,
        pairs,
    })
}
