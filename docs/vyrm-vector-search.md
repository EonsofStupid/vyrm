# Vyrm vector/search contract (M5)

Status: local executable reference gate, 2026-08-19.

`vyrm-vector` is the rebuildable search layer over canonical `RuntimeVector`
versions. The data-runtime commit log remains truth. An index may accelerate a
query, but it cannot invent freshness, visibility, filtering, scoring, or
ordering semantics.

## Frozen semantics

- Queries are scoped by a validated `ReadStamp`, transaction cursor, and valid
  time.
- Dense, sparse, and multi-dense `MaxSim` exact search support cosine, dot,
  Euclidean, and Manhattan scoring. Higher is always better; distance metrics
  return negative distance.
- The latest transaction-visible version wins for each vector identity. A
  future valid-time version does not hide an earlier applicable version; a
  retired latest version does.
- Exact integer/unsigned/decimal/string filters implement `equals`,
  `not_equals`, `in`, ranges, existence, `all`, `any`, and `not`, with explicit
  missing-property behavior and bounded AST depth/size.
- Optional exact embedding-model bindings prevent same-shaped but incompatible
  vector spaces from being mixed by exact scans, artifacts, or the planner.
- Duplicate `(vector identity, source cursor)` versions, dimensional drift,
  non-finite values, corrupt artifacts, wrong fields/metrics/scopes, incomplete
  filter coverage, and stale generations fail closed.
- Result ordering is score descending, reference ascending, then source cursor
  descending. The borrowing exact API avoids a corpus copy on the hot path.

The portable contract and projection identities are frozen by
`crates/vyrm-vector/fixtures/vector-search-v1.json`.

## Persistent collection control

The first F7 product-facing control surface is now executable. A
`VectorCollectionRepository` stores a versioned catalogue in authoritative
control state and advances it through compare-and-swap journal transitions.
Each collection owns one or more named vector definitions binding field, value
kind, dimensions, metric, optional embedding-model digest, and requested
pinned/cached/cold placement. Accepted mutations retain bounded operation
receipts so exact retries survive restart and changed payloads under one key
conflict.

RRD exposes authenticated ensure/list operations with separate deny-by-default
actions. Collection-addressed search resolves the stored definition and rejects
kind/dimension drift before planning. Atomic `put_vector` mutations may also
bind collection plus vector name; commit validates field, kind, dimensions, and
model provenance before retaining the vector and payload properties in the
unified data transaction. Native reopen and real-process tests prove replay,
collision, list, denial, bound point commit, and exact search through the
catalogue. Dedicated point lifecycle/scroll APIs, payload indexes, physical
memory-tier enforcement, and persistent ANN artifact serving remain open.

RRD search now exposes the oracle's complete bounded payload-filter algebra:
equals/not-equals, membership, range, existence, and recursive all/any/not.
The server lowers public values into the existing exact filter evaluator rather
than maintaining transport-specific semantics. A real committed point proves a
matching tenant filter returns it and a non-matching filter returns no hit.
This is exact one-stage semantic filtering; persisted payload indexes and
persisted filter-aware HNSW serving remain separate open gates.

`POST /v1/vector/points/scroll` adds the first dedicated point-read lifecycle
operation. It resolves the named collection space, captures one authoritative
read stamp, calls the same `materialize_visible` primitive as search, applies an
optional payload filter, orders exact references, and returns bounded vector,
provenance, payload, source-cursor, and resume evidence. The route has its own
deny-by-default action.

`POST /v1/vector/points/retrieve` accepts a bounded unique identity batch,
resolves the same collection visibility snapshot, returns found points in
request order, and explicitly returns missing references. It has an independent
authorization action and does not infer absence from an omitted result.
First-class deletion remains open.

## Rebuildable projections

Two canonical JSON reference artifacts currently exist:

1. `ImmutableVectorSegment` stores authenticated exact candidate history.
2. `HnswIndex` stores deterministic dense-vector HNSW with a wider layer zero,
   heap-based traversal, filter-aware candidate admission, and exact reranking.

Both carry a `ProjectionStamp` with contract version, identity, configuration
digest, artifact digest, generation, source cursor, and lifecycle state. The
unified `VectorCatalog` publishes with compare-and-swap revision control,
advances generations exactly once, moves replaced artifacts to `retiring`,
quarantines only the active ready generation, and reclaims retired digests only
when no supplied `(projection id, generation)` pin protects them.

`VectorRuntime` is the in-process coordinator. It plans from the current catalog
and then rechecks the selected artifact against the exact published descriptor
before execution. `Exact` never selects HNSW. `RequireApproximate` fails if no
fresh HNSW exists. `AllowApproximate` can fall back to exact. Highly selective
filters raise estimated graph cost because traversal still needs non-matching
nodes for navigation; the reference planner uses a conservative four-unit
navigation multiplier.

The in-process coordinator is not durable truth. Node publication explicitly
records the artifact codec (`exact_segment`, `compact_dense`, or `hnsw`), stages
its content-addressed bytes, and then commits a strict `vector_artifact` record
with the verified object reference in one `vyrmDS` transaction. Only a
successful commit replaces the serving view. Restart reconstructs catalog
revision order from the typed log, proves record and object shared a commit,
loads verified bytes through the object port, decodes the declared codec, and
requires exact descriptor equality. Revision gaps, stale publishers, missing
objects, digest/length differences, and codec substitution fail closed.
Connectome exposes the bounded catalog metadata and receipts without raw vector
payloads.

