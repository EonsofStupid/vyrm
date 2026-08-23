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
/// Sequence index: append sequence to canonical claim key. Current native and
/// Fjall writes use the compact key reference; native still reads the legacy
/// inline `VYRNSI01` claim envelope during format transition.
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
/// Authoritative append-only runtime change log, keyed by global cursor.
pub const RUNTIME_CHANGES: &str = "runtime_changes";
/// Latest persisted version of each typed runtime record. Updated atomically
/// with the authoritative change log and used for reference-integrity checks.
pub const RUNTIME_RECORDS: &str = "runtime_records";
/// Latest persisted version of each typed runtime relation.
pub const RUNTIME_RELATIONS: &str = "runtime_relations";
/// Latest canonical vector values. Vector indexes remain projections.
pub const RUNTIME_VECTORS: &str = "runtime_vectors";
/// Latest canonical time-series samples. Time indexes remain projections.
pub const RUNTIME_SERIES: &str = "runtime_series";
/// Latest canonical WGS84 values. Spatial indexes remain projections.
pub const RUNTIME_GEO: &str = "runtime_geo";
/// Canonical visibility records for verified immutable object bytes.
pub const RUNTIME_OBJECTS: &str = "runtime_objects";
/// Transactional projection work keyed by the source runtime cursor.
pub const RUNTIME_OUTBOX: &str = "runtime_outbox";
/// Accepted-operation audit envelopes keyed by commit identity.
pub const RUNTIME_AUDIT: &str = "runtime_audit";
/// Accepted transaction outcomes keyed by content identity for idempotent retry.
pub const RUNTIME_COMMITS: &str = "runtime_commits";
/// Latest authoritative schema registry for each runtime scope. Every update
/// is also present in the hash-chained runtime change log.
pub const RUNTIME_SCHEMAS: &str = "runtime_schemas";
/// Persisted leased read stamps. Native `vyrmKV` uses the same catalog to pin
/// physical manifests; the append-only compatibility engine needs no further
/// retention machinery yet.
pub const RUNTIME_SNAPSHOTS: &str = "runtime_snapshots";

/// Every logical keyspace owned by the persistent Engine contract, in the
/// canonical order used by storage migration archives. Adding a keyspace is a
/// format decision: migrations deny unknown source keyspaces and therefore
/// cannot silently omit newly introduced state.
pub const ALL: [&str; 18] = [
    CLAIMS,
    SEQUENCE_INDEX,
    ACCESS,
    META,
    INVOCATIONS,
    PROJECTIONS,
    RUNTIME_CHANGES,
    RUNTIME_RECORDS,
    RUNTIME_RELATIONS,
    RUNTIME_VECTORS,
    RUNTIME_SERIES,
    RUNTIME_GEO,
    RUNTIME_OBJECTS,
    RUNTIME_OUTBOX,
    RUNTIME_AUDIT,
    RUNTIME_COMMITS,
    RUNTIME_SCHEMAS,
    RUNTIME_SNAPSHOTS,
];

/// Manifest-authenticated native logical-key encoding. Format 1 stores the
/// UTF-8 keyspace name followed by NUL. Format 2 stores one stable non-zero tag;
/// the archive remains logical and therefore independent of either encoding.
pub(crate) const NATIVE_KEYSPACE_TAG_FORMAT_V2: u64 = 0x5659_5253_4b30_3032; // `VYRSK002`

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeKeyCodec {
    TextV1,
    TagV2,
}

impl NativeKeyCodec {
    pub(crate) fn from_application_format(format: Option<u64>) -> Option<Self> {
        match format {
            None => Some(Self::TextV1),
            Some(NATIVE_KEYSPACE_TAG_FORMAT_V2) => Some(Self::TagV2),
            Some(_) => None,
        }
    }

    pub(crate) fn encode(self, space: &str, key: &[u8]) -> Option<Vec<u8>> {
        match self {
            Self::TextV1 => {
                let mut stored = Vec::with_capacity(space.len() + 1 + key.len());
                stored.extend_from_slice(space.as_bytes());
                stored.push(0);
                stored.extend_from_slice(key);
                Some(stored)
            }
            Self::TagV2 => {
                let mut stored = Vec::with_capacity(1 + key.len());
                stored.push(native_keyspace_tag(space)?);
                stored.extend_from_slice(key);
                Some(stored)
            }
        }
    }

    pub(crate) fn strip<'a>(self, space: &str, stored: &'a [u8]) -> Option<&'a [u8]> {
        match self {
            Self::TextV1 => {
                let prefix = self.encode(space, &[])?;
                stored.strip_prefix(prefix.as_slice())
            }
            Self::TagV2 => stored.strip_prefix(&[native_keyspace_tag(space)?]),
        }
    }
}

