# RRD tiered persistence

Status: G02-W06 executable candidate, 2026-08-29.

RRD has one canonical transaction and storage authority. Tiering changes how
immutable bytes are accessed; it does not create another logical database,
write path, or source of truth.

## S3-compatible immutable objects

`S3CompatibleObjectStore<C>` admits a transport only when it declares SigV4 or
mTLS authentication, signed payloads, real conditional writes, SHA-256
checksums, complete pagination, resumable multipart/list-parts, and ranged
reads. The transport owns credentials, HTTP, TLS, endpoint configuration, and
service-specific error mapping. RRD owns content addressing, expected length
and digest, retry admission, resume comparison, conditional publication, and
post-publication verification.

Upload retains at most one configured part. A restarted request finds the
content-addressed multipart upload, lists every part, rereads the source, and
reuses a part only when number, length, and SHA-256 all match. Parts are
consecutive from one; completion is conditional and carries every part ETag and
checksum. Only `RemoteObjectTransient` is retryable, for at most the configured
attempt count.

Verification and `open_verified` use bounded version-bound ranges. The latter
borrows the store and fetches one range at a time, so archive restore, object
hydration, and artifact transfer no longer require a whole remote allocation.
The consumer still verifies the declared full digest before local atomic
publication.

## Native immutable segment I/O

`DatabaseOptions::segment_io` selects `auto`, `mmap`, `io_uring`, or `bounded`.
Auto probes Linux ring setup and `IORING_OP_READ`; unsupported systems record a
reason and use bounded positional reads. An admitted ring performs actual
offset reads. If a file operation reports a runtime unsupported/permission ABI
error and fallback is allowed, that reason is recorded and subsequent reads use
the bounded path. Other I/O errors propagate.

Mmap, io_uring, and bounded reads all enter the same compressed-block digest,
decode, MVCC, and shared decoded-cache code. `SegmentIoStats` reports segment
selection, fallback count/reason, total and per-backend read operations, bytes,
and peak request bytes. `BlockCacheStats` separately reports decoded resident
bytes, loads, hits, misses, and evictions. This prevents storage access and
compute/cache cost from being blended into an unverifiable low-memory claim.

## Qualification boundary

Local tests prove exact results across mmap and positional I/O, actual io_uring
when the running kernel admits it or explicit fallback otherwise, bounded
request/cache evidence, interrupted remote resume, transient retry, corruption
refusal, logical-archive transfer, and local-loss recovery.

This qualifies the provider-neutral engine contract. A named AWS, MinIO, or
other endpoint still requires its concrete transport and deployment to pass
credential, TLS/endpoint, conditional-operation, pagination, timeout, error
mapping, and independent-host fault certification. Vector-specific hot/cold
placement and compression residency remain G04 work.
