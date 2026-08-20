# vyrmKV native format contract

Status: M3 local promotion baseline passes. WAL, atomic-batch, manifest,
checkpoint, and physical snapshot-bundle formats are version 1; new immutable
segments are version 3 and the reader retains explicit version-1/version-2
compatibility.
The format is pre-release. Any format change before alpha must increment its
explicit version and update the checked-in vectors; readers never guess.

Runtime entry points use `PersistentEngine`: a missing path creates this native
format, and an authenticated `CURRENT` pointer selects it on reopen. An existing
directory without that marker remains on the Fjall compatibility adapter. The
selector never probes partial native internals or rewrites an existing store.

## Durability boundary

One accepted atomic batch is one WAL frame. `Authoritative` acknowledgment is
returned only after `sync_data`; `Buffered` acknowledgment states that the frame
was written but is not yet claimed durable. A failed write or sync poisons that
writer instance. The caller must recover and reopen rather than append behind an
unknown partial write.

The WAL admits only contiguous, non-zero sequence ranges. Recovery replays the
longest valid prefix in order. Sequence reuse, gaps, and overflow fail before a
write begins.

## WAL v1

All integers are unsigned big-endian.

File header (16 bytes):

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 8 | ASCII `VYRWAL01` |
| 8 | 2 | format version (`1`) |
| 10 | 2 | file-header length (`16`) |
| 12 | 4 | CRC32C over bytes `0..12` |

Batch frame header (32 bytes):

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | ASCII `VYR1` |
| 4 | 2 | format version (`1`) |
| 6 | 1 | record kind (`1`, atomic batch) |
| 7 | 1 | flags (`0`; unknown flags fail closed) |
| 8 | 4 | payload length, capped at 16 MiB |
| 12 | 8 | first MVCC sequence |
| 20 | 8 | last MVCC sequence |
| 28 | 4 | CRC32C over header bytes `4..28` and the payload |

The outer WAL treats the payload as bytes so recovery does not need higher-level
schema code.

## Atomic mutation batch v1

The payload begins with `VYRBAT01`, a `u16` version, zero `u16` flags, and a
`u32` operation count. Each operation contains a one-byte kind, three zero flag
bytes, `u32` key/value lengths, then key and value bytes. Put is kind 1; delete
is kind 2 and must carry a zero value length. Empty batches/keys, unknown flags
or kinds, trailing bytes, and lengths outside the declared limits fail closed.
One MVCC sequence is allocated per operation while the whole batch remains one
atomic WAL frame.

After a memtable flush, the successor WAL starts at the manifest's declared
`wal_start_sequence`; recovery takes that boundary explicitly, so an empty
rotated WAL still has an unambiguous next sequence and replay under the wrong
manifest fails.

## Recovery classification

- An incomplete file header is corruption: no valid WAL identity exists.
- A partial final frame header or payload is a torn tail. Recovery returns its
  exact start offset and never mutates the file.
- `repair_torn_tail` is the only truncation path. It truncates to the reported
  valid-prefix boundary and syncs the file.
- Bad magic, version, kind, flags, length, sequence, or checksum in a complete
  frame is corruption. It is never silently reclassified as a torn write.
- Replaying unchanged bytes is idempotent and returns the same batch list and
  valid boundary.

## Manifest v1

A manifest is immutable, has a monotonic generation, names its parent digest
after generation 1, declares the durable/WAL sequence boundary, and lists every
reachable immutable segment. Segment order is canonicalized by level, first
key, and content identity before hashing. A manifest's SHA-256 digest excludes
only its own `digest` field.

Every segment descriptor carries its content identity/checksum, key range,
sequence range, entry count, and byte count. Duplicate identities, inverted
ranges, empty segments, and segments newer than the manifest's durable sequence
fail closed.

