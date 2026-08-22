# vyrmDS unified data and object contract

Status: M4 executable contract, 2026-08-19.

## Canonical values

One `RuntimeCommit` can now carry claims, schema revisions, records, relations,
events, dense/sparse/multivectors, time-series samples, WGS84 point/bounding-box
values, and verified object references. Every mutation advances the same global
cursor and hash chain. The compatibility, memory, and native engines update the
corresponding latest-value keyspaces in the same transaction as the log.

Vector values retain exact caller-supplied floats. Derived embeddings must bind
source digest, model identity and digest, dimensions, normalization, and
generation parameters. Approximate or quantized structures remain rebuildable
M5 projections. Series values have an explicit scalar type and modeled
observation time. Spatial values reject non-finite or out-of-range WGS84
coordinates. Object references bind SHA-256, byte length, media type, canonical
key, backend, version, and ETag evidence.

## Atomic visibility protocol

S3 is not a transaction coordinator. `DataRuntime` therefore implements the
honest boundary:

1. Compute the SHA-256 identity and stage immutable bytes.
2. Durably publish them at `objects/sha256/<prefix>/<digest>`.
3. Read and verify length plus SHA-256 from the backend.
4. Verify every object again immediately before the data commit.
5. Atomically commit canonical mutations, latest-value indexes, hash-chain
   changes, projection outbox work, accepted-operation audit, and the idempotent
   transaction outcome.
6. Treat bytes left by a rejected commit as visible inventory orphans, never as
   referenced truth.

A response lost after commit is safe: retrying the byte-identical transaction
returns its stored outcome instead of duplicating mutations or outbox work.
This is atomic visibility of a verified reference. It is deliberately not a
claim that a remote upload rolls back with local storage.

## Object adapters

`LocalObjectStore` uses create-new staging files, file sync, directory sync,
atomic rename, post-publication digest verification, corruption quarantine, and
explicit orphan inventory/reclamation. It never constructs a path from an
unvalidated digest.

`S3CompatibleObjectStore<C>` owns the same content semantics over the narrow
`S3ObjectClient` port. The transport must provide a real conditional
`put_if_absent`; the adapter refuses to emulate it with unsafe HEAD-then-PUT.
ETags are retained as backend evidence but never assumed to be content hashes.
Endpoint authentication, signing, retry, timeout, and TLS policy belong to the
transport implementation. The current differential uses a deterministic S3
transport fixture; a live endpoint certification is deployment evidence, not a
different canonical contract.

## Transactional evidence

Every projection-relevant mutation writes one deterministic `ProjectionWork`
record keyed by its source cursor. Every accepted commit writes a sealed
`AuditEnvelope`, chained to the prior audit digest. Both are in the same Fjall
transaction or native `vyrmKV` batch as canonical values and the stored commit
outcome. Rejected transactions produce none of them.

## Executable acceptance

`crates/vyrm-store/tests/unified_data.rs` proves:

- one mixed transaction across every canonical family on Memory, Fjall, and
  native engines;
- equal outbox/audit behavior and exact idempotent retry;
- rollback when a late-family reference is dangling;
- object orphan state before commit and recovery after a lost post-commit
  acknowledgement;
- native flush/reopen persistence of values, outbox, audit, and outcome.

Unit tests inject failure at every local publication boundary, detect missing
and corrupt bytes, quarantine corruption, reclaim only explicit orphan
candidates, and differential-test local versus S3-compatible object semantics.

## Remaining deployment evidence

M4 does not certify a particular cloud endpoint. Before a production S3-like
service is named supported, its transport must prove conditional-create,
version/ETag preservation, pagination, error mapping, credentials, retries,
timeouts, and fault behavior against that service. Retention-aware automated GC
also remains gated on mapping runtime snapshot pins to object reachability; the
current reclamation API deletes only an explicit caller-proven digest set.

M7 now carries canonical object references through physical Raft snapshots and
hydrates their immutable bytes before activating a replica. The transfer
manifest is not trusted by itself: the target scans the authenticated VyrmKV
bundle and requires the exact project-scoped `runtime_objects` closure. Local
streaming is fixed-buffer and content-addressed; the current synchronous
S3-compatible client still materializes one object. Multipart/resumable remote
transport, admission/backpressure, and independent-host fault evidence remain
deployment gates.
