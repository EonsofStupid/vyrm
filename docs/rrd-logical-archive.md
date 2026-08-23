# RRD logical archive v1

Status: F1 contract, frozen for the first operable backup/restore slice.

The RRD logical archive is a backend-independent replay of authoritative Engine
operations. It is not a copy of VyrmKV files and it is not the Fjall migration
archive. Its purpose is recovery across physical formats and storage adapters.

## Consistency

An export captures the claim-sequence and runtime-cursor watermarks, reads both
logs through those bounds, and reads the watermarks again. If either watermark
changed, export fails and publishes no archive. This gives the exporter one
stable logical cut without pretending that two separate Engine reads are an
atomic substrate snapshot.

The exporter validates every runtime cursor, commit ordinal, commit digest,
change digest, and previous-change digest. It reconstructs each original
`RuntimeCommit`, then correlates its claim mutations with the append-ordered
claim log. Standalone claim appends are emitted before the claim-bearing commit
that originally followed them. Claim mutations within a commit must occupy a
contiguous claim-sequence interval.

## Format and integrity

Version 1 is a framed binary stream:

- fixed `RRDLAR01` magic, archive version, RRD contract version, and source
  watermarks;
- length-delimited canonical JSON actions (`standalone_claim` or
  `runtime_commit`);
- a footer containing action counts, mutation counts, and SHA-256 over every
  preceding byte.

The file is written to a sibling temporary file, synced, renamed into place,
and followed by a parent-directory sync. A mismatched magic, version, length,
counter, digest, replay cursor, or replay sequence is denied before restore
mutates a target.

SHA-256 provides content authentication against accidental or untrusted-byte
corruption. It does not authenticate the identity of the backup producer.
Signing, encryption, key rotation, and authorization belong to F4 and must not
be implied by this F1 format.

## Restore

Restore accepts only an absent destination. It validates the complete archive
first, replays it into a uniquely named sibling staging root, verifies both
watermarks, flushes, closes, and reopens the database, verifies again, and only
then renames staging to the requested root. A failed attempt removes only its
own exact staging root; the requested destination is never partially exposed.

Runtime commits are replayed through `Engine::commit_runtime`; standalone
claims use `Engine::append_batch`. Therefore schema validation, cursor CAS,
idempotency, projection outbox generation, and audit generation travel through
the same production paths as live writes.

## Explicit boundary

Version 1 archives authoritative claims and typed runtime state, including
immutable object references. Object payload closure, projections, invocation
telemetry, snapshot leases, and policy/session state are not silently claimed
as covered. The backup catalogue slice must pair this logical archive with its
object inventory and describe every included component before a backup can be
called application-complete.

## Backup catalogue v1

The local catalogue stores authenticated JSON at `catalogue.json` and retains
logical archives under `archives/<archive-sha256>.rrd-archive`. Each entry has a
stable backup identity derived from label, caller-supplied creation time, and
archive digest. Repeating the same request is idempotent. Catalogue writes use
a synced sibling temporary file and rename, entries have canonical order, paths
cannot escape the catalogue, and full verification authenticates every retained
archive before restore.

The CLI surface is:

```text
vyrm --db SOURCE storage archive-export --archive FILE
vyrm --db SOURCE storage archive-inspect --archive FILE
vyrm --db ABSENT_TARGET storage archive-restore --archive FILE
vyrm --db SOURCE storage backup-create --catalogue DIR --label LABEL
vyrm --db SOURCE storage backup-list --catalogue DIR
vyrm --db ABSENT_TARGET storage backup-restore --catalogue DIR --backup-id ID
```