Immutable segment v3 stores a fixed 64-byte `VYRSEG03` header, independently
compressed LZ4 record blocks, a bounded `VYRIX003` footer index, and a lowercase
ASCII SHA-256 footer over every preceding physical byte. The header declares
the entry/sequence range, total uncompressed record bytes, index offset, block
count, and the canonical 4 KiB query target. A record is never split: the one-record
oversize exception is bounded by the 1 MiB key plus 8 MiB value contract.

Each index entry declares physical offset/length, decoded length, entry count,
last key, and SHA-256 of the compressed bytes. Offsets must exactly cover the
data region, last keys are non-decreasing so one key may span blocks, and the
index is capped at 64 MiB. Open streams the outer digest with a 64 KiB buffer,
then decodes and validates one block at a time. Runtime reads recheck the block
digest with optimized SHA-256 over the raw 32-byte expected digest before LZ4
decode, so post-open file mutation fails closed without allocating hex strings. Unknown
flags, length/count disagreement, invalid ordering, corrupt compression, gaps,
overlap, and trailing bytes are denied.

Point reads return owned values and load only candidate blocks. A single
immutable segment reduces ordered MVCC groups directly into range results;
multi-segment reads retain the general version merge. Snapshot and compaction
traversal process blocks sequentially. All immutable
segments in one `Database` share a decoded-block LRU: 4 MiB by default,
configurable at create/open, with capacity/resident/entry/hit/miss/eviction
counters exposed by `block_cache_stats`. A block larger than the configured
cache is decoded for its caller but never retained. Version-1 `VYRSEG01`
uncompressed and version-2 `VYRSEG02` single-block files remain readable through
explicit legacy branches; writers emit only version 3.
Put/tombstone records retain every version needed for an older snapshot. Files
are named by their physical-content digest, written to a unique temporary,
synced, atomically renamed, and followed by a directory sync. Reusing an
existing content identity first revalidates the complete segment.

Named checkpoints are separately checksummed, atomically published files that
pin an immutable manifest generation. Names use a path-safe canonical grammar;
repeating identical bytes is idempotent, rebinding a name fails closed, and
release is explicit and directory-synced. Retention and GC consume this
inventory rather than inferring reachability from filenames.

## Physical snapshot bundle v1

`SnapshotBundle` is the transferable physical closure of one flush-bounded
manifest. Export first completes the normal WAL → segment → successor-WAL →
manifest publication sequence. Consequently the captured manifest requires
`wal_start_sequence == durable_sequence + 1`: its successor WAL is empty, and
all state needed at the snapshot boundary is carried by immutable segments.

The binary envelope uses unsigned big-endian lengths:

| Field | Bytes | Meaning |
|---|---:|---|
| magic | 8 | ASCII `VYRSNP01` |
| version | 2 | snapshot-bundle format `1` |
| flags | 2 | zero; unknown flags fail closed |
| manifest length | 4 | canonical JSON manifest bytes |
| segment count | 4 | number of following segment records |
| manifest | variable | authenticated source manifest |
| each segment | `4 + 8 + n + m` | descriptor length, byte length, descriptor JSON, exact `.seg` bytes |
| bundle digest | 64 | lowercase ASCII SHA-256 over every preceding byte |

The envelope is capped at 1 GiB and one million segments. Validation checks the
outer digest, manifest digest and invariants, exact descriptor order, and every
segment's authenticated physical bytes before installation can publish
anything.

Installation does not adopt the source manifest's history. It materializes
content-addressed segment files, creates and syncs the empty continuation WAL,
then creates a new local manifest whose parent is the target's prior `CURRENT`.
One pointer publication makes the imported state visible. A bundle must advance
the local physical sequence; reinstalling the already-current segment closure
is idempotent, while stale bundles fail closed. Writes continue at exactly
`source durable_sequence + 1`.

Deterministic crash and storage-full injection covers synchronized segments,
the successor WAL, and manifest publication. Failures before publication reopen
the old state and can retry over authenticated orphan files. A failure after
publication reopens the imported state. Corruption, truncation, stale install,
round-trip, reopen, idempotency, target-state replacement, and post-install
continuation are executable tests in `tests/snapshot_bundle.rs`.

