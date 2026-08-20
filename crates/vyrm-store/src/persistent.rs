//! Canonical durable engine selector.
//!
//! New stores use native `vyrmKV`. A directory carrying native's authenticated
//! `CURRENT` pointer reopens as native; any other existing directory remains on
//! the Fjall compatibility adapter. Selection is therefore stable across
//! restart and never guesses that an existing store can be reinterpreted.

use crate::{
    migration_status, Durability, Engine, Error, Invocation, InvocationInput, MigrationPhase,
    NativeEngine, RecallOutcome, RemovalReport, Result, Store,
};
use std::path::Path;
use vyrm_core::{
    AuditEnvelope, Claim, ClaimSource, DataTransaction, DataTransactionView, Millis, Predicate,
    ProjectionWork, ReadStamp, Reader, RetentionPin, RuntimeChangePage, RuntimeCommit,
    RuntimeCommitOutcome, RuntimeSchemaRegistry, ScopeId, SnapshotHandle, SnapshotId, Subject,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistentBackend {
    Native,
    FjallCompatibility,
}

impl PersistentBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "vyrmkv_native",
            Self::FjallCompatibility => "fjall_compatibility",
        }
    }
}

pub enum PersistentEngine {
    Native(NativeEngine),
    FjallCompatibility(Store),
}

impl PersistentEngine {
    /// Opens a stable on-disk identity. Missing paths become native stores;
    /// existing non-native directories remain Fjall until an explicit migration.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(report) = migration_status(path)? {
            match report.phase {
                MigrationPhase::Complete | MigrationPhase::RolledBack => {}
                phase => {
                    return Err(Error::Migration(format!(
                        "database has an active {phase:?} migration; resume or roll it back"
                    )))
                }
            }
        }
        let empty = path.exists()
            && path.is_dir()
            && std::fs::read_dir(path)
                .map_err(|error| Error::Substrate(error.to_string()))?
                .next()
                .is_none();
        if !path.exists()
            || empty
            || path.join("CURRENT").is_file()
            || path.join("MANIFEST.LOCK").is_file()
        {
            return Ok(Self::Native(NativeEngine::open(path)?));
        }
        Ok(Self::FjallCompatibility(Store::open(path)?))
    }

    pub fn backend(&self) -> PersistentBackend {
        match self {
            Self::Native(_) => PersistentBackend::Native,
            Self::FjallCompatibility(_) => PersistentBackend::FjallCompatibility,
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Native(engine) => engine.path(),
            Self::FjallCompatibility(engine) => engine.path(),
        }
    }

    pub fn removal_report(&self, since: Millis, evaluated_at: Millis) -> Result<RemovalReport> {
        match self {
            Self::Native(engine) => engine.removal_report(since, evaluated_at),
            Self::FjallCompatibility(engine) => engine.removal_report(since, evaluated_at),
        }
    }

    pub fn access_count(&self) -> Result<usize> {
        match self {
            Self::Native(engine) => engine.access_count(),
            Self::FjallCompatibility(engine) => Ok(engine.access_count()),
        }
    }

    pub fn record_invocation(&self, input: InvocationInput<'_>) -> Result<Invocation> {
        match self {
            Self::Native(engine) => engine.record_invocation(input),
            Self::FjallCompatibility(engine) => engine.record_invocation(input),
        }
    }

    pub fn set_recall_outcome(&self, ordinal: u64, outcome: RecallOutcome) -> Result<Invocation> {
        match self {
            Self::Native(engine) => engine.set_recall_outcome(ordinal, outcome),
            Self::FjallCompatibility(engine) => engine.set_recall_outcome(ordinal, outcome),
        }
    }

    pub fn invocations_since(&self, since: Millis) -> Result<Vec<Invocation>> {
        match self {
            Self::Native(engine) => engine.invocations_since(since),
            Self::FjallCompatibility(engine) => engine.invocations_since(since),
        }
    }

    pub fn invocation_count(&self) -> Result<u64> {
        match self {
            Self::Native(engine) => engine.invocation_count(),
            Self::FjallCompatibility(engine) => engine.invocation_count(),
        }
    }
}

impl ClaimSource for PersistentEngine {
    type Error = Error;

