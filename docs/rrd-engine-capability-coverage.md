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
| HNSW vector search with cosine, dot, Euclidean, Manhattan, SIMD, and governed GPU construction | Collection-bound HNSW v2 builds, incrementally advances, publishes, reopens, traverses, and exactly reranks all four metrics. Query traversal runtime-dispatches AVX2. One engine-owned optional GPU registry always builds the deterministic CPU oracle first, decodes untrusted adapter bytes, requires exact byte/descriptor parity plus deterministic semantic probes, and publishes the selected backend, fallback, differential, and resource evidence atomically with the artifact. Public CPU/prefer/require policy, failure/corrupt fallback, no-publication require failure, replay without adapter reinstall, restart, and search work accounting pass. | **Engine core qualified.** No production CUDA/ROCm/Metal/Vulkan adapter or fixed-hardware GPU latency/throughput certification is claimed; compact graph layout and distributed placement remain later physical work. | G04-W02, G04-W04, G04-W07 |
| Full-text indexing, configurable analysis, BM25, relevance, and highlighting | Content-addressed BM25 v2 artifacts support Unicode-alphanumeric or whitespace tokenization, case-sensitive or lowercase analysis, ASCII folding, normalized stop words, token bounds, deterministic English stemming, configurable `k1`/`b`, posting positions, UTF-8 byte offsets, matched-term evidence, and stable highlighting. `MATCH`, exact planner selection/fallback, and hybrid consumption share the same artifact. Fixed relevance/highlight corpora and public-engine tests cover the path. | **Query-engine foundation present.** Additional language-specific analyzers and distributed index placement belong to later product/cluster work, not a second text engine. | G03-W05, G08-W02 |
| Dense, sparse, and keyword hybrid fusion with RRF | `RrdEngine::execute_retrieval_query` executes bounded recursive BM25, dense, sparse, and multivector branches at one read stamp and applies weighted reciprocal-rank fusion with per-stage contribution evidence. | **Engine core present.** Native SPLADE generation, sparse inverted indexing, DBSF/general formula operators, and generated outward bindings remain open. | G04-W03, G06-W01 |
| Metadata filtering during HNSW traversal | HNSW accepts only active typed collection payload indexes, applies the bounded filter algebra to layer-zero eligibility while retaining non-matching navigation nodes, and reapplies it during exact reranking. Active artifacts protect their payload indexes from deletion. Nested algebra plus fixed selectivity, insertion, retirement, incremental-generation, and reopen corpora pass. | **Engine core qualified.** ACORN-style payload-derived graph edges and payload-bitmap storage remain later physical optimization rather than correctness gaps. | G04-W02, G04-W04 |
| Scalar, product, binary quantization and up-to-64x memory reduction | One authenticated build/list/activate/retire lifecycle owns immutable scalar, product 4×–64×, binary, and TurboQuant 4/2/1.5/1-bit artifacts. Checksummed owned/mmap reopen, corruption denial, update/rebuild/recovery, scalar/runtime-dispatched SIMD parity, exact reranking, and a fixed 512×64 bias/recall/byte/latency matrix pass. Product codebooks and total artifact bytes are reported separately from packed-code ratio. | **Engine core qualified.** The 64× result is packed vector-code compression, not a universal total-residency claim. Generated bindings, production fixed-hardware scale evidence, and distributed placement remain open; G04-W07 qualifies the shared GPU construction boundary for HNSW without claiming a quantization GPU kernel. | G04-W04, G04-W05, G04-W07, G06-W01 |
| Physical pinned, cached, and cold vector residency | `RrdEngine` owns one process-local manager. Per-named-vector pinned artifacts use hard-bounded retained admission, cached artifacts use byte-bounded deterministic LRU, and cold artifacts are transient verified mmap/owned loads. Metadata-first request filtering, active-generation reconciliation, tier transitions, cache bypass, pressure exact fallback/ResourceExhausted, and empty-cache restart equality pass. | **Engine core qualified.** Independent original/HNSW/quantized/sparse/payload tiers, automatic warming, compact mmap HNSW, distributed placement, generated controls, and production sizing remain open. | G04-W05, G06-W01, G08-W02 |
| Multiple vectors, multimodal retrieval, and ColBERT-style late interaction | Revisioned collections persist named dense, sparse, and multi-dense definitions. One recursive query composes named text/image/sparse/late representations; model-digest-pinned multi-dense reranking uses directed MaxSim, retains provenance, and replays identically after reopen. | **Engine retrieval core present.** Generated outward bindings and live provider adapter certification remain G06/deployment work. | G04-W01, G04-W03, G04-W06 |
| Native embedding and governed inference | One engine-owned revisioned runtime registry binds executable adapters to exact model digests, explicit local/offline or remote-provider trust boundaries, and hard per-input/batch/output limits. Public batch generation retains source/model/read/registry provenance. Atomic embed-and-search verifies the named-vector binding and uses the existing vector planner at one captured read stamp without committing a temporary query vector. FastEmbed performs one validated native batch call; deny-network, corrupt digest, duplicate model, shape, normalization, and resource violations fail before admission. | **Engine core qualified.** Transport-neutral contracts and authenticated Rust engine operations are public. Generated HTTP/MCP/CLI/SDK bindings remain G06; live provider endpoint and credential adapters require deployment certification. | G04-W06, G06-W01 |
| Score boosting, model reranking, and MMR diversity | Ordered governed stages provide typed-payload fixed-point boosts, exact named-vector reranking, exact model-digest-pinned reranking, and deterministic greedy MMR. | **Engine core present.** General formula/DBSF algebra, outward bindings, and production relevance/performance qualification remain open. | G04-W03, G06-W01 |
| Real-time CDC and live queries over WebSockets | One engine-owned subscription record binds an exact changefeed or RRFlowQL query to the global runtime cursor, durable cumulative ACK, retained resume floor, renewable lease, fenced connection generation, and bounded in-flight window. Authenticated WebSocket push and the supported Rust client replay unacknowledged data across reconnect/restart; interleaved writers retain cursor/hash-chain order. | **Foundation present.** Cross-region fan-out placement and managed gateway scaling remain topology work, not another subscription engine. | G03-W06, G08-W02 |
| Immediate, non-blocking real-time indexing | Authoritative commits never wait for query-index work. Scalar, geo, materialized-view, count, grouped-count, and BM25 generations retain reconciliation evidence. For HNSW, new puts/updates/retirements are immediately exact-overlaid on the active graph; unchanged configurations insert only post-generation versions into a new immutable generation while the prior graph serves, and publication records full-build/incremental provenance. | **Single-node engine core qualified.** Automatic scheduling/merge thresholds and distributed maintenance placement remain topology/operations work. | G03-W05, G04-W02, G08-W02 |
| Incremental materialized views and analytics | Filtered materialized views, global count artifacts, and grouped-count artifacts are durable content-addressed index kinds. Rebuilds compare the prior complete generation with the new stamped snapshot, publish inserted/updated/removed evidence, and never expose a partial generation. Public catalogue snapshots expose bounded analytics/maintenance summaries. | **Foundation present.** Distributed scheduling and shard-local merge policy remain later topology work. | G03-W05, G08-W02 |
| Embedded scripting and functions | One immutable automation catalogue owns versioned JavaScript ES2020 and portable WebAssembly JSON-v1 definitions, source/module digests, explicit limits and capabilities, and synchronous transaction triggers. QuickJS runs without clock, random, dynamic-code, module, filesystem, or network authority under heap/stack/interrupt bounds; Wasmi admits no imports or WASI and enforces memory/value-stack/call-stack/fuel bounds. Transactions pin the catalogue revision before committing, execute triggers against only the original typed mutations, atomically append allowed derived events, and replay the same revision after a crash window. | **Engine foundation present.** Generated HTTP/MCP/CLI/SDK bindings and broader namespaced function syntax remain G06 work; asynchronous/background functions and host I/O are deliberately outside v1. | G03-W07, G06-W01 |
| Multitenancy and partitioning | Runtime scopes, exact resource-prefix grants, compiled tenant/row predicates, field projections, estate identities, and shard types exist. RRFlowQL policy is injected before binding/planning and cross-tenant rows are excluded by executable tests. | **Partial.** Logical query isolation is present; physical partition placement and distributed cross-tenant fault qualification remain open. | G05-W03, G08-W01, G08-W02 |
| JWT/third-party auth, RBAC, row-level, and field-level permissions | One revisioned authority persists users/services/nodes, bounded acyclic inherited roles, API-key verifier revisions, RRD-issued bounded JWT issuer metadata, exact post-verification issuer/subject/audience bindings, and constrained grants. JWT and externally bound sessions share current-policy enforcement; rotation/revocation invalidates tokens and existing leases across reopen. Tenant/row predicates and allowed fields are compiled into RRFlowQL before planning and stamped in public plan evidence. Redacted audit records form an atomic-head semantic hash chain with protected read/JSONL export, concurrent-writer/reopen validation, and security/estate administration coverage. | **Security and structured-audit foundations present.** External OIDC signature/JWK verification remains an adapter responsibility; constrained non-query operations deliberately deny until their policy injectors exist, and distributed/certificate lifecycle qualification remains later work. | G05-W03, G05-W04, G08-W03 |
| MMap, io_uring, storage-compute separation, and low-RAM storage | Native V3 immutable segments use one policy across mmap, runtime-probed Linux io_uring, and bounded positional fallback. Process-local evidence separates physical operations/bytes/peak requests/fallbacks from decoded block-cache residency. S3 archive/artifact transfer is authenticated, resumable multipart, conditional, checksummed, retry-bounded, and range-streamed. Vector artifacts add hard-bounded retained residency plus transient verified mmap/owned cold access. | **Storage foundation qualified.** Automatic workload warming/promotion, compact mmap HNSW, distributed residency, and live provider endpoint certification remain deployment evidence. | G02-W01, G02-W06, G04-W04, G04-W05 |
| Distributed HA, automatic sharding, multi-region replication, and read replicas | Estate, shard/replica contracts, traced object transfer, reconciliation primitives, and Kubernetes resources exist. | **Partial foundation only.** Consensus-integrated data commits, independent-host failover, automatic sharding, multi-region replication, and HA qualification are missing. | G08-W01, G08-W02, G08-W03 |
| In-memory, embedded, daemon, edge, remote, and distributed deployment | One strict checked-in corpus passes the full logical engine in memory/native-embedded compositions, a real standalone local daemon plus Rust client, the remote mTLS client face, and the bounded mmap/offline edge retrieval subset. Native second-writer denial and clean daemon-reopen cursor evidence are executable. | **Deployment foundation present.** Browser/mobile packaging and consensus-backed distributed placement/failover remain open; edge deliberately proves only its supported offline retrieval contract. | G05-W01, G08-W02, G10-W03 |
| RRFlowQL, GraphQL, REST/HTTP, WebSocket, gRPC, and native SDKs | RRFlowQL, generated OpenAPI REST/HTTP, MCP, CLI, and the Rust engine/client boundary exist. | **Partial.** GraphQL, WebSocket, gRPC, and the promised TypeScript/Python/Go/Java/.NET SDK qualification do not. | G06-W01, G06-W04, G06-W05 |

