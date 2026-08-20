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
//! Three engines ship in Rust today: [`Store`] (the transitional Fjall
//! compatibility adapter), [`crate::NativeEngine`] (the Vyrm-native target),
//! and [`MemoryEngine`] (the reference, for conformance differentials per
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
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use vyrm_core::reference::MemoryClaims;
use vyrm_core::{
    projection_family, resolve_as_of, AuditEnvelope, Claim, ClaimSource, DataTransaction,
    DataTransactionView, Millis, ObjectReference, Predicate, ProjectionWork, ReadStamp, Reader,
    RetentionPin, RuntimeChange, RuntimeChangePage, RuntimeCommit, RuntimeCommitOutcome,
    RuntimeGeo, RuntimeGraphSnapshot, RuntimeMutation, RuntimeRecord, RuntimeRef, RuntimeRelation,
    RuntimeSchemaRegistry, RuntimeSeriesSample, RuntimeVector, ScopeId, SnapshotHandle, SnapshotId,
    Subject,
};

/// A read-only physical counter snapshot used to attribute bounded storage
/// work to one logical operation. Counters are cumulative; callers difference
/// two snapshots taken immediately around the work. Backends without stable
/// physical counters return `logical_only` evidence instead of inventing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalStoreEvidence {
    pub backend: String,
    pub evidence_level: String,
    pub physical_sequence: Option<u64>,
    pub manifest_generation: Option<u64>,
    pub durable_sequence: Option<u64>,
    pub memtable_versions: Option<u64>,
    pub memtable_bytes: Option<u64>,
    pub memtable_max_versions: Option<u64>,
    pub wal_payload_bytes: Option<u64>,
    pub wal_payload_max_bytes: Option<u64>,
    pub automatic_flushes: Option<u64>,
    pub maintenance_write_stalls: Option<u64>,
    pub failed_maintenance_flushes: Option<u64>,
    pub oversized_batches: Option<u64>,
    pub segment_count: Option<u64>,
    pub segment_bytes: Option<u64>,
    pub cache_capacity_bytes: Option<u64>,
    pub cache_resident_bytes: Option<u64>,
    pub cache_entries: Option<u64>,
    pub cache_hits: Option<u64>,
    pub cache_misses: Option<u64>,
    pub cache_evictions: Option<u64>,
    pub block_loads: Option<u64>,
    pub block_bytes_loaded: Option<u64>,
    pub block_bytes_decoded: Option<u64>,
}