    fn versions_at_or_before(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        as_of: Millis,
    ) -> Result<Vec<Claim>> {
        match self {
            Self::Native(engine) => engine.versions_at_or_before(subject, predicate, as_of),
            Self::FjallCompatibility(engine) => {
                engine.versions_at_or_before(subject, predicate, as_of)
            }
        }
    }

    fn all_versions(&self, subject: &Subject, predicate: &Predicate) -> Result<Vec<Claim>> {
        match self {
            Self::Native(engine) => engine.all_versions(subject, predicate),
            Self::FjallCompatibility(engine) => engine.all_versions(subject, predicate),
        }
    }

    fn subject_versions(&self, subject: &Subject) -> Result<Vec<Claim>> {
        match self {
            Self::Native(engine) => engine.subject_versions(subject),
            Self::FjallCompatibility(engine) => engine.subject_versions(subject),
        }
    }
}

impl Engine for PersistentEngine {
    fn append_batch(&self, claims: &[Claim]) -> Result<crate::AppendOutcome> {
        match self {
            Self::Native(engine) => Engine::append_batch(engine, claims),
            Self::FjallCompatibility(engine) => Engine::append_batch(engine, claims),
        }
    }

    fn sequence(&self) -> Result<u64> {
        match self {
            Self::Native(engine) => Engine::sequence(engine),
            Self::FjallCompatibility(engine) => Engine::sequence(engine),
        }
    }

    fn claims_in_range(&self, from: u64, to: u64) -> Result<Vec<Claim>> {
        match self {
            Self::Native(engine) => Engine::claims_in_range(engine, from, to),
            Self::FjallCompatibility(engine) => Engine::claims_in_range(engine, from, to),
        }
    }

    fn subjects(&self) -> Result<Vec<Subject>> {
        match self {
            Self::Native(engine) => Engine::subjects(engine),
            Self::FjallCompatibility(engine) => Engine::subjects(engine),
        }
    }

    fn observe(
        &self,
        reader: &Reader,
        subject: &Subject,
        predicate: &Predicate,
        at: Millis,
    ) -> Result<()> {
        match self {
            Self::Native(engine) => Engine::observe(engine, reader, subject, predicate, at),
            Self::FjallCompatibility(engine) => {
                Engine::observe(engine, reader, subject, predicate, at)
            }
        }
    }

