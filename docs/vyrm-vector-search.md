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
- Duplicate `(vector identity, source cursor)` versions, dimensional drift,
  non-finite values, corrupt artifacts, wrong fields/metrics/scopes, incomplete
  filter coverage, and stale generations fail closed.
- Result ordering is score descending, reference ascending, then source cursor
  descending. The borrowing exact API avoids a corpus copy on the hot path.

The portable contract and projection identities are frozen by
`crates/vyrm-vector/fixtures/vector-search-v1.json`.

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

## Honest boundary

This establishes the local M5 semantic and measurement baseline. It does not
establish superiority over Qdrant or any other vector database.

- HNSW currently accelerates only dense vectors; sparse and multi-vector ANN
  remain exact-only.
- Scalar quantization is an experiment, not a published planner path.
- Reference artifacts are canonical JSON and are storage-heavy. A compact
  binary/mmap representation is required before edge or competitive claims.
- There is no SIMD kernel, payload bitmap index, background optimizer, GPU
  builder, shard replication, or live cross-system benchmark yet.
- Recall depends strongly on dimension, corpus, graph parameters, filter
  selectivity, and `ef`. The low-`ef` rows are intentionally retained so the
  project cannot hide that quality/latency tradeoff.

M6 owns embedding jobs, compact/edge packaging, SIMD/GPU builders, and parity
evidence. Cross-system Qdrant proof remains a separate fixed-hardware protocol
after those paths are ready.