impl PhysicalStoreEvidence {
    pub fn logical_only(backend: impl Into<String>) -> Self {
        Self {
            backend: backend.into(),
            evidence_level: "logical_only".into(),
            physical_sequence: None,
            manifest_generation: None,
            durable_sequence: None,
            memtable_versions: None,
            memtable_bytes: None,
            memtable_max_versions: None,
            wal_payload_bytes: None,
            wal_payload_max_bytes: None,
            automatic_flushes: None,
            maintenance_write_stalls: None,
            failed_maintenance_flushes: None,
            oversized_batches: None,
            segment_count: None,
            segment_bytes: None,
            cache_capacity_bytes: None,
            cache_resident_bytes: None,
            cache_entries: None,
            cache_hits: None,
            cache_misses: None,
            cache_evictions: None,
            block_loads: None,
            block_bytes_loaded: None,
            block_bytes_decoded: None,
        }
    }
}

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
    fn put_projection_with(&self, name: &str, bytes: &[u8], durability: Durability) -> Result<()>;

    /// Current global cursor of the authoritative typed runtime log.
    fn runtime_cursor(&self) -> Result<u64>;

    /// Latest authoritative schema registry for one runtime scope.
    fn runtime_schema(&self, scope: &ScopeId) -> Result<Option<RuntimeSchemaRegistry>>;

    /// Atomically captures the cursor, schema revision, and hash-chain head
    /// that make one logical read state reproducible across adapters.
    fn runtime_read_stamp(&self, scope: &ScopeId) -> Result<ReadStamp>;

    /// Persists a leased snapshot handle over one atomic read stamp.
    fn open_runtime_snapshot(
        &self,
        scope: &ScopeId,
        owner: &str,
        now: Millis,
        ttl: Millis,
    ) -> Result<SnapshotHandle>;

    /// Reads a bounded page that can never advance beyond the captured stamp.
    fn runtime_snapshot_changes(
        &self,
        snapshot: &SnapshotHandle,
        after: u64,
        limit: usize,
        now: Millis,
    ) -> Result<RuntimeChangePage>;

    /// Releases one persisted lease. Releasing an absent lease is idempotent.
    fn release_runtime_snapshot(&self, id: &SnapshotId) -> Result<bool>;

    /// Lists non-expired persisted leases in stable identity order.
    fn runtime_snapshots(&self, now: Millis) -> Result<Vec<SnapshotHandle>>;

    /// Lists the logical retention roots implied by every live snapshot.
    fn runtime_retention_pins(&self, now: Millis) -> Result<Vec<RetentionPin>>;

    /// Reads against an exact stamped state without creating a durable lease.
    /// This is reserved for the short lifetime of a data transaction; scans
    /// that outlive a transaction must use a persisted snapshot handle.
    fn runtime_read_changes(
        &self,
        read: &ReadStamp,
        after: u64,
        limit: usize,
    ) -> Result<RuntimeChangePage>;

    /// Atomically commits typed runtime mutations with exact-cursor conflict
    /// detection. Embedded claims join the same storage transaction.
    fn commit_runtime(&self, commit: &RuntimeCommit) -> Result<RuntimeCommitOutcome>;

    /// Reads a bounded, resumable page of runtime changes.
    fn runtime_changes_since(
        &self,
        after: u64,
        limit: usize,
        scope: Option<&ScopeId>,
    ) -> Result<RuntimeChangePage>;

    /// Reads durable projection work after a source cursor.
    fn runtime_outbox_since(&self, after: u64, limit: usize) -> Result<Vec<ProjectionWork>>;

    /// Reads the accepted-operation audit envelope for one commit.
    fn runtime_audit(&self, commit_id: &str) -> Result<Option<AuditEnvelope>>;

    /// Looks up a durably accepted content identity for coordinator retry.
    fn runtime_commit_outcome(&self, commit_id: &str) -> Result<Option<RuntimeCommitOutcome>>;

    /// Captures cumulative, non-mutating physical counters. This deliberately
    /// does not emit a trace: callers bracket logical work and persist one
    /// bounded summary afterward, avoiding recursive tracing of the trace log.
    fn physical_store_evidence(&self) -> Result<PhysicalStoreEvidence> {
        Ok(PhysicalStoreEvidence::logical_only("unspecified"))
    }

    /// Commits a mutation envelope bound to its exact read stamp. The existing
    /// runtime CAS remains the final race-proof authority.
    fn commit_data_transaction(
        &self,
        transaction: &DataTransaction,
    ) -> Result<RuntimeCommitOutcome> {
        transaction.validate()?;
        self.commit_runtime(&transaction.commit)
    }

    /// Reconstructs the stamped base graph and overlays pending writes. The
    /// result is prospective and cannot be confused with committed evidence.
    fn preview_data_transaction(
        &self,
        transaction: &DataTransaction,
        valid_at: Millis,
    ) -> Result<DataTransactionView> {
        transaction.validate()?;
        let page = self.runtime_read_changes(&transaction.read, 0, usize::MAX)?;
        if page.through_cursor != transaction.read.commit_cursor {
            return Err(Error::Substrate(format!(
                "stamped runtime replay ended at {}, expected {}",
                page.through_cursor, transaction.read.commit_cursor
            )));
        }
        let base = RuntimeGraphSnapshot::from_changes(
            &page.changes,
            transaction.read.scope.clone(),
            valid_at,
            transaction.read.commit_cursor,
        );
        transaction.preview(&base).map_err(Error::from)
    }

    // ---- provided: the semantic layer every engine inherits ----

    /// Appends a single claim. Equivalent to a batch of one.
    fn assert(&self, claim: &Claim) -> Result<AppendOutcome> {
        let candidates =
            self.versions_at_or_before(&claim.subject, &claim.predicate, claim.valid_from)?;
        let previous = resolve_as_of(&candidates, claim.valid_from).cloned();
        match previous {
            Some(previous) if previous.valid_from < claim.valid_from => {
                let pair = vyrm_core::supersede(&previous, claim.clone())?;
                self.append_batch(&pair)
            }
            _ => self.append_batch(std::slice::from_ref(claim)),
        }
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
        tracing::warn!(
            differences = differences.len(),
            "divergence — projection quarantined"
        );
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
        Ok(crate::projection::RebuildOutcome {
            from: 0,
            to,
            applied,
        })
    }
}

