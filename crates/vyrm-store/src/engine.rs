//! The storage port. `PLAN.md` Step S: the ability to fold in storage.
//!
//! vyrm's value is the semantic layer — bi-temporal claims, durability
//! classes, projections that ground against their log, recall, the ledger.
//! The engine underneath is a *port*: eight primitives (append, sequence,
//! range, subjects, observe, projection get/put, and the `ClaimSource`
//! reads) that any backend can supply. Everything else — assert,
//! current-state projection, rebuild, grounding, quarantine, reset — is
//! **provided by this trait**, so an engine implements the primitives and
//! inherits the semantics. That layering is the contract a parity
//! implementation follows in another language: the Go/bbolt engine
//! implements these same primitives over the same key encodings
//! (`vyrm-core/fixtures/golden-vectors.json` is the cross-language proof)
//! and the semantic layer above it is a translation, not a redesign.
//!
//! Two engines ship in Rust: [`Store`] (Fjall — the measured default) and
//! [`MemoryEngine`] (the reference, for conformance differentials per
//! standing rule 3). Cache tiers (Moka in-process, Dragonfly shared)
//! compose *around* an engine rather than implementing this trait: they
//! accelerate reads and must never be the system of record.

use crate::error::{Error, Result};
use crate::keyspaces::Durability;
use crate::projection::{
    difference, CurrentProjection, GroundedStamp, GroundingReport, ProjectionStatus,
    CURRENT_PROJECTION,
};
use crate::store::{AppendOutcome, Store};
use std::collections::BTreeMap;
use std::sync::Mutex;
use vyrm_core::reference::MemoryClaims;
use vyrm_core::{Claim, ClaimSource, Millis, Predicate, Reader, Subject};

pub trait Engine: ClaimSource<Error = Error> {
    // ---- primitives every backend supplies ----

    /// Appends claims atomically with authoritative durability, advancing
    /// the sequence watermark in the same transaction.
    fn append_batch(&self, claims: &[Claim]) -> Result<AppendOutcome>;

    /// Current claim sequence watermark.
    fn sequence(&self) -> Result<u64>;

    /// Claims appended in `(from, to]`, in append order.
    fn claims_in_range(&self, from: u64, to: u64) -> Result<Vec<Claim>>;

    /// Every distinct subject with at least one claim, in key order.
    fn subjects(&self) -> Result<Vec<Subject>>;

    /// Records a read. Telemetry durability: loss on crash is acceptable.
    fn observe(
        &self,
        reader: &Reader,
        subject: &Subject,
        predicate: &Predicate,
        at: Millis,
    ) -> Result<()>;

    /// Loads a named projection blob. `None` means the caller rebuilds.
    fn get_projection(&self, name: &str) -> Result<Option<Vec<u8>>>;

    /// Stores a named projection blob with an explicit durability class.
    fn put_projection_with(&self, name: &str, bytes: &[u8], durability: Durability)
        -> Result<()>;

    // ---- provided: the semantic layer every engine inherits ----

    /// Appends a single claim. Equivalent to a batch of one.
    fn assert(&self, claim: &Claim) -> Result<AppendOutcome> {
        self.append_batch(std::slice::from_ref(claim))
    }

    /// [`Engine::put_projection_with`] at the Buffered default: a projection
    /// is derivable, so a crash-lost write costs a rebuild, never truth.
    fn put_projection(&self, name: &str, bytes: &[u8]) -> Result<()> {
        self.put_projection_with(name, bytes, Durability::Buffered)
    }

    /// Loads the current-state projection. Absence is the empty projection
    /// at watermark 0 — a recovery path, not an error.
    fn current_projection(&self) -> Result<CurrentProjection> {
        match self.get_projection(CURRENT_PROJECTION)? {
            Some(bytes) => Ok(CurrentProjection::from_stored_bytes(&bytes)?),
            None => Ok(CurrentProjection::empty()),
        }
    }

