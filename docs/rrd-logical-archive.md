# RRD logical archive v1

Status: G02-W02 implementation candidate. The V1 behavior described here is
implemented and its focused persistence matrix passes locally; exact workspace,
publication, and platform verification remain required before G02-W02 is
crossed off.

The RRD logical archive is a backend-independent replay of authoritative Engine
operations. It is not a copy of RRD LSM files and it is not the Fjall migration
archive. Its purpose is recovery across physical formats and storage adapters.

## Consistency

An export captures the claim-sequence and runtime-cursor watermarks, performs a
bounded validation/count pass, streams the same cut into the archive, and reads
the watermarks again. Claims and runtime changes are read in fixed 1,024-entry
pages. The exporter retains at most one page plus one archive action; one action
has an explicit 64 MiB V1 limit. It never materializes the whole claim log,
runtime log, reconstructed commit list, or action list.

If either source watermark changes, export fails and publishes no archive. An
unfinished export is bound to its original watermarks and audit head; source
advancement invalidates and removes that stale private cut rather than mixing
two observations.

The exporter validates every runtime cursor, commit ordinal, commit digest,
change digest, previous-change digest, original `AuditEnvelope`, previous-audit
digest, and audit head. It reconstructs each original `RuntimeCommit`, then
correlates its claim mutations with the append-ordered claim log. Standalone
claim appends are emitted before the claim-bearing commit that originally
followed them. Claim mutations within a commit must occupy a contiguous
claim-sequence interval.

## Format and integrity

Version 1 is a framed binary stream:

- fixed `RRDLAR01` magic, archive version, RRD contract version, and source
  watermarks;
- length-delimited canonical JSON actions (`standalone_claim` or
  `runtime_commit`); a runtime action carries its exact accepted audit envelope;
- a footer containing action counts, mutation counts, and SHA-256 over every
  preceding byte.

The file is written to a deterministic private partial file. After each action,
the bytes are synced and a content-authenticated receipt is atomically
published with the fixed cut, completed action count, claim/runtime coordinates,
audit head, byte count, and partial digest. Resume re-parses the durable prefix
and compares every retained action to the current fixed source cut. It also
reconciles the crash window where an action is durable but its receipt is one
action behind. The complete authenticated footer is a separate durable
checkpoint: if execution stops before publication, resume validates the finished
private archive against a fresh source summary and publishes those exact bytes.
Only a complete footer is renamed to the requested path.

A mismatched magic, version, length, counter, action, receipt digest, stream
digest, replay cursor, replay sequence, or audit chain is denied.

SHA-256 provides content authentication against accidental or untrusted-byte
corruption. It does not authenticate the identity of the backup producer.
Signing, encryption, key rotation, and authorization belong to F4 and must not
be implied by this F1 format.

## Restore

Restore accepts only an absent destination. It validates the complete archive
first and replays into an archive-addressed private sibling staging root. A
checksummed receipt records each durable action. Resume scans the archive and
staging database, verifies every retained claim, commit, outcome, and audit, and
derives the exact completed prefix from authoritative staging watermarks. This
reconciles interruption before or after receipt publication without duplicate
claims or commits.

After the full prefix is present, restore verifies claim/runtime/audit heads,
flushes, closes, reopens, verifies again, populates the bound catalogue/object
closure when requested, removes the private receipt, and only then atomically
renames staging to the absent target. A corrupt archive never creates staging;
a divergent staging root fails closed and the target remains absent.

Standalone claims use `Engine::append_batch`. Runtime commits use a crate-private
native recovery path that performs the ordinary schema, reference, cursor,
change-chain, projection-outbox, and outcome planning, then verifies the
archived audit envelope against the commit, outcome cursor, prior audit digest,
and optional historical `ReadStamp` before writing the same atomic batch. This
path is not exposed through `Engine`, HTTP, MCP, SDK, or ordinary mutation APIs.

## Explicit boundary

The framed `.rrd-archive` contains authoritative claims and typed runtime state:
schema, records/documents, relations/native edges, events, vectors, time-series,
geo values, immutable object references, and exact runtime audit coordinates.

An application-complete V1 backup binds that stream to a content-addressed
immutable-object manifest, streaming object payload copies, and the vector/index
catalogue manifest in one backup identity. Restore verifies every component
before target publication. Manifest entry counts and bytes have explicit V1
bounds. Projections remain rebuild-required; invocation telemetry and snapshot
leases remain excluded. Retention/RPO/RTO policy belongs to G02-W03, general
format migration to G02-W04, signer identity/encryption to G05 security work,
and remote/S3 low-memory qualification to G02-W06.

## Backup catalogue v1

The local catalogue stores authenticated JSON at `catalogue.json` and retains
logical archives under `archives/<archive-sha256>.rrd-archive`. Each entry has a
stable backup identity derived from label, caller-supplied creation time, and
the bound archive/object/catalogue digests. Repeating the same request is
idempotent. Catalogue writes use
a synced sibling temporary file and rename, entries have canonical order, paths
cannot escape the catalogue, and full verification authenticates every retained
archive before restore.

## Focused executable evidence

The G02-W02 matrix covers more than the required ten persistent scenarios:

- 2,100 standalone claims crossing multiple source pages;
- one 1,100-mutation atomic commit split across runtime pages;
- schema, document/record, relation/edge, event, vector, time-series, geo,
  object-reference, and claim round trip;
- exact transaction `ReadStamp`, audit chain, outcome, and reopen identity;
- interruption after action durability and after receipt durability for both
  export and restore;
- interruption after the export footer is durable but before final publication;
- receipt tampering, source advancement, archive corruption, truncation, and
  existing-target denial;
- application backup object bytes, catalogue revision/record, and audit closure;
- substituted object/archive/catalogue bytes denied before target publication.

The format is intentionally RRFlow-specific. Apache Arrow IPC also processes
unbounded data as ordered messages/record batches, while Qdrant distinguishes
portable collection snapshots from disk-level disaster-recovery backups. Those
are design references, not parity or superiority evidence. See the official
[Arrow IPC format](https://arrow.apache.org/docs/format/Columnar.html#serialization-and-interprocess-communication-ipc)
and [Qdrant snapshot contract](https://qdrant.tech/documentation/operations/snapshots/).

The CLI surface is:

```text
rrflow --db SOURCE storage archive-export --archive FILE
rrflow --db SOURCE storage archive-inspect --archive FILE
rrflow --db ABSENT_TARGET storage archive-restore --archive FILE
rrflow --db SOURCE storage backup-create --catalogue DIR --label LABEL
rrflow --db SOURCE storage backup-list --catalogue DIR
rrflow --db ABSENT_TARGET storage backup-restore --catalogue DIR --backup-id ID
```
