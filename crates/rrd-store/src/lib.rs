//! # rrd-store
//!
//! Storage port and transitional Fjall compatibility adapter. The port is the
//! contract the Rrd-native substrate will implement and eventually replace.
//!
//! ## Corrections applied
//!
//! This crate exists partly to carry the corrections recorded in `SPEC.md` §11,
//! derived from audit of `native/fjall-vortex-runtime/src/main.rs`:
//!
//! 1. Sequence allocation occurs inside the write transaction, so it does not
//!    depend on an external lock for correctness.
//! 2. Sequence increment uses `checked_add`.
//! 3. A claim write issues one fsync, not two.
//! 4. Reads take a snapshot and acquire no write lock.
//! 5. [`Store::append_batch`] amortizes both transport and fsync cost.
//!
//! ## Concurrency
//!
//! [`Store`] holds no mutex. Fjall is internally synchronized for multi-threaded
//! access, and `SingleWriterTxDatabase` serializes write transactions itself. The
//! external mutex in the prior runtime protected only correction 1; with that
//! corrected, reads run concurrently.

mod archive;
mod backup;
mod control;
mod ds;
mod engine;
mod error;
mod footprint;
mod gc;
mod invocation;
mod keyspaces;
mod migration;
mod native;
mod object;
mod persistent;
mod projection;
mod s3;
mod store;
mod upgrade;
mod writer;

pub use archive::{
    export_logical_archive, export_logical_archive_with_progress, inspect_logical_archive,
    restore_logical_archive_to_new_root, restore_logical_archive_to_new_root_with,
    restore_logical_archive_to_new_root_with_progress, LogicalArchiveCheckpoint,
    LogicalArchiveInventory, LogicalArchiveOperation, LogicalArchiveProgress, LogicalRestoreReport,
    LOGICAL_ARCHIVE_VERSION,
};
pub use backup::{
    create_application_backup, create_logical_backup, load_backup_catalogue,
    restore_catalogued_backup, verify_backup_catalogue, verify_restored_backup_objects,
    BackupCatalogue, BackupCoverage, BackupEntry, CatalogueManifestInventory,
    ObjectPayloadManifestInventory, BACKUP_CATALOGUE_VERSION,
};
pub use control::{ControlJournalEntry, ControlTransition};
pub use ds::{DataRuntime, DataRuntimeAccess, DataRuntimeRef, DataRuntimeStep};
pub use engine::{Engine, MemoryEngine, PhysicalStoreEvidence};
pub use error::{Error, Result};
pub use footprint::{measure_storage_footprint, FootprintBytes, StorageFootprint};
pub use gc::{PairStatus, RemovalReport, Verdict};
pub use invocation::{Effectiveness, Invocation, InvocationInput, Outcome, RecallOutcome, Trigger};
pub use keyspaces::Durability;
pub use migration::{
    migrate_fjall_to_native, migrate_fjall_to_native_with_fault, migration_status,
    rollback_fjall_migration, MigrationFault, MigrationInventory, MigrationPhase, MigrationReport,
};
pub use native::{
    native_database_artifact_view, native_runtime_commit_context, native_runtime_commit_outcome,
    native_snapshot_all_object_references, native_snapshot_artifact_view,
    native_snapshot_object_references, prepare_native_runtime_commit, NativeEngine,
    NativeRuntimeCommitPlan,
};
pub use object::{
    ImmutableObjectStore, LocalObjectStore, ObjectInventory, ObjectInventoryEntry,
    ObjectInventoryState, ObjectStep, VerifiedObject,
};
pub use persistent::{PersistentBackend, PersistentEngine};
pub use projection::{
    CurrentProjection, GroundedStamp, GroundingReport, ProjectionStatus, RebuildOutcome,
    CURRENT_PROJECTION,
};
pub use rrd_core::{
    DataTransaction, DataTransactionView, ReadStamp, RetentionPin, RetentionPinId,
    RuntimeChangePage, RuntimeCommitOutcome, SnapshotHandle, SnapshotId,
};
pub use rrd_lsm::{
    publish_rename as publish_durable_rename, sync_directory as sync_directory_metadata,
};
pub use s3::{ConditionalPut, S3CompatibleObjectStore, S3ObjectClient, S3ObjectMetadata};
pub use store::{AppendOutcome, IdempotentAppendOutcome, Store};
pub use upgrade::{
    migrate_native_format, migrate_native_format_with_fault, native_format_migration_status,
    FormatMigrationFault, FormatMigrationLedger, FormatMigrationPhase,
};
pub use writer::{Writer, WriterConfig, WriterStats};