    /// §8.2: applies claims in `(watermark, current_sequence]` and advances
    /// the watermark in the same write as the projection. Refuses when
    /// quarantined — rebuilding on top of detected divergence would be the
    /// silent repair §8.3 forbids.
    #[tracing::instrument(level = "debug", skip_all)]
    fn rebuild_current(&self) -> Result<crate::projection::RebuildOutcome> {
        let mut projection = self.current_projection()?;
        if let ProjectionStatus::Quarantined { at, .. } = &projection.status {
            return Err(Error::Quarantined(format!(
                "projection `{CURRENT_PROJECTION}` quarantined at {at}; reset to recover"
            )));
        }
        let from = projection.watermark;
        let to = self.sequence()?;
        let interval = self.claims_in_range(from, to)?;
        let applied = interval.len();
        projection.apply(&interval);
        projection.watermark = to;
        self.put_projection_with(
            CURRENT_PROJECTION,
            &projection.to_stored_bytes()?,
            Durability::Buffered,
        )?;
        tracing::debug!(from, to, applied, "rebuild advanced the watermark");
        Ok(crate::projection::RebuildOutcome { from, to, applied })
    }

    /// §8.3: recomputes the projection from the sequence index at the
    /// projection's own watermark and differences the result against the
    /// incrementally maintained state. Empty differential stamps `grounded`;
    /// any difference quarantines the projection with Authoritative
    /// durability and reports it. Never repairs.
    #[tracing::instrument(level = "debug", skip_all)]
    fn ground_current(&self, at: Millis) -> Result<GroundingReport> {
        let mut projection = self.current_projection()?;
        if let ProjectionStatus::Quarantined { at, .. } = &projection.status {
            return Err(Error::Quarantined(format!(
                "projection `{CURRENT_PROJECTION}` quarantined at {at}; reset to recover"
            )));
        }

        let mut recomputed = CurrentProjection::empty();
        recomputed.apply(&self.claims_in_range(0, projection.watermark)?);

        let differences = difference(recomputed.entries(), projection.entries());
        if differences.is_empty() {
            let stamp = GroundedStamp {
                at,
                sequence: projection.watermark,
                digest: projection.digest()?,
            };
            projection.last_grounded = Some(stamp);
            self.put_projection_with(
                CURRENT_PROJECTION,
                &projection.to_stored_bytes()?,
                Durability::Buffered,
            )?;
            tracing::debug!(sequence = stamp.sequence, digest = stamp.digest, "grounded");
            return Ok(GroundingReport::Grounded(stamp));
        }

        projection.status = ProjectionStatus::Quarantined {
            at,
            differences: differences.clone(),
        };
        // The one derived-state write that pays for durability: a quarantine
        // a crash could forget would un-halt a diverged projection silently.
        self.put_projection_with(
            CURRENT_PROJECTION,
            &projection.to_stored_bytes()?,
            Durability::Authoritative,
        )?;
        tracing::warn!(differences = differences.len(), "divergence — projection quarantined");
        Ok(GroundingReport::Divergence { differences })
    }

    /// Operator recovery: discards the projection and recomputes it from the
    /// log. The only exit from quarantine, and explicit — recomputation
    /// *becoming* the projection is a decision, not a background repair.
    /// Buffered: losing this write resurrects the quarantine, which fails
    /// closed.
    fn reset_current(&self) -> Result<crate::projection::RebuildOutcome> {
        let to = self.sequence()?;
        let mut projection = CurrentProjection::empty();
        let interval = self.claims_in_range(0, to)?;
        let applied = interval.len();
        projection.apply(&interval);
        projection.watermark = to;
        self.put_projection_with(
            CURRENT_PROJECTION,
            &projection.to_stored_bytes()?,
            Durability::Buffered,
        )?;
        Ok(crate::projection::RebuildOutcome { from: 0, to, applied })
    }
}

impl Engine for Store {
    fn append_batch(&self, claims: &[Claim]) -> Result<AppendOutcome> {
        Store::append_batch(self, claims)
    }
    fn sequence(&self) -> Result<u64> {
        Store::sequence(self)
    }
    fn claims_in_range(&self, from: u64, to: u64) -> Result<Vec<Claim>> {
        Store::claims_in_range(self, from, to)
    }
    fn subjects(&self) -> Result<Vec<Subject>> {
        Store::subjects(self)
    }
    fn observe(
        &self,
        reader: &Reader,
        subject: &Subject,
        predicate: &Predicate,
        at: Millis,
    ) -> Result<()> {
        Store::observe(self, reader, subject, predicate, at)
    }
    fn get_projection(&self, name: &str) -> Result<Option<Vec<u8>>> {
        Store::get_projection(self, name)
    }
    fn put_projection_with(&self, name: &str, bytes: &[u8], durability: Durability) -> Result<()> {
        Store::put_projection_with(self, name, bytes, durability)
    }
}

