# RRD engine capability coverage

Audited: 2026-08-29

This is a source-and-test audit, not a product-green declaration. `Core present`
means an executable engine path and focused test exist. `Partial` means a real
subset exists but the stated product capability is not closed. `Missing` means
there is no authoritative engine implementation matching the claim. A work-plan
item is not implementation evidence.

Audit total: **5 core-present, 14 partial, 1 missing**. `Core present` still
does not mean the full product promise is closed; every remaining qualifier in
the row is an acceptance obligation in `rrflow.workplan.toml`.

The requested checklist is frozen one-for-one under stable IDs. None may be
removed, merged away, or marked complete from a plan entry alone:

1. `CAP-01` multi-model core — G03-W01/G03-W02.
2. `CAP-02` graph engine — G03-W02/G03-W03/G06-W02.
3. `CAP-03` ACID transactions — G02-W01/G03-W02/G05-W02.
4. `CAP-04` HNSW metrics and SIMD — G04-W02/G04-W04.
5. `CAP-05` full-text search — G03-W05.
6. `CAP-06` hybrid search and RRF — G04-W03.
7. `CAP-07` in-traversal metadata filtering — G04-W02.
8. `CAP-08` scalar/product/binary quantization — G04-W04.
9. `CAP-09` multivector/multimodal/late interaction — G04-W01/G04-W03.
10. `CAP-10` boosting/model reranking/MMR — G04-W03.
11. `CAP-11` WebSocket CDC and live queries — G03-W06.
12. `CAP-12` immediate non-blocking indexing — G03-W05/G04-W02.
13. `CAP-13` materialized analytics — G03-W05.
14. `CAP-14` embedded scripting and functions — G03-W07.
15. `CAP-15` multitenancy and partitioning — G05-W03/G08-W01/G08-W02.
16. `CAP-16` JWT/OIDC/RBAC/row/field policy — G05-W03.
17. `CAP-17` mmap/io_uring/storage-compute separation — G02-W01/G02-W06/G04-W04/G04-W05.
18. `CAP-18` distributed HA and multi-region — G08-W01/G08-W02/G08-W03.
19. `CAP-19` embedded/edge/daemon/distributed deployment — G05-W01/G08-W02/G10-W03.
20. `CAP-20` RRFlowQL/GraphQL/HTTP/WebSocket/gRPC/SDKs — G06-W01/G06-W04/G06-W05.

The detailed evidence and remaining qualifiers for those exact IDs follow in
the same order.

