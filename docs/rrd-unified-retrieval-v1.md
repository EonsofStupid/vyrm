# RRD unified retrieval algebra V1

Status: G04-W03 executable engine candidate, 2026-08-29.

This contract makes multimodal retrieval one bounded RRD engine operation. It
does not assemble independent vector, keyword, recommendation, and reranking
services in application middleware. `RrdEngine::execute_retrieval_query`
captures one authoritative read stamp, resolves one collection catalogue, and
executes every source, fusion, reranking, and result-shaping stage against that
snapshot.

The Rust contract is implemented by `ExecuteRetrievalQuery`, `RetrievalQuery`,
`RetrievalRerankStage`, and `RetrievalResultShape` in `rrd-contract`. Generated
HTTP, MCP, CLI, and language-SDK bindings remain G06 work and are not claimed by
this engine-only gate.

## Ownership and security

- RRD requires both `query_execute` and `vector_search`; neither permission
  implies the other.
- Every branch is bound to the request's collection. Stored example references
  name an exact vector identity, so points with multiple modalities are not
  ambiguous.
- The engine captures one `ReadStamp`, reads the collection catalogue once, and
  reduces canonical runtime changes once. Keyword and vector leaves cannot
  silently observe different commits.
- Every filter, score-boost property, group key, and facet key must name an
  active typed payload index in the collection catalogue.
- Conflicting active values for the same named vector or conflicting payloads
  for one subject fail closed instead of selecting an arbitrary version.

## Bounded recursive query model

A request separates three concerns:

1. a recursive query program that produces ranked candidates;
2. `limit`, `candidate_limit`, and `max_scanned_changes` work bounds; and
3. a point, group, facet, or matrix result shape.

The contract admits at most eight recursive levels, 128 query nodes, 16
prefetch branches, 256 recommendation/context examples, and 16 rerank stages.
Every prefetch limit is bounded by `candidate_limit`, and the product of query
nodes and candidate capacity must remain inside the vector-search change bound.
Unknown JSON fields, empty required inputs, non-finite vectors, dimensional
drift, invalid fixed-point factors, and amplification beyond these limits are
rejected before execution.

## Leaf sources

| Source | V1 semantics |
|---|---|
| `nearest` | Dense, sparse, or multi-dense query through the existing collection planner. Dense may select exact, HNSW, or TurboQuant according to the requested mode; final scores retain exact engine semantics. |
| `keyword` | Existing RRFlowQL/BM25 analysis and scoring at the same read stamp, restricted to subjects present in the collection. |
| `recommend` | Positive and optional negative raw/stored examples. `average_vector` constructs one compatible dense, sparse, or multi-dense query; `best_score` ranks by the best positive score penalized by the best negative score. |
| `discover` | A target plus positive/negative context pairs. Context membership establishes ordered zones; target similarity resolves candidates within a zone. |
| `context` | Positive/negative context pairs without a target. Candidates are ordered by their deterministic context zone and margin. |

Sparse values are first-class exact vectors in this algebra. V1 does not claim
a sparse inverted-index lifecycle or server-side SPLADE generation.

## Prefetch, fusion, and governed reranking

`RetrievalPrefetch` supplies a bounded candidate set to a parent stage. Prefetch
is recursive, so dense, sparse, keyword, recommendation, or previously fused
branches can participate in a multistage program without middleware.

V1 fusion is weighted reciprocal rank fusion (RRF). The request supplies one
positive fixed-point weight per branch and a bounded rank constant. Candidates
are merged by subject; ties resolve by canonical subject identity. The response
retains each stage ordinal, rank, and score contribution.

Reranking stages execute in declared order:

- `score_boost` applies bounded fixed-point addition and multiplication only to
  candidates matching a governed typed-payload filter;
- `exact` scores the candidate's selected named vector against a supplied query;
- `model` additionally requires the collection's exact embedding-model digest
  and candidate provenance. Multi-dense scoring uses directed ColBERT-style
  MaxSim;
- `mmr` performs deterministic greedy maximal marginal relevance over a named
  vector, balancing relevance and diversity with a bounded fixed-point factor.

This is a deliberately smaller formula language than Qdrant's general formula
query. DBSF, arbitrary arithmetic/geo expressions, and Matryoshka-specific
operators remain explicit future extensions rather than aliases for V1.

## Result shapes

- `points` returns the final ranked hits.
- `groups` groups the bounded final candidate universe by an indexed typed
  payload value and enforces both group and per-group limits.
- `facets` counts indexed typed payload values over that same bounded candidate
  universe. It is not an unbounded whole-collection aggregation.
- `matrix` selects a bounded ranked sample and scores every ordered non-self
  pair using one named vector. Cells are directed because multi-dense MaxSim is
  not generally symmetric.

Every hit can expose all matching named-vector references for the subject, so
dense text, sparse text, image, and late-interaction representations remain one
collection object instead of disconnected records.

## Evidence and deterministic replay

The result includes the read manifest, known-at cursor, a complete query-plan
SHA-256, and ordered stage evidence. Each stage records its kind, input and
output candidate counts, exact/approximate status, and an independent plan
digest. Final hits retain per-stage contribution evidence.

The executable engine scenario covers keyword+dense+sparse RRF; exact image
reranking; model-pinned multi-dense MaxSim; payload boosting; MMR; both
recommendation strategies; discovery; context; groups; facets; a directed
matrix; deny-by-default permission separation; database reopen; and exact
result replay after reopen.

## Honest remaining boundary

G04-W03 establishes one cohesive engine algebra and correctness/reopen
evidence. It does not establish Qdrant parity or production superiority. The
remaining vector foundation includes sparse ANN/inverted indexing, compact
graph and payload-index layouts, physical quantized memory tiers, physical GPU
execution, server/provider inference, outward G06 bindings, production fixed-
hardware scale evidence, and the broader formula/DBSF operators listed above.
The scalar/product/binary/TurboQuant engine lifecycle is now qualified by
G04-W04 rather than remaining part of this query-algebra gap.