## Evidence anchors

- Mixed-model commit and schema authority: `crates/rrd-core/src/runtime.rs`,
  `crates/rrd-core/src/schema.rs`, and
  `crates/rrd-server/tests/http_process.rs::data_transaction_atomically_commits_every_public_model_and_replays_after_restart`.
- Query, graph traversal, BM25, and unified retrieval: `crates/rrd-query/src`,
  `crates/rrd-engine/src/engine/query.rs`, and
  `crates/rrd-engine/src/engine/retrieval_query.rs`.
- Vector HNSW, one-stage filters, MaxSim, quantization, exact reranking, and SIMD:
  `crates/rrd-vector/src/hnsw.rs`, `crates/rrd-vector/src/exact.rs`,
  `crates/rrd-vector/src/turboquant.rs`, and `crates/rrd-vector/src/compact.rs`.
- CDC/live query: `crates/rrd-engine/src/engine/changefeed.rs`,
  `crates/rrd-engine/src/engine/query.rs`, and
  `crates/rrd-server/src/http/capabilities.rs`.
- Security and transport: `crates/rrd-security/src/lib.rs`,
  `crates/rrd-engine/src/engine/security.rs`,
  `crates/rrd-engine/src/engine/invocation.rs`,
  `crates/rrd-engine/src/engine/query.rs`, and
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
