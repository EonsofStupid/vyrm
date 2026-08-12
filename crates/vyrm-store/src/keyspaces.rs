//! Keyspace names and durability classes.
//!
//! A keyspace here is a Fjall 3.x keyspace: one LSM tree within one database,
//! named a *partition* before Fjall 3.0.
//!
//! Durability accounts for 0.431 ms of a 0.562 ms claim write (`SPEC.md` §2), so
//! telemetry must not incur it. `SPEC.md` §7.1 assigns each keyspace a class.

use fjall::PersistMode;

/// Authoritative claims.
pub const CLAIMS: &str = "claims";
/// Sequence index: append sequence to claim key.
///
/// Replaces the `events` keyspace carried over from the prior runtime, which was
/// allocated for a term the specification never defined and which nothing wrote
/// to. See `PLAN.md` F1.
pub const SEQUENCE_INDEX: &str = "sequence_index";
/// Read telemetry. Loss on crash is acceptable.
pub const ACCESS: &str = "access";
/// Watermarks and idempotency keys.
pub const META: &str = "meta";
/// Recorded operator invocations (`SPEC.md` §13).
pub const INVOCATIONS: &str = "invocations";
/// Derived projections (the routing index among them). Derivable from their
/// sources by construction, so loss on crash costs a rebuild, not truth.
pub const PROJECTIONS: &str = "projections";

/// Key under which the claim sequence watermark is recorded.
pub const SEQUENCE_WATERMARK: &[u8] = b"watermark/claims/sequence";

/// Key under which the invocation ordinal watermark is recorded.
pub const INVOCATION_WATERMARK: &[u8] = b"watermark/invocations/ordinal";

/// Persistence policy for a write transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// Authoritative writes. Journal is flushed and synced before commit returns.
    Authoritative,
    /// Telemetry writes. Buffered; a periodic flush or a later authoritative
    /// write carries them to disk.
    Buffered,
}

impl Durability {
    /// Persist mode supplied to the write transaction.
    ///
    /// `Authoritative` returns `Some(SyncAll)`, which is the sole fsync for the
    /// transaction. `SPEC.md` §11 correction 3: the commit carries durability, so
    /// no separate persist call follows it.
    pub fn persist_mode(self) -> Option<PersistMode> {
        match self {
            Durability::Authoritative => Some(PersistMode::SyncAll),
            Durability::Buffered => Some(PersistMode::Buffer),
        }
    }
}
