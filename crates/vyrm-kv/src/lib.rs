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
mod wal;

pub use batch::{Mutation, WriteBatch, BATCH_FORMAT_VERSION};
pub use database::{
    CompactionBoundary, CompactionOutcome, Database, FailureMode, FlushBoundary,
    GarbageCollectionReport, Snapshot,
};
pub use error::{Error, Result};
pub use manifest::{
    Checkpoint, CurrentPointer, Manifest, ManifestStore, SegmentDescriptor, MANIFEST_FORMAT_VERSION,
};
pub use memtable::{Memtable, VersionedValue};
pub use segment::{Segment, SEGMENT_FORMAT_VERSION};
pub use wal::{
    recover, recover_from, repair_torn_tail, AppendReceipt, Durability, RecoveredBatch, Recovery,
    WalBatch, WalWriter, WAL_FORMAT_VERSION, WAL_MAX_PAYLOAD_BYTES,
};
