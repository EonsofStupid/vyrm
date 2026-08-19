//! # vyrm-store
//!
//! Storage port and transitional Fjall compatibility adapter. The port is the
//! contract the Vyrm-native substrate will implement and eventually replace.
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

mod engine;
mod error;
mod gc;
mod invocation;
mod keyspaces;
mod native;
mod persistent;
mod projection;
mod store;
mod writer;

pub use engine::{Engine, MemoryEngine};
pub use error::{Error, Result};
pub use gc::{PairStatus, RemovalReport, Verdict};
pub use invocation::{Effectiveness, Invocation, InvocationInput, Outcome, RecallOutcome, Trigger};
pub use keyspaces::Durability;
pub use native::NativeEngine;
pub use persistent::{PersistentBackend, PersistentEngine};
pub use projection::{
    CurrentProjection, GroundedStamp, GroundingReport, ProjectionStatus, RebuildOutcome,
    CURRENT_PROJECTION,
};
pub use store::{AppendOutcome, Store};
pub use vyrm_core::{
    DataTransaction, DataTransactionView, ReadStamp, RetentionPin, RetentionPinId,
    RuntimeChangePage, RuntimeCommitOutcome, SnapshotHandle, SnapshotId,
};
pub use writer::{Writer, WriterConfig, WriterStats};
