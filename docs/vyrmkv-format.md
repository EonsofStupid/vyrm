# vyrmKV native format contract

Status: M3 local promotion baseline passes. WAL, atomic-batch, manifest, and
checkpoint formats are version 1; new immutable segments are version 2 and the
reader retains explicit version-1 compatibility. The format is pre-release.
Any format change before alpha must increment its explicit version and update
the checked-in vectors; readers never guess.

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

Immutable segment v2 stores a fixed 48-byte `VYRSEG02` header, an LZ4 block with
a prepended decompressed-size field, and a lowercase ASCII SHA-256 footer over
the physical header and compressed body. Its header declares version, length,
compression flags, entry count, sequence range, and uncompressed record bytes.
The decoder bounds decompressed bytes to 1 GiB, requires the LZ4 prefix to equal
the authenticated declared size, then validates the canonical record stream.
Unknown flags, corrupt compressed bodies, invalid ordering, and trailing bytes
fail closed. Version-1 `VYRSEG01` uncompressed segments remain readable through
an explicit decoder branch; writers emit only version 2.

After validation, the reader retains canonical record bytes plus a sparse index
of byte ranges into that same buffer rather than cloning index keys or
materializing every immutable key/version in another ordered map. Point, range,
snapshot, and compaction iteration stream exact MVCC records.
Put/tombstone records retain every version needed for an older snapshot. Files
are named by their physical-content digest, written to a unique temporary,
synced, atomically renamed, and followed by a directory sync. Reusing an
existing content identity first revalidates the complete segment.

Named checkpoints are separately checksummed, atomically published files that
pin an immutable manifest generation. Names use a path-safe canonical grammar;
repeating identical bytes is idempotent, rebinding a name fails closed, and
release is explicit and directory-synced. Retention and GC consume this
inventory rather than inferring reachability from filenames.

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

CRC32C calculation uses the platform-dispatched implementation while retaining
the exact v1 bytes and published `123456789` check value. The tests also cover
ordered replay, reopen/continuation, invalid batches, partial headers, partial
payloads, complete checksum corruption, unknown versions, explicit repair, and
repair/recovery idempotency. Segment tests cover sparse-reader/Memtable point,
range, and MVCC differentials, v1 backward reads, authenticated-length mismatch,
compressed-body corruption, checksum failure, and truncation.
