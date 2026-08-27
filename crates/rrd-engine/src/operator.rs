//! Embedded/offline operator boundary used by the RRFlow CLI.
//!
//! The handle owns the physical storage selection. Outward adapters receive a
//! deliberately bounded engine surface and never construct an `rrd-store`
//! backend or call storage migration/archive functions directly.

use rrd_store::Engine as _;
use std::path::Path;

pub use rrd_core::{
    digest, Claim, ClaimReader, Millis, Predicate, Producer, Reader, ReasoningPayload, RecallQuery,
    RecallSet, ScopeId, Subject,
};
pub use rrd_store::{
    BackupCatalogue, BackupEntry, Effectiveness, FormatMigrationLedger, GroundingReport,
    Invocation, InvocationInput, LogicalArchiveInventory, LogicalRestoreReport, MigrationReport,
    Outcome, RecallOutcome, RemovalReport, Trigger,
};

pub type CoreResult<T> = rrd_core::Result<T>;
pub type Result<T> = rrd_store::Result<T>;

/// One embedded RRD authority for ordinary operator commands. The concrete
/// backend remains private to the engine composition crate.
pub struct EmbeddedOperator {
    storage: rrd_store::PersistentEngine,
}

impl EmbeddedOperator {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            storage: rrd_store::PersistentEngine::open(path)?,
        })
    }

    /// Supplies the engine's implementation of its internal runtime port to
    /// engine-owned algorithms. The opaque return prevents adapters from
    /// naming or constructing the physical backend.
    pub fn runtime_store(&self) -> &impl rrd_store::Engine {
        &self.storage
    }

    pub fn path(&self) -> &Path {
        self.storage.path()
    }

    pub fn backend_name(&self) -> &'static str {
        self.storage.backend().as_str()
    }

    pub fn record_invocation(&self, input: InvocationInput<'_>) -> Result<Invocation> {
        self.storage.record_invocation(input)
    }

    pub fn set_recall_outcome(&self, ordinal: u64, outcome: RecallOutcome) -> Result<Invocation> {
        self.storage.set_recall_outcome(ordinal, outcome)
    }

    pub fn invocations_since(&self, since: Millis) -> Result<Vec<Invocation>> {
        self.storage.invocations_since(since)
    }

    pub fn invocation_count(&self) -> Result<u64> {
        self.storage.invocation_count()
    }

    pub fn access_count(&self) -> Result<usize> {
        self.storage.access_count()
    }

    pub fn sequence(&self) -> Result<u64> {
        self.storage.sequence()
    }

    pub fn observe(
        &self,
        reader: &Reader,
        subject: &Subject,
        predicate: &Predicate,
        at: Millis,
    ) -> Result<()> {
        self.storage.observe(reader, subject, predicate, at)
    }

    pub fn recall(&self, query: &RecallQuery, token_budget: usize) -> Result<RecallSet> {
        rrd_core::recall(&self.storage, query, token_budget)
    }

    pub fn assert(&self, claim: &Claim) -> Result<rrd_store::AppendOutcome> {
        self.storage.assert(claim)
    }

    pub fn as_of(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        at: Millis,
    ) -> Result<Option<Claim>> {
        self.storage.as_of(subject, predicate, at)
    }

    pub fn history(&self, subject: &Subject, predicate: &Predicate) -> Result<Vec<Claim>> {
        self.storage.history(subject, predicate)
    }

    pub fn rebuild_current(&self) -> Result<rrd_store::RebuildOutcome> {
        self.storage.rebuild_current()
    }

    pub fn ground_current(&self, at: Millis) -> Result<GroundingReport> {
        self.storage.ground_current(at)
    }

    pub fn reset_current(&self) -> Result<rrd_store::RebuildOutcome> {
        self.storage.reset_current()
    }

    pub fn removal_report(&self, since: Millis, evaluated_at: Millis) -> Result<RemovalReport> {
        self.storage.removal_report(since, evaluated_at)
    }

    pub fn export_logical_archive(&self, archive: &Path) -> Result<LogicalArchiveInventory> {
        rrd_store::export_logical_archive(&self.storage, archive)
    }

    pub fn create_logical_backup(
        &self,
        catalogue: &Path,
        label: &str,
        now: Millis,
    ) -> Result<BackupEntry> {
        rrd_store::create_logical_backup(&self.storage, catalogue, label, now)
    }
}

pub fn migrate_storage(db: &Path, now: Millis) -> Result<MigrationReport> {
    rrd_store::migrate_fjall_to_native(db, now)
}

pub fn storage_migration_status(db: &Path) -> Result<Option<MigrationReport>> {
    rrd_store::migration_status(db)
}

pub fn rollback_storage_migration(db: &Path) -> Result<MigrationReport> {
    rrd_store::rollback_fjall_migration(db)
}

pub fn inspect_logical_archive(archive: &Path) -> Result<LogicalArchiveInventory> {
    rrd_store::inspect_logical_archive(archive)
}

pub fn restore_logical_archive(
    archive: &Path,
    db: &Path,
    now: Millis,
) -> Result<LogicalRestoreReport> {
    rrd_store::restore_logical_archive_to_new_root(archive, db, now)
}

pub fn verify_backup_catalogue(catalogue: &Path) -> Result<BackupCatalogue> {
    rrd_store::verify_backup_catalogue(catalogue)
}

pub fn restore_catalogued_backup(
    catalogue: &Path,
    backup_id: &str,
    db: &Path,
    now: Millis,
) -> Result<LogicalRestoreReport> {
    rrd_store::restore_catalogued_backup(catalogue, backup_id, db, now)
}

pub fn migrate_native_format(db: &Path, now: Millis) -> Result<FormatMigrationLedger> {
    rrd_store::migrate_native_format(db, now)
}

pub fn native_format_migration_status(db: &Path) -> Result<Option<FormatMigrationLedger>> {
    rrd_store::native_format_migration_status(db)
}
