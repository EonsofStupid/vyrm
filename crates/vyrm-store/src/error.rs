//! Substrate adapter errors.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// Propagated from the substrate.
    Substrate(String),
    /// Claim encoding or decoding failed.
    Codec(String),
    /// Propagated from the kernel.
    Kernel(vyrm_core::Error),
    /// Sequence allocation overflowed. Reported rather than saturated, so that
    /// overflow cannot degrade into silent key reuse. `SPEC.md` §11 correction 2.
    SequenceOverflow,
    /// A recorded watermark was not a valid sequence value.
    CorruptWatermark(String),
    /// A projection was halted by grounding (`SPEC.md` §8.3) and refuses reads
    /// and rebuilds until an operator resets it.
    Quarantined(String),
    /// Optimistic concurrency rejected a writer that observed an older head.
    RuntimeConflict { expected: u64, actual: u64 },
    /// A relation or event named a record that is not present in its scope.
    DanglingRuntimeReference(String),
    /// A typed write was attempted before its scope installed a registry.
    RuntimeSchemaMissing(String),
    /// A schema update skipped or repeated a persisted revision.
    RuntimeSchemaConflict { expected: u64, actual: u64 },
    /// A leased snapshot is not present in the authoritative snapshot catalog.
    SnapshotNotFound(String),
    /// A leased snapshot was presented after its expiration instant.
    SnapshotExpired { id: String, expired_at: u64 },
    /// A caller supplied fields that do not match the persisted snapshot with
    /// the same identity.
    SnapshotMismatch(String),
    /// A stamped transaction read names a cursor that is not retained.
    ReadStampUnavailable(String),
    /// A stamped transaction read does not match the retained hash/schema state.
    ReadStampMismatch(String),
    /// Object-tier I/O or capability error.
    Object(String),
    /// Explicit storage migration failed closed.
    Migration(String),
    /// A referenced immutable object is absent.
    ObjectMissing(String),
    /// Stored bytes do not match their content address.
    ObjectCorrupt { expected: String, actual: String },
    /// Stored bytes match the digest but not the committed length evidence.
    ObjectLengthMismatch { expected: u64, actual: u64 },
    /// Deterministic failure-injection boundary used by crash tests.
    FaultInjected(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Substrate(m) => write!(f, "substrate: {m}"),
            Error::Codec(m) => write!(f, "claim codec: {m}"),
            Error::Kernel(e) => write!(f, "kernel: {e}"),
            Error::SequenceOverflow => write!(f, "sequence allocation overflowed"),
            Error::CorruptWatermark(m) => write!(f, "corrupt sequence watermark: {m}"),
            Error::Quarantined(m) => write!(f, "quarantined: {m}"),
            Error::RuntimeConflict { expected, actual } => write!(
                f,
                "runtime commit conflict: expected cursor {expected}, actual cursor {actual}"
            ),
            Error::DanglingRuntimeReference(reference) => {
                write!(f, "dangling runtime reference: {reference}")
            }
            Error::RuntimeSchemaMissing(scope) => {
                write!(f, "runtime schema is not installed for scope {scope}")
            }
            Error::RuntimeSchemaConflict { expected, actual } => write!(
                f,
                "runtime schema conflict: expected revision {expected}, actual revision {actual}"
            ),
            Error::SnapshotNotFound(id) => write!(f, "runtime snapshot not found: {id}"),
            Error::SnapshotExpired { id, expired_at } => {
                write!(f, "runtime snapshot {id} expired at {expired_at}")
            }
            Error::SnapshotMismatch(id) => {
                write!(
                    f,
                    "runtime snapshot {id} does not match the persisted lease"
                )
            }
            Error::ReadStampUnavailable(id) => {
                write!(f, "runtime read stamp is not retained: {id}")
            }
            Error::ReadStampMismatch(id) => {
                write!(f, "runtime read stamp does not match retained state: {id}")
            }
            Error::Object(message) => write!(f, "object store: {message}"),
            Error::Migration(message) => write!(f, "storage migration: {message}"),
            Error::ObjectMissing(digest) => write!(f, "object missing: {digest}"),
            Error::ObjectCorrupt { expected, actual } => write!(
                f,
                "object corrupt: expected digest {expected}, actual digest {actual}"
            ),
            Error::ObjectLengthMismatch { expected, actual } => write!(
                f,
                "object length mismatch: expected {expected}, actual {actual}"
            ),
            Error::FaultInjected(point) => write!(f, "failure injected at {point}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<fjall::Error> for Error {
    fn from(value: fjall::Error) -> Self {
        Error::Substrate(value.to_string())
    }
}

impl From<vyrm_kv::Error> for Error {
    fn from(value: vyrm_kv::Error) -> Self {
        Error::Substrate(value.to_string())
    }
}

impl From<vyrm_core::Error> for Error {
    fn from(value: vyrm_core::Error) -> Self {
        Error::Kernel(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::Codec(value.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::Object(value.to_string())
    }
}