## Evidence

Reproduce the retained fixed-seed profile with:

```bash
cargo run --locked --release -q -p vyrm-vector \
  --example vector_evidence -- 10000 128 25
```

Retained raw output:
[`evidence/m5-vector-local-10000x128.json`](evidence/m5-vector-local-10000x128.json).
The run used rustc 1.95.0 on an 8-vCPU Intel Xeon E5-2699 v4 KVM guest. Timing
is a single local observation, not a cross-machine performance claim.

| Filter | `ef` | Recall@10 | exact ms | HNSW ms | planner |
|---:|---:|---:|---:|---:|---|
| 100% | 64 | 0.664 | 23.38 | 2.18 | HNSW |
| 100% | 128 | 0.892 | 24.56 | 4.10 | HNSW |
| 100% | 256 | 0.980 | 23.46 | 5.91 | HNSW |
| 50% | 128 | 0.972 | 20.27 | 6.22 | HNSW |
| 10% | 64 | 0.992 | 16.08 | 8.55 | HNSW |
| 10% | 256 | 1.000 | 16.12 | 18.09 | exact scan |
| 1% | 32 | 1.000 | 15.42 | 19.49 | exact scan |
| 1% | 128 | 1.000 | 15.81 | 44.65 | exact scan |

The same run observed:

- 18.94 s deterministic HNSW construction;
- 19,971,560 artifact bytes for 5,120,000 raw f32 payload bytes (3.90×);
- 19,764 KiB RSS before build, 90,060 KiB after reopen, and 130,348 KiB
  high-water RSS while old and reopened generations overlapped;
- experimental per-vector symmetric int8 payload at 25.78% of raw f32 size,
  mean absolute cosine-score error 0.000274, and 1.0 recall@10 after exact
  reranking 64 candidates.

The deterministic test matrix additionally covers an independent scalar exact
oracle, Memory/Fjall/native log differential, 512-vector unfiltered/selective
recall gate, corrupt/stale denial, and eight generations of mixed updates,
valid-time deletes, deterministic rebuild, byte reopen, catalog replacement,
snapshot-protected retirement, and reclamation.

## M6 extension

M6 adds a compact exact dense format with mmap reads, scalar/AVX2 differential,
model-bound requests/projections, and strict accelerator-build admission. See
[`vyrm-embedding-edge.md`](vyrm-embedding-edge.md). The JSON segment remains
the portable M5 semantic fixture, while the compact format is the production
dense payload direction.

## Honest boundary

This establishes the local M5 semantic and measurement baseline. It does not
establish superiority over Qdrant or any other vector database.

- HNSW currently accelerates only dense vectors; sparse and multi-vector ANN
  remain exact-only.
- Scalar quantization is an experiment, not a published planner path.
- The older `ScalarQuantizedVector` remains a separate per-vector symmetric
  int8 experiment. `TurboQuantVector` now implements a deterministic MSE
  TurboQuant variant with seeded randomized Hadamard rotation, fixed
  standard-normal Lloyd-Max codebooks, 4/2/1.5/1-bit packing, per-vector norm
  correction, and asymmetric dense-query scoring. `TurboQuantSegment` removes
  full-f32 payloads from its authenticated binary artifact, participates in the
  projection catalogue/planner, and supplies candidates for exact-f32 reranking.
  Public artifact build/lifecycle APIs, SIMD, broad quality/latency evidence,
  and the paper's residual QJL estimator are not implemented.
- HNSW graph artifacts remain canonical JSON and storage-heavy. Dense exact
  payloads now have a compact mmap representation; compact graph/payload bitmap
  indexes and background optimization remain open.
- Scalar and AVX2 exact kernels exist. The GPU boundary verifies adapter output,
  but no physical GPU adapter, shard replication, or live cross-system
  benchmark is certified yet.
- Recall depends strongly on dimension, corpus, graph parameters, filter
  selectivity, and `ef`. The low-`ef` rows are intentionally retained so the
  project cannot hide that quality/latency tradeoff.

Cross-system Qdrant proof remains a separate fixed-hardware protocol after the
remaining production paths are ready.

## TurboQuant implementation and remaining promotion gate

The primary contract is Zandieh et al.,
[“TurboQuant: Online Vector Quantization with Near-optimal Distortion Rate”](https://arxiv.org/abs/2504.19874)
(ICLR 2026). The landed MSE variant has frozen deterministic tests for
normalization/norm retention, seeded rotation, fixed distribution-matched
centroids at every supported bit width, packed-code decoding, asymmetric
scoring, authenticated artifact reopen/corruption denial, planner selection,
and exact reranking. It deliberately does not claim the paper's residual QJL
estimator. Exact f32 vectors remain authoritative for quality measurement and
final reranking.

The evidence matrix must report encode/index time, bytes per vector including
norms/seeds/residual sketches, MSE, inner-product bias/variance, Recall@k before
and after exact reranking, filtered recall, query throughput and p50/p95/p99,
scalar/SIMD parity, artifact authentication, reopen, and adversarial/non-power-
of-two dimensions. Before product promotion, the remaining rows—especially
large-corpus bias/recall, filtered quality, scalar/SIMD parity, mmap and public
lifecycle/recovery administration—must pass. The planner-visible artifact is
therefore an internal alpha path, not a Qdrant-equivalence or superiority claim.