impl Engine for Store {
    fn physical_store_evidence(&self) -> Result<PhysicalStoreEvidence> {
        Ok(PhysicalStoreEvidence::logical_only("fjall_compatibility"))
    }

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
    fn runtime_cursor(&self) -> Result<u64> {
        Store::runtime_cursor(self)
    }
    fn runtime_schema(&self, scope: &ScopeId) -> Result<Option<RuntimeSchemaRegistry>> {
        Store::runtime_schema(self, scope)
    }
    fn runtime_read_stamp(&self, scope: &ScopeId) -> Result<ReadStamp> {
        Store::runtime_read_stamp(self, scope)
    }
    fn open_runtime_snapshot(
        &self,
        scope: &ScopeId,
        owner: &str,
        now: Millis,
        ttl: Millis,
    ) -> Result<SnapshotHandle> {
        Store::open_runtime_snapshot(self, scope, owner, now, ttl)
    }
    fn runtime_snapshot_changes(
        &self,
        snapshot: &SnapshotHandle,
        after: u64,
        limit: usize,
        now: Millis,
    ) -> Result<RuntimeChangePage> {
        Store::runtime_snapshot_changes(self, snapshot, after, limit, now)
    }
    fn release_runtime_snapshot(&self, id: &SnapshotId) -> Result<bool> {
        Store::release_runtime_snapshot(self, id)
    }
    fn runtime_snapshots(&self, now: Millis) -> Result<Vec<SnapshotHandle>> {
        Store::runtime_snapshots(self, now)
    }
    fn runtime_retention_pins(&self, now: Millis) -> Result<Vec<RetentionPin>> {
        Store::runtime_retention_pins(self, now)
    }
    fn runtime_read_changes(
        &self,
        read: &ReadStamp,
        after: u64,
        limit: usize,
    ) -> Result<RuntimeChangePage> {
        Store::runtime_read_changes(self, read, after, limit)
    }
    fn commit_runtime(&self, commit: &RuntimeCommit) -> Result<RuntimeCommitOutcome> {
        Store::commit_runtime(self, commit)
    }
    fn runtime_changes_since(
        &self,
        after: u64,
        limit: usize,
        scope: Option<&ScopeId>,
    ) -> Result<RuntimeChangePage> {
        Store::runtime_changes_since(self, after, limit, scope)
    }
    fn runtime_outbox_since(&self, after: u64, limit: usize) -> Result<Vec<ProjectionWork>> {
        Store::runtime_outbox_since(self, after, limit)
    }
    fn runtime_audit(&self, commit_id: &str) -> Result<Option<AuditEnvelope>> {
        Store::runtime_audit(self, commit_id)
    }
    fn runtime_commit_outcome(&self, commit_id: &str) -> Result<Option<RuntimeCommitOutcome>> {
        Store::runtime_commit_outcome(self, commit_id)
    }
}