    fn get_projection(&self, name: &str) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Native(engine) => Engine::get_projection(engine, name),
            Self::FjallCompatibility(engine) => Engine::get_projection(engine, name),
        }
    }

    fn put_projection_with(&self, name: &str, bytes: &[u8], durability: Durability) -> Result<()> {
        match self {
            Self::Native(engine) => Engine::put_projection_with(engine, name, bytes, durability),
            Self::FjallCompatibility(engine) => {
                Engine::put_projection_with(engine, name, bytes, durability)
            }
        }
    }

    fn runtime_cursor(&self) -> Result<u64> {
        match self {
            Self::Native(engine) => Engine::runtime_cursor(engine),
            Self::FjallCompatibility(engine) => Engine::runtime_cursor(engine),
        }
    }

    fn runtime_schema(&self, scope: &ScopeId) -> Result<Option<RuntimeSchemaRegistry>> {
        match self {
            Self::Native(engine) => Engine::runtime_schema(engine, scope),
            Self::FjallCompatibility(engine) => Engine::runtime_schema(engine, scope),
        }
    }

    fn runtime_read_stamp(&self, scope: &ScopeId) -> Result<ReadStamp> {
        match self {
            Self::Native(engine) => Engine::runtime_read_stamp(engine, scope),
            Self::FjallCompatibility(engine) => Engine::runtime_read_stamp(engine, scope),
        }
    }

    fn open_runtime_snapshot(
        &self,
        scope: &ScopeId,
        owner: &str,
        now: Millis,
        ttl: Millis,
    ) -> Result<SnapshotHandle> {
        match self {
            Self::Native(engine) => Engine::open_runtime_snapshot(engine, scope, owner, now, ttl),
            Self::FjallCompatibility(engine) => {
                Engine::open_runtime_snapshot(engine, scope, owner, now, ttl)
            }
        }
    }

    fn runtime_snapshot_changes(
        &self,
        snapshot: &SnapshotHandle,
        after: u64,
        limit: usize,
        now: Millis,
    ) -> Result<RuntimeChangePage> {
        match self {
            Self::Native(engine) => {
                Engine::runtime_snapshot_changes(engine, snapshot, after, limit, now)
            }
            Self::FjallCompatibility(engine) => {
                Engine::runtime_snapshot_changes(engine, snapshot, after, limit, now)
            }
        }
    }

    fn release_runtime_snapshot(&self, id: &SnapshotId) -> Result<bool> {
        match self {
            Self::Native(engine) => Engine::release_runtime_snapshot(engine, id),
            Self::FjallCompatibility(engine) => Engine::release_runtime_snapshot(engine, id),
        }
    }

    fn runtime_snapshots(&self, now: Millis) -> Result<Vec<SnapshotHandle>> {
        match self {
            Self::Native(engine) => Engine::runtime_snapshots(engine, now),
            Self::FjallCompatibility(engine) => Engine::runtime_snapshots(engine, now),
        }
    }

    fn runtime_retention_pins(&self, now: Millis) -> Result<Vec<RetentionPin>> {
        match self {
            Self::Native(engine) => Engine::runtime_retention_pins(engine, now),
            Self::FjallCompatibility(engine) => Engine::runtime_retention_pins(engine, now),
        }
    }

    fn runtime_read_changes(
        &self,
        read: &ReadStamp,
        after: u64,
        limit: usize,
    ) -> Result<RuntimeChangePage> {
        match self {
            Self::Native(engine) => Engine::runtime_read_changes(engine, read, after, limit),
            Self::FjallCompatibility(engine) => {
                Engine::runtime_read_changes(engine, read, after, limit)
            }
        }
    }

    fn commit_runtime(&self, commit: &RuntimeCommit) -> Result<RuntimeCommitOutcome> {
        match self {
            Self::Native(engine) => Engine::commit_runtime(engine, commit),
            Self::FjallCompatibility(engine) => Engine::commit_runtime(engine, commit),
        }
    }

    fn runtime_changes_since(
        &self,
        after: u64,
        limit: usize,
        scope: Option<&ScopeId>,
    ) -> Result<RuntimeChangePage> {
        match self {
            Self::Native(engine) => Engine::runtime_changes_since(engine, after, limit, scope),
            Self::FjallCompatibility(engine) => {
                Engine::runtime_changes_since(engine, after, limit, scope)
            }
        }
    }

    fn runtime_outbox_since(&self, after: u64, limit: usize) -> Result<Vec<ProjectionWork>> {
        match self {
            Self::Native(engine) => Engine::runtime_outbox_since(engine, after, limit),
            Self::FjallCompatibility(engine) => Engine::runtime_outbox_since(engine, after, limit),
        }
    }

    fn runtime_audit(&self, commit_id: &str) -> Result<Option<AuditEnvelope>> {
        match self {
            Self::Native(engine) => Engine::runtime_audit(engine, commit_id),
            Self::FjallCompatibility(engine) => Engine::runtime_audit(engine, commit_id),
        }
    }

    fn runtime_commit_outcome(&self, commit_id: &str) -> Result<Option<RuntimeCommitOutcome>> {
        match self {
            Self::Native(engine) => Engine::runtime_commit_outcome(engine, commit_id),
            Self::FjallCompatibility(engine) => {
                Engine::runtime_commit_outcome(engine, commit_id)
            }
        }
    }

    fn commit_data_transaction(
        &self,
        transaction: &DataTransaction,
    ) -> Result<RuntimeCommitOutcome> {
        match self {
            Self::Native(engine) => Engine::commit_data_transaction(engine, transaction),
            Self::FjallCompatibility(engine) => {
                Engine::commit_data_transaction(engine, transaction)
            }
        }
    }

    fn preview_data_transaction(
        &self,
        transaction: &DataTransaction,
        valid_at: Millis,
    ) -> Result<DataTransactionView> {
        match self {
            Self::Native(engine) => Engine::preview_data_transaction(engine, transaction, valid_at),
            Self::FjallCompatibility(engine) => {
                Engine::preview_data_transaction(engine, transaction, valid_at)
            }
        }
    }
}
