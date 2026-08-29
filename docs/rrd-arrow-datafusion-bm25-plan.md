# RRD Arrow, DataFusion, and BM25 integration plan

**Status:** Q1/Q2 execution plane implemented; retrieval breadth remains, 2026-08-29
**Scope:** the shared query/retrieval center of the one RRFlow engine
**Authority:** `RrdEngine`; no UI, adapter, SDK, or lower physical crate owns
query or retrieval truth

## Current executable truth

The repository now has an executable shared retrieval vertical:

- `rrd-query` parses, binds, and plans RRFlowQL, converts the captured stamped
  snapshot to typed Arrow batches, and executes non-text relational operators
  through DataFusion 55.
- BM25 analysis, corpus statistics, deterministic authenticated artifacts,
  `MATCH` binding/scoring, persistent catalogue publication, and reopen tests
  exist.
- `RrdEngine` owns HNSW and TurboQuant build/publication/reopen/selection,
  stale-generation fallback or denial, exact reranking, and authenticated
  backup/restore of TurboQuant payloads.
- `RrdEngine::search_hybrid` executes BM25 and vector branches at one captured
  read stamp and performs deterministic weighted reciprocal-rank fusion.

The remaining gaps are specific rather than existential. DataFusion now reads
the bounded materialized snapshot through an immutable RRD `TableProvider` and
physical record-batch stream. Exact projection/limit hints, unsupported-filter
retention, elapsed cancellation, combined snapshot/operator memory accounting,
bounded temporary spill, and stable `EXPLAIN ANALYZE` evidence are executable.
Lazy storage-to-Arrow scans, sparse and late-interaction fusion, broader
analyzer support, and capability-catalogue promotion remain open.

| Slice | Current state |
|---|---|
| Q1 Arrow snapshot bridge | Implemented: one stamped schema, bounded batches, stable dynamic-field typing, and source-family differentials |
| Q2 DataFusion execution | Implemented for the captured-snapshot plane: custom provider, physical stream, governed pushdown dispositions, memory/spill/time budgets, and stable analysis evidence |
| Q3 native BM25 projection | Implemented through engine ensure/query/reopen; broader analyzer and operational-surface qualification remain |
| Q4 unified retrieval | Partial: engine-owned HNSW, TurboQuant, BM25, and reciprocal-rank fusion pass reopen/staleness tests; sparse/late-interaction breadth remains |

## Source-derived design constraints

Apache DataFusion defines three separate extension responsibilities:
`TableProvider` describes a source and accepts pushdown hints,
`ExecutionPlan` describes physical execution, and
`SendableRecordBatchStream` performs the actual work. Planning must not perform
storage I/O. RRFlow therefore captures and validates an immutable RRD read
stamp before registering a provider; provider scans operate only on the
captured snapshot and cannot observe a later catalogue or data head.

Qdrant's BM25 path separates document term-frequency encoding from corpus IDF
statistics. The score used by its local reference path is:

```text
idf(term) = ln(((document_count - document_frequency + 0.5)
                / (document_frequency + 0.5)) + 1)

score(query, document) = sum(
  idf(term) * term_frequency * (k1 + 1)
  / (term_frequency + k1 * (1 - b + b * document_length / average_length))
)
```

RRD will implement that behavior as a native versioned projection rather than
calling a Python service or maintaining a second database.

Primary references:

- <https://datafusion.apache.org/library-user-guide/custom-table-providers.html>
- <https://github.com/apache/datafusion>
- <https://github.com/qdrant/fastembed/blob/main/fastembed/sparse/bm25.py>
- <https://github.com/qdrant/qdrant/issues/2796>
- <https://github.com/qdrant/qdrant-client/blob/master/qdrant_client/local/local_collection.py>

## Cohesion invariants

1. `RrdEngine` is the only public composition root.
2. Canonical data remains in the RRD WAL/MVCC/runtime log. Arrow batches,
   scalar indexes, BM25 postings, HNSW graphs, and TurboQuant segments are
   rebuildable projections, never competing truth.
3. Every plan binds one `ScopeId`, `ReadStamp`, schema revision, valid time,
   known-at cursor, actor, and authorization decision.
4. The query catalogue contains scalar, text, vector, temporal, geo, and graph
   projection metadata. Specialized physical artifacts do not create separate
   catalogues visible to users.
5. A projection is selectable only when its configuration digest, source
   cursor, schema revision, valid time, generation, content digest, and state
   match the bound read.
6. Stale, corrupt, incomplete, or unauthorized projections fail closed. An
   exact authoritative fallback is allowed only when its declared resource
   budget can complete.
7. Mutations publish projection work/invalidation through the same atomic
   runtime commit. Index refresh may be asynchronous; freshness evidence may
   not be.
8. RRFlowQL is the temporal and multi-model language front end. It lowers to
   DataFusion expressions/plans directly; it is not converted into a second SQL
   string and reparsed.
9. Arrow schemas are versioned RRD contracts. Runtime values are mapped to
   typed Arrow arrays; a JSON-blob column is not the primary execution model.
10. Embedded, daemon, MCP, CLI, SDK, and Connectome requests invoke the same
    engine operation and receive the same plan/read evidence.

