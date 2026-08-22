//! Native storage substrate for Vyrm.
//!
//! This crate begins at the durable boundary. One accepted atomic batch is one
//! checksummed WAL frame. Recovery accepts only a contiguous valid prefix;
//! incomplete tail bytes are reported for explicit repair, while corruption in
//! a complete frame fails closed.

mod batch;
mod database;
mod error;
mod manifest;
mod memtable;
mod segment;
mod snapshot_bundle;
mod wal;

pub use batch::{Mutation, WriteBatch, BATCH_FORMAT_VERSION};
pub use database::{
    CompactionBoundary, CompactionOutcome, CompactionPolicy, Database, DatabaseOptions,
    FailureMode, FlushBoundary, GarbageCollectionReport, MaintenancePolicy, MaintenanceStats,
    Snapshot, SnapshotInstallBoundary, DEFAULT_COMPACTION_MAX_INPUT_SEGMENTS,
    DEFAULT_COMPACTION_TARGET_SEGMENT_BYTES, DEFAULT_L0_COMPACTION_TRIGGER,
    DEFAULT_MAX_COMPACTION_LEVEL, DEFAULT_MEMTABLE_MAX_VERSIONS, DEFAULT_WAL_PAYLOAD_MAX_BYTES,
};
pub use error::{Error, Result};
pub use manifest::{
    Checkpoint, CurrentPointer, Manifest, ManifestStore, SegmentDescriptor, MANIFEST_FORMAT_VERSION,
};
pub use memtable::{Memtable, VersionedValue};
pub use segment::{
    BlockCacheStats, Segment, DEFAULT_BLOCK_CACHE_BYTES, SEGMENT_BLOCK_TARGET_BYTES,
    SEGMENT_FORMAT_VERSION,
};
pub use snapshot_bundle::{
    SnapshotBundle, SnapshotBundleFile, SnapshotExportBoundary, SnapshotSegment,
    SNAPSHOT_BUNDLE_FORMAT_VERSION, SNAPSHOT_BUNDLE_MAX_BYTES,
};
pub use wal::{
    recover, recover_from, repair_torn_tail, AppendReceipt, Durability, RecoveredBatch, Recovery,
    WalBatch, WalWriter, WAL_FORMAT_VERSION, WAL_MAX_PAYLOAD_BYTES,
};
