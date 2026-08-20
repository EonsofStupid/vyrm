# Fjall to vyrmKV migration contract

Status: implementation gate for removing the Fjall compatibility backend.

## Promise

Migration is an explicit, offline state transition. It copies the byte-exact
contents of all canonical Engine keyspaces from one cross-keyspace Fjall read
snapshot into an absent sibling vyrmKV directory. The staged store is not made
visible until its archive digest, per-keyspace counts, total byte count, and
semantic reopen checks all pass.

The original Fjall directory is retained after cutover. Initial migration does
not delete source data or its authenticated export. Rollback is allowed only
while the native store still has the exact manifest identity and sequence that
were recorded at cutover; otherwise it refuses to discard divergent writes.

## Canonical inventory

The migration format owns the ordered keyspace list in
`vyrm_store::keyspaces::ALL`. A source containing any other keyspace is denied.
An empty canonical keyspace remains part of the inventory. This converts a new
keyspace from an easy-to-miss loop edit into an explicit migration-format
change.

## Archive (`VYRMIG01`, version 1)

The archive is streaming and bounded by the storage substrate's key/value
limits. It contains:

1. magic, version, zero flags, and the canonical ordered keyspace names;
2. ordered records: keyspace ordinal, key length, value length, key, value;
3. a footer with total entries, total key/value bytes, per-keyspace counts, and
   SHA-256 of the complete header and record stream.

Readers reject unsupported versions or flags, reordered/renamed keyspaces,
invalid ordinals, empty or oversized keys, oversized values, non-increasing
keys within a keyspace, inconsistent counters, digest mismatch, truncation, and
trailing bytes. Import writes bounded vyrmKV batches and prefixes each logical
key with `keyspace + NUL`, exactly like `NativeEngine`.

The empty archive is frozen by
`crates/vyrm-store/tests/fixtures/migration-v1-empty.hex`; an incompatible byte
change requires a new format version and golden vector.

## Durable phases

Each phase is recorded through a synced temporary JSON marker, rename, and
parent-directory sync:

1. `exported` — Fjall was synced and one cross-keyspace snapshot was archived.
2. `imported` — the absent native staging directory contains every archive row.
3. `verified` — its visible inventory and digest match the archive and it
   reopens as a native Engine.
4. `source_moved` — Fjall was renamed to the retained backup sibling.
5. `cutover` — staging was renamed to the requested database path and its
   native state token was recorded.
6. `complete` — a final native reopen and semantic status read succeeded.

Filesystem state is authoritative when a crash lands between a rename and its
marker update. Resume recognizes those states and advances rather than
re-exporting or overwriting an artifact. Normal `PersistentEngine::open`
refuses an active marker so a missing path cannot become a new empty database
during the cutover window.

## Recovery rules

- Before `source_moved`, Fjall remains the only visible database and resume may
  reconstruct an invalid staging directory from the authenticated archive.
- Between `source_moved` and `cutover`, resume completes the staging rename; it
  never creates a fresh database at the now-missing source path.
- At or after `cutover`, resume verifies the native state token and completes.
- Rollback moves the unchanged native directory to a retained sibling, restores
  the Fjall backup, and records `rolled_back`. Every artifact remains available
  for diagnosis.
- Unknown, ambiguous, divergent, or corrupt states are denied and require an
  operator decision. Migration never guesses.

## Evidence gate

The compatibility backend is removable only after tests prove complete
multi-keyspace migration, corrupt/truncated archive refusal, unknown-keyspace
refusal, restart at every phase boundary, idempotent resume, rollback before
native divergence, rollback refusal after divergence, and stable backend
selection. A separate deterministic put/update/delete/reopen/compaction soak
must compare vyrmKV and Fjall against an independent ordered-map model.

This design follows the operational invariants—not code—of RocksDB checkpoints
([one consistent database view and an absent target](https://github.com/facebook/rocksdb/wiki/Checkpoints)),
Qdrant snapshot restore
([explicit restore into a clean target](https://qdrant.tech/documentation/snapshots/)),
and SurrealDB logical migration
([validate imported state before switching](https://surrealdb.com/docs/build/deployment/surrealdb-cloud/operations/migrating-data)).
Those systems remain benchmark baselines; the document makes no unmeasured
superiority claim.