/// The reference engine: `MemoryClaims` plus the primitives, behind a
/// mutex. Exists so conformance is a differential between two engines
/// (standing rule 3) and so the runtime layer is provably generic over the
/// port. Not a production store — nothing here survives the process.
#[derive(Default)]
pub struct MemoryEngine {
    inner: Mutex<MemoryEngineInner>,
}

#[derive(Default)]
struct MemoryEngineInner {
    claims: MemoryClaims,
    /// Append order, so `claims_in_range` replays exactly like a log.
    order: Vec<Claim>,
    projections: BTreeMap<String, Vec<u8>>,
    observes: u64,
}

impl MemoryEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe calls recorded, for tests that assert telemetry flowed.
    pub fn observe_count(&self) -> u64 {
        self.inner.lock().expect("engine mutex").observes
    }
}

impl ClaimSource for MemoryEngine {
    type Error = Error;

    fn versions_at_or_before(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        as_of: Millis,
    ) -> Result<Vec<Claim>> {
        let inner = self.inner.lock().expect("engine mutex");
        Ok(infallible(inner.claims.versions_at_or_before(subject, predicate, as_of)))
    }

    fn all_versions(&self, subject: &Subject, predicate: &Predicate) -> Result<Vec<Claim>> {
        let inner = self.inner.lock().expect("engine mutex");
        Ok(infallible(inner.claims.all_versions(subject, predicate)))
    }

    fn subject_versions(&self, subject: &Subject) -> Result<Vec<Claim>> {
        let inner = self.inner.lock().expect("engine mutex");
        Ok(infallible(inner.claims.subject_versions(subject)))
    }
}

fn infallible<T>(result: std::result::Result<T, std::convert::Infallible>) -> T {
    match result {
        Ok(value) => value,
    }
}

impl Engine for MemoryEngine {
    fn append_batch(&self, claims: &[Claim]) -> Result<AppendOutcome> {
        let mut inner = self.inner.lock().expect("engine mutex");
        let start = inner.order.len() as u64;
        for claim in claims {
            inner.claims.insert(claim.clone())?;
            inner.order.push(claim.clone());
        }
        Ok(AppendOutcome {
            first_sequence: start + 1,
            last_sequence: start + claims.len() as u64,
            count: claims.len(),
        })
    }

    fn sequence(&self) -> Result<u64> {
        Ok(self.inner.lock().expect("engine mutex").order.len() as u64)
    }

    fn claims_in_range(&self, from: u64, to: u64) -> Result<Vec<Claim>> {
        let inner = self.inner.lock().expect("engine mutex");
        let end = (to as usize).min(inner.order.len());
        if from as usize >= end {
            return Ok(Vec::new());
        }
        Ok(inner.order[from as usize..end].to_vec())
    }

    fn subjects(&self) -> Result<Vec<Subject>> {
        let inner = self.inner.lock().expect("engine mutex");
        let mut out: Vec<Subject> = Vec::new();
        for claim in inner.claims.iter() {
            if out.last().map(|s| s.as_str()) != Some(claim.subject.as_str()) {
                out.push(claim.subject.clone());
            }
        }
        out.dedup_by(|a, b| a.as_str() == b.as_str());
        Ok(out)
    }

    fn observe(&self, _: &Reader, _: &Subject, _: &Predicate, _: Millis) -> Result<()> {
        self.inner.lock().expect("engine mutex").observes += 1;
        Ok(())
    }

    fn get_projection(&self, name: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.inner.lock().expect("engine mutex").projections.get(name).cloned())
    }

    fn put_projection_with(&self, name: &str, bytes: &[u8], _: Durability) -> Result<()> {
        self.inner
            .lock()
            .expect("engine mutex")
            .projections
            .insert(name.to_owned(), bytes.to_vec());
        Ok(())
    }
}