## Dependency-ordered implementation

### Q1 — Arrow snapshot bridge

- Add pinned Arrow/DataFusion dependencies to `rrd-query` with the smallest
  feature set required for in-process execution.
- Define stable Arrow schemas for record/document, relation, event,
  time-series, geo, claim, and traversal result rows.
- Convert a bound authoritative snapshot into bounded `RecordBatch` streams.
- Preserve canonical identity, valid-time fields, source cursor, schema
  revision, and typed user properties.
- Differentially prove Arrow round trips against the current reference rows.

**Exit gate:** every currently supported RRFlowQL source produces equivalent,
deterministically ordered typed rows from the same immutable `ReadStamp`.

### Q2 — DataFusion execution plane

- Introduce an `RrdSnapshotTableProvider` over the Q1 snapshot. Its `scan`
  method performs no storage I/O and honors projection, filter, and limit
  pushdown only when semantics are exact.
- Lower bound RRFlowQL filters, projections, ordering, and limits to DataFusion
  logical expressions.
- Execute through DataFusion's physical planner and Arrow batch streams.
- Retain RRFlow's authorization, temporal binding, graph expansion, read-stamp
  validation, budgets, deterministic identity ordering, and plan evidence
  around that execution.
- Remove the bespoke evaluator after differential and live-query callers use
  the shared executor.

**Exit gate:** `RrdEngine::execute_query` and live-query recomputation use the
DataFusion path; no adapter or UI calls `rrd_query::execute` directly.

Implemented evidence includes a reference-order differential under a forced
external sort spill, exact filter/projection/limit result differentials across
memory/Fjall/native backends, deterministic timeout and spill-cap denial, Arrow
stamp corruption denial, and safe execution from both synchronous callers and
an existing async server runtime. The provider deliberately declares filters
unsupported until an RRD-semantic predicate implementation can prove exactness;
DataFusion retains those predicates above the scan.

### Q3 — native BM25 projection

- Extend the shared index definition with an explicit text-index kind and a
  versioned analyzer configuration. Initial executable analyzer support is
  Unicode word tokenization plus deterministic Unicode lowercase; unsupported
  stemming/language configurations are rejected rather than silently ignored.
- Build a content-addressed artifact containing document identities, document
  lengths, average length, document frequencies, sorted postings with term
  frequencies, source/schema/valid-time coordinates, and analyzer/BM25
  configuration digests.
- Validate canonical encoding and the artifact digest before publication and
  after reopen.
- Add a RRFlowQL text-match predicate and bind it only to string fields.
- Select a ready matching BM25 artifact when fresh. Otherwise run the same
  scorer over an authoritative bounded snapshot or deny when the exact fallback
  exceeds its budget.
- Return score, matched-term evidence, access path, scanned postings/documents,
  source cursor, and exactness in the engine result.

**Exit gate:** build, close, reopen, query, update, invalidate, rebuild,
corruption denial, and exact-reference differential tests all pass through
`RrdEngine`.

### Q4 — unified retrieval and existing vector artifacts

- Move HNSW/TurboQuant build, publication, reopen, and selection behind
  engine-owned operations using the shared projection catalogue coordinates.
- Permit one-stage filter-aware ANN only when filter coverage is proved;
  otherwise choose a correct fallback.
- Add dense, sparse, BM25, and late-interaction branches to one retrieval plan.
- Add reciprocal-rank fusion first, then distribution-based fusion and explicit
  exact reranking policies.

**Exit gate:** `RrdEngine` can execute lexical, dense, sparse, multivector,
HNSW, TurboQuant, and hybrid plans without the caller constructing an
`rrd-vector` or `rrd-query` runtime beside it.

## Required verification for this foundation slice

The earlier minimum-ten persistent-scenario requirement remains a release
floor. For this query/retrieval center, the gate additionally requires:

1. Arrow/reference differential for every supported model.
2. DataFusion/reference differential across filters, projections, limits, and
   temporal selectors.
3. BM25 formula golden vectors against the Qdrant reference equation.
4. BM25 deterministic build and byte-for-byte reopen.
5. BM25 mutation invalidation and rebuild.
6. stale-artifact exact fallback and over-budget denial.
7. corrupted catalogue/artifact denial.
8. engine-only BM25 build/search across process reopen.
9. engine-only HNSW build/search across process reopen.
10. engine-only TurboQuant build/search and exact rerank across process reopen.
11. hybrid BM25+dense deterministic fusion across reopen.
12. transaction rollback proving no partial canonical data, catalogue, or
    projection visibility.

All scenarios must pass. A lower-crate test may supplement these gates but may
not satisfy an engine-cohesion row.

## Work exclusions until Q1–Q4 pass

- Connectome feature expansion or visual polish
- new MCP/CLI surface breadth unrelated to exposing a completed engine action
- blanket Fjall, Qdrant, or SurrealDB superiority benchmarks
- distributed, cloud, GPU, or maintenance feature expansion
- documentation claims that Arrow, DataFusion, BM25, HNSW, TurboQuant, or
  hybrid retrieval are cohesive before their engine-only exit gates pass