| Required capability | Current engine evidence | Truth now | Closure authority |
|---|---|---|---|
| Native document, graph, relational, time-series, geo, key-value, strict-schema, and schemaless models | One revisioned `RuntimeSchemaRegistry` names its namespace/database and all logical models. One runtime commit can create/update/retire/recreate document, key-value, record, relation/native-edge, event, vector, series, geo, and object values; one exact-stamp snapshot reduces them under the same catalogue revision and authenticated cursor. Bounded RRFlowQL transaction programs bind that same complete mutation vocabulary. Mixed-model atomicity, historical reads, concurrent-writer fencing, and Memory/Fjall/native reopen parity are covered. | **Engine CRUD core present.** Dedicated HTTP/MCP administration surfaces remain open under G06; reasoning claims deliberately retain their separate bi-temporal correction contract. | G03-W01, G03-W02, G03-W03, G06-W02 |
| Graph engine with typed directed links and deep traversal | Typed `RuntimeRelation` values, bounded RRFlowQL `traverse:` execution, and exact same-stamp equi-joins run through one planner/executor. | **Core present.** Complete graph administration and richer path/join algebra remain open. | G03-W02, G03-W03, G06-W02 |
| Multi-row, multi-table ACID transactions | One `RuntimeCommit` atomically carries mixed mutation families under one global cursor. The server durably prepares a complete prospective all-model read-your-writes snapshot, commits, aborts, times out, and idempotently replays across restart. Commit-intent crash windows and a cancelled/lost data response followed by server restart converge through the same runtime identity without duplicate data. G03-W03 bounded `BEGIN; MUTATE $binding; ...; COMMIT|CANCEL` programs delegate to this coordinator. | **Transaction foundation present.** Same-cursor writers are fenced and lifecycle/data reopen is exact; lower-level disk-full and abrupt process-kill injection remain resilience qualification, not another transaction engine. | G02-W01, G03-W02, G03-W03, G05-W02 |
| HNSW vector search with cosine, dot, Euclidean, Manhattan, and SIMD | Public and internal metric enums cover all four metrics; persistent HNSW build/open/search and exact rerank exist. AVX2 is used by the compact dense scorer. | **Partial.** The functional metric paths exist; a complete HNSW/metric SIMD differential and production performance matrix does not. | G04-W02, G04-W04 |
| Full-text indexing, configurable analysis, BM25, relevance, and highlighting | Content-addressed BM25 v2 artifacts support Unicode-alphanumeric or whitespace tokenization, case-sensitive or lowercase analysis, ASCII folding, normalized stop words, token bounds, deterministic English stemming, configurable `k1`/`b`, posting positions, UTF-8 byte offsets, matched-term evidence, and stable highlighting. `MATCH`, exact planner selection/fallback, and hybrid consumption share the same artifact. Fixed relevance/highlight corpora and public-engine tests cover the path. | **Query-engine foundation present.** Additional language-specific analyzers and distributed index placement belong to later product/cluster work, not a second text engine. | G03-W05, G08-W02 |
| Dense, sparse, and keyword hybrid fusion with RRF | `RrdEngine::search_hybrid` executes BM25 and vector branches at one read stamp and applies weighted reciprocal-rank fusion. Sparse vector queries are executable. | **Core present.** Native SPLADE generation and the broader multistage query algebra remain open. | G04-W03 |
| Metadata filtering during HNSW traversal | HNSW validates indexed filter properties and passes visibility/filter admission into graph-layer search; a selective-filter test covers the crossover. | **Core present.** Fixed selectivity, update, and reopen qualification corpora remain open. | G04-W02 |
| Scalar, product, binary quantization and up-to-64x memory reduction | TurboQuant supports 4, 2, 1.5, and 1-bit packed artifacts with exact reranking; scalar quantization primitives also exist. | **Partial.** Product quantization and a separately governed binary-quantization lifecycle are missing, and no accepted universal 64x/recall claim exists. | G04-W04 |
| Multiple vectors, multimodal retrieval, and ColBERT-style late interaction | Collections support named dense, sparse, and multi-dense values; multi-dense search uses MaxSim. | **Core present.** End-to-end multimodal ingestion/model provenance and the complete late-interaction surface are not qualified. | G04-W01, G04-W03 |
| Score boosting, model reranking, and MMR diversity | HNSW/TurboQuant perform exact reranking, and hybrid search performs weighted RRF. | **Partial.** General boosting, model reranking, and MMR do not exist. | G04-W03 |
| Real-time CDC and live queries over WebSockets | One engine-owned subscription record binds an exact changefeed or RRFlowQL query to the global runtime cursor, durable cumulative ACK, retained resume floor, renewable lease, fenced connection generation, and bounded in-flight window. Authenticated WebSocket push and the supported Rust client replay unacknowledged data across reconnect/restart; interleaved writers retain cursor/hash-chain order. | **Foundation present.** Cross-region fan-out placement and managed gateway scaling remain topology work, not another subscription engine. | G03-W06, G08-W02 |
| Immediate, non-blocking real-time indexing | Authoritative commits never wait for query-index rebuilds. Every scalar, geo, materialized-view, count, grouped-count, and BM25 generation binds source/schema/valid-time coordinates; stale generations are excluded and exact authoritative execution remains available. Rebuild publication is atomic and records full-build or prior/new-cursor reconciliation evidence. | **Query-index boundary present.** Persistent HNSW/TurboQuant online maintenance remains G04-W02. | G03-W05, G04-W02 |
| Incremental materialized views and analytics | Filtered materialized views, global count artifacts, and grouped-count artifacts are durable content-addressed index kinds. Rebuilds compare the prior complete generation with the new stamped snapshot, publish inserted/updated/removed evidence, and never expose a partial generation. Public catalogue snapshots expose bounded analytics/maintenance summaries. | **Foundation present.** Distributed scheduling and shard-local merge policy remain later topology work. | G03-W05, G08-W02 |
| Embedded scripting and functions | No in-database JavaScript or equivalent governed function runtime is present. | **Missing.** | G03-W07 |
| Multitenancy and partitioning | Runtime scopes, resource-prefix grants, tenant filter properties, estate identities, and shard types exist. | **Partial.** Complete tenant isolation, partition placement, and cross-tenant fault tests are missing. | G05-W03, G08-W01, G08-W02 |
| JWT/third-party auth, RBAC, row-level, and field-level permissions | Persistent principals, API-key authentication, resource/action grants, revocation/expiry, session tokens, TLS 1.3, and mTLS exist. | **Partial.** JWT issuance, OIDC/third-party identity, role inheritance, and row/field policy are missing. | G05-W03 |
| MMap, io_uring, storage-compute separation, and low-RAM storage | Native V3 immutable segments use one policy across mmap, runtime-probed Linux io_uring, and bounded positional fallback. Process-local evidence separates physical operations/bytes/peak requests/fallbacks from decoded block-cache residency. S3 archive/artifact transfer is authenticated, resumable multipart, conditional, checksummed, retry-bounded, and range-streamed. | **Storage foundation qualified.** Vector-specific hot/cold placement, compression residency, and workload promotion remain G04 work; live provider endpoint certification remains deployment evidence. | G02-W01, G02-W06, G04-W04, G04-W05 |
| Distributed HA, automatic sharding, multi-region replication, and read replicas | Estate, shard/replica contracts, traced object transfer, reconciliation primitives, and Kubernetes resources exist. | **Partial foundation only.** Consensus-integrated data commits, independent-host failover, automatic sharding, multi-region replication, and HA qualification are missing. | G08-W01, G08-W02, G08-W03 |
| In-memory, embedded, daemon, edge, remote, and distributed deployment | One strict checked-in corpus passes the full logical engine in memory/native-embedded compositions, a real standalone local daemon plus Rust client, the remote mTLS client face, and the bounded mmap/offline edge retrieval subset. Native second-writer denial and clean daemon-reopen cursor evidence are executable. | **Deployment foundation present.** Browser/mobile packaging and consensus-backed distributed placement/failover remain open; edge deliberately proves only its supported offline retrieval contract. | G05-W01, G08-W02, G10-W03 |
| RRFlowQL, GraphQL, REST/HTTP, WebSocket, gRPC, and native SDKs | RRFlowQL, generated OpenAPI REST/HTTP, MCP, CLI, and the Rust engine/client boundary exist. | **Partial.** GraphQL, WebSocket, gRPC, and the promised TypeScript/Python/Go/Java/.NET SDK qualification do not. | G06-W01, G06-W04, G06-W05 |

