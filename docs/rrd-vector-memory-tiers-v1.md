# RRD vector memory tiers v1

Status: engine foundation implemented and locally qualified for G04-W05.
Generated outward administration bindings, distributed placement, automatic
warming, and production fixed-hardware sizing remain separate work.

RRD treats a named vector's memory tier as process-local physical policy over
immutable, content-addressed artifact bytes. The durable vector catalogue,
quantization lifecycle, canonical vectors, and object receipts remain the
source of truth. A tier never creates another catalogue and never changes
logical query results.

## One engine-owned authority

Every `RrdEngine` owns one `VectorResidencyManager`. Search first reconstructs
the exact active planner descriptors from durable metadata without reading
artifact bodies. It then filters descriptors by scope, field, dimensions,
metric, embedding-model digest, filter coverage, projection state, and exact or
approximate mode. Only request-compatible active generations are presented to
the residency manager.

Retired generations are used while replaying the catalogue's generation chain,
but are removed from the serving manifest before bytes load. Reconciliation
removes any resident key no longer present in that active manifest. The
residency key binds scope, projection identity, generation, and object digest;
decoded bytes must still reproduce the durable descriptor exactly.

## Physical policies

| Tier | Physical behavior | Capacity behavior |
|---|---|---|
| `pinned` | Owned decoded artifact is retained across requests. A cached copy is promoted without decoding again. | Admission is hard bounded. An admitted entry is never silently evicted; insufficient capacity returns explicit pressure. |
| `cached` | Owned decoded artifact is retained in a process-local byte-accounted LRU. A pinned copy can be demoted without decoding again. | Least-recently-used entries are evicted until the new object fits. An object larger than the complete cache bypasses it and is served transiently. |
| `cold` | No decoded entry is retained. Supported immutable local binary codecs open through verified read-only mmap; JSON or non-local backends use a verified transient owned decode. | The manager holds zero resident bytes after acquisition. The operating system may page mapped bytes, but RRD does not claim explicit OS page-cache control. |

Default limits are 256 MiB pinned and 64 MiB cached. Embedded callers can set
validated limits with `RrdEngine::open_with_vector_residency` or
`RrdEngine::memory_with_vector_residency`. Each declared object length is the
conservative admission/accounting unit, so decoded serving bytes cannot be
smuggled outside the configured manager budget.

Compact dense, scalar/product/binary quantized, and TurboQuant artifacts have
native read-only mmap codecs. Exact JSON segments and HNSW v2 use the verified
transient-owned cold fallback. A future compact HNSW format can join the mmap
path without changing collection or planner semantics.

## Pressure, corruption, and fallback

Resource pressure and integrity failure are deliberately different:

- object absence, digest/length corruption, codec corruption, or descriptor
  substitution fails the request closed;
- a pressure-denied artifact is removed only from the current process-local
  planner view;
- `allow_approximate` may consequently choose the canonical exact scan;
- `require_approximate` returns `ResourceExhausted` when pressure removes the
  required approximate path;
- durable catalogue revision, active generation, and object reachability are
  not rewritten because one process is short of memory.

The manager serializes admission, LRU ordering, tier transitions, and
reconciliation. Search releases that lock before planning and scoring;
immutable artifacts are shared by `Arc`, so serving never holds the policy lock
while it executes a vector kernel.

## Restart and evidence

Residency is intentionally empty after process restart. The next search replays
authenticated metadata, reacquires only compatible active bytes under the
current collection tier, and must reproduce the same access-path and hit
semantics. No process cache is treated as recovery state.

`RrdEngine::vector_residency_snapshot` exposes bounded process evidence:
resident entries/bytes, configured limits, hits, misses, evictions, stale
reclaims, tier transitions, cold mmap/owned loads, and oversized-cache bypasses.
The snapshot is diagnostic state, not a durable correctness authority.

Executable ownership is split deliberately:

- `runtime::vector_residency::tests` proves hard pinned admission, cached LRU,
  physical promotion/demotion, cold mmap, and stale reclamation;
- `runtime_vector_artifact_catalog` proves metadata replay exposes only the
  active generation;
- `engine_vector_memory_tiers_are_physical_bounded_and_restart_safe` proves
  public cached/pinned/cold behavior, restart equality, exact pressure fallback,
  and required-approximate resource exhaustion through `RrdEngine`.

## Explicit boundary

The tier is currently per named vector and applies uniformly to all compatible
active artifacts for that vector. Independent original-vector, HNSW,
quantization, sparse-index, payload, and payload-index tiers; admission queues;
automatic workload promotion/warming; distributed residency; and live memory
SLO tuning are not claimed by G04-W05.
