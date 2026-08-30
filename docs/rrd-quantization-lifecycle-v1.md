# RRD quantization artifact lifecycle v1

Status: engine foundation implemented and locally qualified for G04-W04. Generated
HTTP, MCP, CLI, and SDK bindings remain G06 work. This is not a fixed-hardware
production benchmark or a claim of Qdrant equivalence.

RRD treats quantization as an immutable derived projection of canonical dense
vectors. Scalar, product, binary, and TurboQuant use one authenticated engine
lifecycle and one planner. None of the codecs creates a second vector database
or becomes authoritative for returned scores.

## Public engine contract

The strict `rrd-contract` request and result types expose four authenticated
operations:

- `build_vector_quantization_artifact` builds the next immutable generation in
  `ready` state without changing serving.
- `list_vector_quantization_artifacts` returns bounded lifecycle and object
  evidence.
- `activate_vector_quantization_artifact` verifies the content-addressed object
  and makes exactly one generation for that artifact identity planner-visible.
- `retire_vector_quantization_artifact` removes a generation from planning
  without deleting canonical vectors or rewriting history.

Build, activate, and retire require the existing vector-administration
permission; list requires the vector-list permission. Every request passes the
engine's session authorization and audit boundary. G06 owns generated transport
bindings and must not introduce another lifecycle implementation.

The historical `ensure_vector_index` TurboQuant request is retained as a
compatibility adapter, not a second implementation. It builds or resumes a
matching `ready` lifecycle generation, activates it, and returns the legacy
index-shaped snapshot. A matching active configuration/source stamp replays
idempotently. The generic vector-artifact publisher rejects every quantized
codec, and restart suppresses any pre-lifecycle TurboQuant serving view before
installing lifecycle-selected generations.

## Durable state machine

The immutable artifact record binds collection, named vector, codec kind,
projection descriptor, generation, source cursor, configuration digest,
artifact digest, content-addressed object receipt, and build time. A separate
append-only event stream records `build`, `activate`, and `retire` with contiguous
revision numbers and a SHA-256 predecessor chain.

Build atomically commits the artifact record, object reference, and matching
build event. Lifecycle reconstruction rejects duplicate or missing builds,
revision gaps, digest-chain changes, non-contiguous generations, backward
activation, and record/object commits that were not atomic. The catalogue is
reconstructed at an exact read stamp and that same stamp is consumed by the
write CAS, so concurrent lifecycle changes cannot publish a semantically stale
event. Build and transition operations also emit durable projection traces and
link the active reasoning run when one exists.

`ready` is intentionally not served. Activation validates the object length,
SHA-256, binary framing, canonical metadata, internal artifact digest, codec
kind, and descriptor before publication. Activating a later generation retires
the prior active generation of the same identity. Restart reconstructs only the
active generations into the shared `VectorRuntime`; corruption or missing bytes
fail closed.

The lifecycle revision and the older generic vector-catalogue revision are
independent durable coordinates. Restoring an active quantized artifact is
therefore revision-neutral for the generic catalogue; a subsequent HNSW
publication still compares against the exact durable vector-catalogue revision.

## Codecs and physical form

| Method | Packed vector ratio | Physical representation |
|---|---:|---|
| Scalar | 4× | Artifact-wide symmetric signed int8 scale and dense codes |
| Product | 4×, 8×, 16×, 32×, or 64× | Deterministic sampled 256-entry subspace codebooks and one-byte centroid codes |
| Binary | 32× | Sign-bit vectors and asymmetric full-precision query scoring |
| TurboQuant | 8×, 16×, 21×, or 32× | Seeded randomized Hadamard rotation, fixed distribution-matched 4/2/1.5/1-bit codes, and norm correction |

The ratio is `full_precision_vector_bytes / packed_vector_bytes`; it is not a
claim that the complete file or resident process is smaller by that ratio.
Metadata and product codebooks are reported separately as auxiliary bytes. On
small corpora, product-codebook overhead can make the complete artifact larger
than the canonical f32 payload even when its packed vector codes reach 64×.

All four artifact families have bounded, checksummed binary formats and true
read-only mmap reopen. Scalar and product decoded scoring plus TurboQuant's
rotated dot product use runtime-dispatched AVX2 on supported x86_64 hosts and a
portable scalar oracle elsewhere. Binary scoring uses packed bit operations.
The qualification matrix requires scalar/automatic result parity.

## Query semantics

The existing `SearchMode` controls exact fallback and required approximate
selection. Active quantized descriptors enter the same planner catalogue as
exact segments and HNSW. Scope, field, dimensions, metric, embedding-model
identity, filter-index coverage, source cursor, valid time, and read cursor are
checked again at execution.

Quantized scores only choose candidate references. `VectorRuntime` resolves
those references against canonical full-precision vector versions and executes
the exact scorer before returning hits. Exact search remains independently
available. A ready, retired, stale, mismatched, or corrupt artifact cannot
silently alter logical truth.

## Local qualification evidence

`crates/rrd-vector/tests/quantization_matrix.rs` uses a fixed 512×64 corpus,
eight queries, top-10 results, and exact reranking over 96 candidates. It covers
all 11 supported codec/ratio rows and asserts:

- exact packed compression ratios through product 64×;
- owned and read-only mmap descriptor identity;
- scalar/runtime-dispatched SIMD result and score parity;
- mean absolute score error no greater than 0.15;
- per-artifact build and eight-query automatic-kernel search matrices no
  greater than five seconds on the retained debug-profile gate;
- at least 0.80 mean Recall@10 after exact reranking;
- explicit packed, auxiliary, complete-artifact, and full-f32 byte accounting.

The public engine integration additionally covers authenticated denial, ready
isolation, list, activation, exact reranking, process reopen, retirement,
post-update exact fallback, next-generation rebuild, and reactivation. Codec
unit tests reject framing, checksum, and payload corruption.

The retained local observation is
[`evidence/g04-w04-quantization-local-512x64.json`](evidence/g04-w04-quantization-local-512x64.json).
Its nanosecond values are diagnostic measurements from one debug-profile host,
not release-mode service SLOs. Larger production corpora, p50/p95/p99 fixed-
hardware runs, distributed placement, physical memory-tier eviction, and GPU
qualification remain separately governed work.