## Evidence anchors

- Mixed-model commit and schema authority: `crates/rrd-core/src/runtime.rs`,
  `crates/rrd-core/src/schema.rs`, and
  `crates/rrd-server/tests/http_process.rs::data_transaction_atomically_commits_every_public_model_and_replays_after_restart`.
- Query, graph traversal, BM25, and hybrid fusion: `crates/rrd-query/src`,
  `crates/rrd-engine/src/engine/query.rs`, and
  `crates/rrd-engine/src/engine/retrieval.rs`.
- Vector HNSW, one-stage filters, MaxSim, quantization, exact reranking, and SIMD:
  `crates/rrd-vector/src/hnsw.rs`, `crates/rrd-vector/src/exact.rs`,
  `crates/rrd-vector/src/turboquant.rs`, and `crates/rrd-vector/src/compact.rs`.
- CDC/live query: `crates/rrd-engine/src/engine/changefeed.rs`,
  `crates/rrd-engine/src/engine/query.rs`, and
  `crates/rrd-server/src/http/capabilities.rs`.
- Security and transport: `crates/rrd-security/src/lib.rs` and
  `crates/rrd-server/src/http/server.rs`.
- Native persistence, edge, cluster, and estate: `crates/rrd-lsm`,
  `crates/rrd-store/src/native.rs`, `crates/rrflow-edge`,
  `crates/rrd-cluster`, `crates/rrd-estate`, and `crates/rrd-kubernetes`.

The checked-in `rrflow.workplan.toml` is the closure authority. The table must
be regenerated from executable capability dispositions before firm-alpha; this
manual audit is an interim correction, not a second catalogue.

## Native storage comparison

Fresh pinned evidence from 2026-08-26 establishes bounded promotion wins over
Fjall 3.1.8; it does not establish universal backend superiority.

- The 9-trial standard workload passes its correctness-and-promotion gate.
  Native/Fjall ratios are 1.243 for write throughput, 1.701 for read throughput,
  0.836 for write p95 latency, 0.616 for read p95 latency, 0.168 for clean-reopen
  recovery time, 0.921 for peak RSS, and 0.915 for clean-reopen allocated bytes.
  Lower latency, recovery, memory, and footprint ratios are better.
- The 5-trial embedding metadata-fanout workload also passes: 1.716 for resolved
  item throughput, 0.536 for p95 request latency, and 0.963 for clean-reopen
  allocated bytes.
- Earlier read-heavy evidence contains a losing native write-p95 cell (1.357).
  That result remains part of the record and prevents a claim that Fjall is
  categorically weaker across every workload and metric.

The full configurations, per-trial observations, footprint contracts, and
promotion decisions are in
`eval/results/2026-08-26-rrd-lsm-standard-current.json` and
`eval/results/2026-08-26-rrd-lsm-ai-metadata-fanout-current.json`.