pub(crate) fn native_keyspace_tag(space: &str) -> Option<u8> {
    match space {
        CLAIMS => Some(1),
        SEQUENCE_INDEX => Some(2),
        ACCESS => Some(3),
        META => Some(4),
        INVOCATIONS => Some(5),
        PROJECTIONS => Some(6),
        RUNTIME_CHANGES => Some(7),
        RUNTIME_RECORDS => Some(8),
        RUNTIME_RELATIONS => Some(9),
        RUNTIME_VECTORS => Some(10),
        RUNTIME_SERIES => Some(11),
        RUNTIME_GEO => Some(12),
        RUNTIME_OBJECTS => Some(13),
        RUNTIME_OUTBOX => Some(14),
        RUNTIME_AUDIT => Some(15),
        RUNTIME_COMMITS => Some(16),
        RUNTIME_SCHEMAS => Some(17),
        RUNTIME_SNAPSHOTS => Some(18),
        _ => None,
    }
}

pub(crate) fn native_keyspace_for_tag(tag: u8) -> Option<&'static str> {
    tag.checked_sub(1)
        .and_then(|index| ALL.get(usize::from(index)))
        .copied()
}

#[cfg(test)]
mod native_key_codec_tests {
    use super::*;

    #[test]
    fn native_keyspace_tags_are_frozen_unique_and_canonical() {
        let tags = ALL
            .iter()
            .map(|space| native_keyspace_tag(space).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(tags, (1_u8..=18).collect::<Vec<_>>());
        for (space, tag) in ALL.iter().zip(tags) {
            assert_eq!(native_keyspace_for_tag(tag), Some(*space));
        }
        assert_eq!(
            NativeKeyCodec::from_application_format(None),
            Some(NativeKeyCodec::TextV1)
        );
        assert_eq!(
            NativeKeyCodec::from_application_format(Some(NATIVE_KEYSPACE_TAG_FORMAT_V2)),
            Some(NativeKeyCodec::TagV2)
        );
        assert_eq!(NativeKeyCodec::from_application_format(Some(7)), None);
        assert_eq!(native_keyspace_tag("unknown"), None);
        assert_eq!(native_keyspace_for_tag(0), None);
        assert_eq!(native_keyspace_for_tag(19), None);
    }

    #[test]
    fn native_key_codecs_round_trip_frozen_wire_bytes() {
        let logical = b"subject/predicate";
        let legacy = NativeKeyCodec::TextV1.encode(CLAIMS, logical).unwrap();
        assert_eq!(legacy, b"claims\0subject/predicate");
        assert_eq!(
            NativeKeyCodec::TextV1.strip(CLAIMS, &legacy),
            Some(logical.as_slice())
        );

        let compact = NativeKeyCodec::TagV2.encode(CLAIMS, logical).unwrap();
        assert_eq!(compact, b"\x01subject/predicate");
        assert_eq!(
            NativeKeyCodec::TagV2.strip(CLAIMS, &compact),
            Some(logical.as_slice())
        );

        assert_eq!(NativeKeyCodec::TagV2.encode("unknown", logical), None);
        assert_eq!(NativeKeyCodec::TagV2.strip(SEQUENCE_INDEX, &compact), None);
    }
}

/// Key under which the claim sequence watermark is recorded.
pub const SEQUENCE_WATERMARK: &[u8] = b"watermark/claims/sequence";

pub(crate) fn accepted_append_key(idempotency_key: &str) -> Vec<u8> {
    let mut key = b"accepted/claim-append/".to_vec();
    key.extend_from_slice(idempotency_key.as_bytes());
    key
}

/// Key under which the invocation ordinal watermark is recorded.
pub const INVOCATION_WATERMARK: &[u8] = b"watermark/invocations/ordinal";

/// Global cursor and hash-chain head for typed runtime changes.
pub const RUNTIME_CURSOR: &[u8] = b"watermark/runtime/cursor";
pub const RUNTIME_LAST_DIGEST: &[u8] = b"watermark/runtime/last-digest";
pub const RUNTIME_LAST_AUDIT_DIGEST: &[u8] = b"watermark/runtime/last-audit-digest";
/// Versioned RFC 9162 compact frontier for the global runtime log. Complete
/// subtree nodes share this META keyspace so migrations and snapshots cannot
/// accidentally omit proof state.
pub const RUNTIME_ACCUMULATOR_STATE: &[u8] = b"runtime/merkle/v1/state";
const RUNTIME_ACCUMULATOR_NODE_PREFIX: &[u8] = b"runtime/merkle/v1/node/";

pub fn runtime_accumulator_node_key(level: u8, index: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(RUNTIME_ACCUMULATOR_NODE_PREFIX.len() + 9);
    key.extend_from_slice(RUNTIME_ACCUMULATOR_NODE_PREFIX);
    key.push(level);
    key.extend_from_slice(&index.to_be_bytes());
    key
}

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