/// The reference engine: `MemoryClaims` plus the primitives, behind a
/// mutex. Exists so conformance is a differential across all engines
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
    runtime_changes: Vec<RuntimeChange>,
    runtime_records: BTreeMap<(ScopeId, RuntimeRef), RuntimeRecord>,
    runtime_relations: BTreeMap<(ScopeId, RuntimeRef), RuntimeRelation>,
    runtime_vectors: BTreeMap<(ScopeId, RuntimeRef), RuntimeVector>,
    runtime_series: BTreeMap<(ScopeId, RuntimeRef), RuntimeSeriesSample>,
    runtime_geo: BTreeMap<(ScopeId, RuntimeRef), RuntimeGeo>,
    runtime_objects: BTreeMap<(ScopeId, RuntimeRef), ObjectReference>,
    runtime_outbox: BTreeMap<u64, ProjectionWork>,
    runtime_audit: BTreeMap<String, AuditEnvelope>,
    runtime_last_audit_digest: Option<String>,
    runtime_commits: BTreeMap<String, RuntimeCommitOutcome>,
    runtime_schemas: BTreeMap<ScopeId, RuntimeSchemaRegistry>,
    runtime_snapshots: BTreeMap<SnapshotId, SnapshotHandle>,
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
        Ok(infallible(
            inner
                .claims
                .versions_at_or_before(subject, predicate, as_of),
        ))
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
    fn physical_store_evidence(&self) -> Result<PhysicalStoreEvidence> {
        Ok(PhysicalStoreEvidence::logical_only("memory"))
    }

    fn append_batch(&self, claims: &[Claim]) -> Result<AppendOutcome> {
        let mut inner = self.inner.lock().expect("engine mutex");
        // Match Fjall rollback: reject the whole batch before mutating either
        // authoritative collection if any member is invalid.
        for claim in claims {
            claim.validate()?;
        }
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
        Ok(self
            .inner
            .lock()
            .expect("engine mutex")
            .projections
            .get(name)
            .cloned())
    }

    fn put_projection_with(&self, name: &str, bytes: &[u8], _: Durability) -> Result<()> {
        self.inner
            .lock()
            .expect("engine mutex")
            .projections
            .insert(name.to_owned(), bytes.to_vec());
        Ok(())
    }

    fn runtime_cursor(&self) -> Result<u64> {
        Ok(self
            .inner
            .lock()
            .expect("engine mutex")
            .runtime_changes
            .len() as u64)
    }

    fn runtime_schema(&self, scope: &ScopeId) -> Result<Option<RuntimeSchemaRegistry>> {
        Ok(self
            .inner
            .lock()
            .expect("engine mutex")
            .runtime_schemas
            .get(scope)
            .cloned())
    }

    fn runtime_read_stamp(&self, scope: &ScopeId) -> Result<ReadStamp> {
        let inner = self.inner.lock().expect("engine mutex");
        memory_read_stamp(&inner, scope)
    }

    fn open_runtime_snapshot(
        &self,
        scope: &ScopeId,
        owner: &str,
        now: Millis,
        ttl: Millis,
    ) -> Result<SnapshotHandle> {
        let mut inner = self.inner.lock().expect("engine mutex");
        let handle = SnapshotHandle::new(memory_read_stamp(&inner, scope)?, owner, now, ttl)?;
        inner
            .runtime_snapshots
            .insert(handle.id.clone(), handle.clone());
        Ok(handle)
    }

    fn runtime_snapshot_changes(
        &self,
        snapshot: &SnapshotHandle,
        after: u64,
        limit: usize,
        now: Millis,
    ) -> Result<RuntimeChangePage> {
        snapshot.validate()?;
        if limit == 0 {
            return Err(Error::Substrate(
                "runtime change page limit must be greater than zero".into(),
            ));
        }
        let inner = self.inner.lock().expect("engine mutex");
        let Some(persisted) = inner.runtime_snapshots.get(&snapshot.id) else {
            return Err(Error::SnapshotNotFound(snapshot.id.to_string()));
        };
        if persisted != snapshot {
            return Err(Error::SnapshotMismatch(snapshot.id.to_string()));
        }
        if snapshot.is_expired(now) {
            return Err(Error::SnapshotExpired {
                id: snapshot.id.to_string(),
                expired_at: snapshot.expires_at,
            });
        }
        Ok(memory_change_page(
            &inner.runtime_changes,
            snapshot.read.commit_cursor,
            after,
            limit,
            Some(&snapshot.read.scope),
        ))
    }

    fn release_runtime_snapshot(&self, id: &SnapshotId) -> Result<bool> {
        Ok(self
            .inner
            .lock()
            .expect("engine mutex")
            .runtime_snapshots
            .remove(id)
            .is_some())
    }

    fn runtime_snapshots(&self, now: Millis) -> Result<Vec<SnapshotHandle>> {
        Ok(self
            .inner
            .lock()
            .expect("engine mutex")
            .runtime_snapshots
            .values()
            .filter(|snapshot| !snapshot.is_expired(now))
            .cloned()
            .collect())
    }

    fn runtime_retention_pins(&self, now: Millis) -> Result<Vec<RetentionPin>> {
        self.runtime_snapshots(now)?
            .iter()
            .map(RetentionPin::from_snapshot)
            .collect::<vyrm_core::Result<Vec<_>>>()
            .map_err(Error::from)
    }

    fn runtime_read_changes(
        &self,
        read: &ReadStamp,
        after: u64,
        limit: usize,
    ) -> Result<RuntimeChangePage> {
        if limit == 0 {
            return Err(Error::Substrate(
                "runtime change page limit must be greater than zero".into(),
            ));
        }
        let inner = self.inner.lock().expect("engine mutex");
        memory_validate_read_stamp(&inner, read)?;
        Ok(memory_change_page(
            &inner.runtime_changes,
            read.commit_cursor,
            after,
            limit,
            Some(&read.scope),
        ))
    }

    fn commit_runtime(&self, commit: &RuntimeCommit) -> Result<RuntimeCommitOutcome> {
        commit.validate()?;
        let mut inner = self.inner.lock().expect("engine mutex");
        let commit_id = commit.digest();
        let start = inner.runtime_changes.len() as u64;
        if start != commit.expected_cursor {
            return Err(Error::RuntimeConflict {
                expected: commit.expected_cursor,
                actual: start,
            });
        }
        let previous_schema = inner.runtime_schemas.get(&commit.scope);
        let proposed_schema = commit.mutations.iter().find_map(|mutation| match mutation {
            RuntimeMutation::Schema { registry } => Some(registry),
            _ => None,
        });
        let effective_schema = match (previous_schema, proposed_schema) {
            (None, Some(registry)) if registry.revision == 1 => registry,
            (None, Some(registry)) => {
                return Err(Error::RuntimeSchemaConflict {
                    expected: 1,
                    actual: registry.revision,
                });
            }
            (Some(previous), Some(registry))
                if registry.revision == previous.revision.saturating_add(1) =>
            {
                registry
            }
            (Some(previous), Some(registry)) => {
                return Err(Error::RuntimeSchemaConflict {
                    expected: previous.revision.saturating_add(1),
                    actual: registry.revision,
                });
            }
            (Some(previous), None) => previous,
            (None, None) => {
                return Err(Error::RuntimeSchemaMissing(commit.scope.to_string()));
            }
        };
        let existing_records = inner
            .runtime_records
            .iter()
            .filter(|((scope, _), _)| scope == &commit.scope)
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        let existing_relations = inner
            .runtime_relations
            .iter()
            .filter(|((scope, _), _)| scope == &commit.scope)
            .map(|(_, relation)| relation)
            .collect::<Vec<_>>();
        effective_schema.validate_objects(
            &commit.mutations,
            existing_records,
            existing_relations,
        )?;
        let new_records = commit
            .mutations
            .iter()
            .filter_map(|mutation| match mutation {
                RuntimeMutation::Record { record } => Some(record.reference.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        for mutation in &commit.mutations {
            let references: Vec<&RuntimeRef> = match mutation {
                RuntimeMutation::Relation { relation } => vec![&relation.from, &relation.to],
                RuntimeMutation::Event { event } => event.subject.iter().collect(),
                RuntimeMutation::Vector { vector } => vec![&vector.subject],
                RuntimeMutation::SeriesSample { sample } => vec![&sample.series],
                RuntimeMutation::Geo { geo } => vec![&geo.subject],
                RuntimeMutation::Object { object } => object.subject.iter().collect(),
                RuntimeMutation::Claim { .. }
                | RuntimeMutation::Schema { .. }
                | RuntimeMutation::Record { .. } => Vec::new(),
            };
            for reference in references {
                if !new_records.contains(reference)
                    && !inner
                        .runtime_records
                        .contains_key(&(commit.scope.clone(), reference.clone()))
                {
                    return Err(Error::DanglingRuntimeReference(format!(
                        "{}/{} in scope {}",
                        reference.kind, reference.id, commit.scope
                    )));
                }
            }
        }

        let claims = commit
            .mutations
            .iter()
            .filter_map(|mutation| match mutation {
                RuntimeMutation::Claim { claim } => Some(claim.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for claim in &claims {
            claim.validate()?;
        }
        let claim_start = inner.order.len() as u64;
        for claim in claims.iter().cloned() {
            inner.claims.insert(claim.clone())?;
            inner.order.push(claim);
        }

        let mut previous_digest = inner
            .runtime_changes
            .last()
            .map(|change| change.digest.clone());
        let mut committed = Vec::with_capacity(commit.mutations.len());
        let mut pending_work = Vec::new();
        for (ordinal, mutation) in commit.mutations.iter().cloned().enumerate() {
            let cursor = start + ordinal as u64 + 1;
            if let Some(family) = projection_family(&mutation) {
                pending_work.push(ProjectionWork::for_change(
                    commit.scope.clone(),
                    cursor,
                    commit_id.clone(),
                    ordinal as u64,
                    family,
                )?);
            }
            let change = RuntimeChange::committed(
                cursor,
                commit,
                &commit_id,
                ordinal as u64,
                mutation,
                previous_digest.clone(),
            );
            previous_digest = Some(change.digest.clone());
            committed.push(change);
        }
        let last_cursor = start + commit.mutations.len() as u64;
        let audit = AuditEnvelope::accepted_commit(
            commit,
            &commit_id,
            last_cursor,
            inner.runtime_last_audit_digest.clone(),
        )?;
        for mutation in &commit.mutations {
            match mutation {
                RuntimeMutation::Schema { registry } => {
                    inner
                        .runtime_schemas
                        .insert(commit.scope.clone(), registry.clone());
                }
                RuntimeMutation::Record { record } => {
                    inner.runtime_records.insert(
                        (commit.scope.clone(), record.reference.clone()),
                        record.clone(),
                    );
                }
                RuntimeMutation::Relation { relation } => {
                    inner.runtime_relations.insert(
                        (commit.scope.clone(), relation.reference.clone()),
                        relation.clone(),
                    );
                }
                RuntimeMutation::Vector { vector } => {
                    inner.runtime_vectors.insert(
                        (commit.scope.clone(), vector.reference.clone()),
                        vector.clone(),
                    );
                }
                RuntimeMutation::SeriesSample { sample } => {
                    inner.runtime_series.insert(
                        (commit.scope.clone(), sample.reference.clone()),
                        sample.clone(),
                    );
                }
                RuntimeMutation::Geo { geo } => {
                    inner
                        .runtime_geo
                        .insert((commit.scope.clone(), geo.reference.clone()), geo.clone());
                }
                RuntimeMutation::Object { object } => {
                    inner.runtime_objects.insert(
                        (commit.scope.clone(), object.reference.clone()),
                        object.clone(),
                    );
                }
                RuntimeMutation::Claim { .. } | RuntimeMutation::Event { .. } => {}
            }
        }
        inner.runtime_changes.extend(committed);
        let outbox_count = pending_work.len();
        for work in pending_work {
            inner.runtime_outbox.insert(work.source_cursor, work);
        }
        inner.runtime_last_audit_digest = Some(audit.digest.clone());
        inner.runtime_audit.insert(commit_id.clone(), audit);
        let claim_count = claims.len();
        let outcome = RuntimeCommitOutcome {
            commit_id,
            first_cursor: start + 1,
            last_cursor: inner.runtime_changes.len() as u64,
            count: commit.mutations.len(),
            first_claim_sequence: (claim_count > 0).then_some(claim_start + 1),
            last_claim_sequence: (claim_count > 0).then_some(claim_start + claim_count as u64),
            outbox_count,
        };
        inner
            .runtime_commits
            .insert(outcome.commit_id.clone(), outcome.clone());
        Ok(outcome)
    }

    fn runtime_changes_since(
        &self,
        after: u64,
        limit: usize,
        scope: Option<&ScopeId>,
    ) -> Result<RuntimeChangePage> {
        if limit == 0 {
            return Err(Error::Substrate(
                "runtime change page limit must be greater than zero".into(),
            ));
        }
        let inner = self.inner.lock().expect("engine mutex");
        Ok(memory_change_page(
            &inner.runtime_changes,
            inner.runtime_changes.len() as u64,
            after,
            limit,
            scope,
        ))
    }

    fn runtime_outbox_since(&self, after: u64, limit: usize) -> Result<Vec<ProjectionWork>> {
        if limit == 0 {
            return Err(Error::Substrate(
                "runtime outbox page limit must be greater than zero".into(),
            ));
        }
        let inner = self.inner.lock().expect("engine mutex");
        Ok(inner
            .runtime_outbox
            .range((after.saturating_add(1))..)
            .take(limit)
            .map(|(_, work)| work.clone())
            .collect())
    }

    fn runtime_audit(&self, commit_id: &str) -> Result<Option<AuditEnvelope>> {
        Ok(self
            .inner
            .lock()
            .expect("engine mutex")
            .runtime_audit
            .get(commit_id)
            .cloned())
    }

    fn runtime_commit_outcome(&self, commit_id: &str) -> Result<Option<RuntimeCommitOutcome>> {
        Ok(self
            .inner
            .lock()
            .expect("engine mutex")
            .runtime_commits
            .get(commit_id)
            .cloned())
    }
}

fn memory_read_stamp(inner: &MemoryEngineInner, scope: &ScopeId) -> Result<ReadStamp> {
    ReadStamp::new(
        scope.clone(),
        inner
            .runtime_schemas
            .get(scope)
            .map(|schema| schema.revision),
        0,
        inner.runtime_changes.len() as u64,
        inner
            .runtime_changes
            .last()
            .map(|change| change.digest.clone()),
    )
    .map_err(Error::from)
}

fn memory_validate_read_stamp(inner: &MemoryEngineInner, read: &ReadStamp) -> Result<()> {
    read.validate()?;
    if read.commit_cursor > inner.runtime_changes.len() as u64 {
        return Err(Error::ReadStampUnavailable(read.manifest_id.clone()));
    }
    let head_digest = if read.commit_cursor == 0 {
        None
    } else {
        Some(
            inner.runtime_changes[read.commit_cursor as usize - 1]
                .digest
                .clone(),
        )
    };
    let schema_revision = inner
        .runtime_changes
        .iter()
        .take(read.commit_cursor as usize)
        .filter(|change| change.scope == read.scope)
        .filter_map(|change| match &change.mutation {
            RuntimeMutation::Schema { registry } => Some(registry.revision),
            _ => None,
        })
        .next_back();
    if read.catalog_revision != 0
        || read.head_digest != head_digest
        || read.schema_revision != schema_revision
    {
        return Err(Error::ReadStampMismatch(read.manifest_id.clone()));
    }
    Ok(())
}

fn memory_change_page(
    changes: &[RuntimeChange],
    head: u64,
    after: u64,
    limit: usize,
    scope: Option<&ScopeId>,
) -> RuntimeChangePage {
    if after == u64::MAX || after >= head {
        return RuntimeChangePage {
            requested_after: after,
            through_cursor: after,
            head_cursor: head,
            changes: Vec::new(),
        };
    }
    let end = (after as usize)
        .saturating_add(limit)
        .min(head as usize)
        .min(changes.len());
    let selected = changes[after as usize..end]
        .iter()
        .filter(|change| scope.is_none_or(|scope| scope == &change.scope))
        .cloned()
        .collect();
    RuntimeChangePage {
        requested_after: after,
        through_cursor: end as u64,
        head_cursor: head,
        changes: selected,
    }
}