`SnapshotBundleFile` preserves these exact v1 bytes while exporting through a
64 KiB copy/hash buffer and validating one segment at a time. File creation is
`create_new`, synchronized before use, and removes partial output on ordinary
failure. Deterministic crash/storage-full injection covers header-written,
segment-written, and file-synced boundaries. The Linux memory regression uses
a bundle larger than 16 MiB and caps incremental export RSS at 16 MiB. A second
Linux process regression opens and reads 20 MiB of immutable segments with a
4 MiB shared cache, requires eviction, and caps RSS growth at 16 MiB.

OpenRaft adapter v4 consumes this exact contract for canonical-state transfer.
It inspects the authenticated state/domain records before installation, then
publishes the imported closure through the same local manifest CAS. Vote, log,
commit, purge, and snapshot-cache records live in a separate node-local VyrmKV
domain and therefore cannot appear in the bundle.

The native `Engine` adapter passes Memory/Fjall/native semantic and exact query
differentials, including flush/reopen. Compaction retains the newest version
visible at every explicitly protected physical sequence plus the durable head;
an obsolete tombstone with no retained older value disappears. Runtime leases
create physical manifest checkpoints and reconcile them on reopen,
compaction, release, and expiry. GC validates the complete root inventory,
then removes only manifests, segments, and WALs unreachable from `CURRENT` or
a named checkpoint.

Deterministic crash and storage-full injection covers the WAL-sync,
segment-sync, successor-WAL-sync, and manifest-publication flush boundaries,
plus compaction segment and manifest publication. Every cell reopens, verifies
the accepted data, continues writing, and reopens again. The comparative
benchmark is recorded in `vyrmkv-benchmark.md`. The current five-trial isolated
local baseline passes its strict equal-or-better gate in every measured cell.
Fjall remains live as a compatibility oracle until the result is reproduced in
CI and across broader workloads; no general native performance claim is made.

Manifest publication now holds an OS-level exclusive lock for the publication
session. It validates expected `CURRENT`, generation, and parent; syncs immutable
manifest bytes; atomically renames a separately checksummed `CURRENT` pointer;
then syncs the containing directory. Stale compare-and-swap expectations fail
without changing reachability. Compaction publishes through the same CAS
boundary and leaves its input graph unreachable—but intact—until GC.

Database flush follows the crash-safe publication order directly: sync the
active WAL, write/sync/rename the content-addressed segment, create and sync the
successor WAL, persist the next manifest, then advance `CURRENT`. Crashing
before `CURRENT` leaves only unreachable artifacts and recovers the old WAL;
crashing after it finds both the new segment and successor WAL. Old WALs remain
reachable through historical manifests/checkpoints until GC proves otherwise.

## Frozen vectors

- [`wal-v1.hex`](../crates/vyrm-kv/fixtures/wal-v1.hex)
- [`batch-v1.hex`](../crates/vyrm-kv/fixtures/batch-v1.hex)
- [`manifest-v1.json`](../crates/vyrm-kv/fixtures/manifest-v1.json)
- [`snapshot-bundle-v1.hex`](../crates/vyrm-kv/fixtures/snapshot-bundle-v1.hex)

CRC32C calculation uses the platform-dispatched implementation while retaining
the exact v1 bytes and published `123456789` check value. The tests also cover
ordered replay, reopen/continuation, invalid batches, partial headers, partial
payloads, complete checksum corruption, unknown versions, explicit repair, and
repair/recovery idempotency. Segment tests cover sparse-reader/Memtable point,
range, and MVCC differentials, v1/v2 backward reads, same-key versions spanning
blocks, cache bounds/eviction, authenticated-length mismatch, post-open block
tampering, compressed-body corruption, checksum failure, truncation, and Linux
RSS bounds.
