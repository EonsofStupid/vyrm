# Qdrant capability inventory and RRFlow disposition

**Baseline:** Qdrant `1.19.x`, the current documentation/API line on
2026-08-23. This inventory follows the SurrealDB inventory intentionally:
first the general database/runtime stack, then the specialist vector stack.
Individual endpoint fields are grouped into product capability families, but
no major data, search, persistence, security, SDK, operations, edge, inference,
distributed, or estate surface is intentionally omitted.

Status terms have the same strict meaning as
[`surrealdb-capability-inventory.md`](surrealdb-capability-inventory.md):
**Verified**, **Partial**, **Absent**, and **External**.

## 1. Product form and deployment

Sources: [Qdrant overview](https://qdrant.tech/documentation/overview/),
[local quickstart](https://qdrant.tech/documentation/quick-start/),
[Managed Cloud](https://qdrant.tech/documentation/cloud/),
[Hybrid Cloud](https://qdrant.tech/documentation/hybrid-cloud/), and
[Private Cloud](https://qdrant.tech/documentation/private-cloud/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Rust vector-search server with persistent local storage | **Partial** — vector/storage crates exist; no stable public vector server |
| Standalone binary and Docker image | **Absent** as a supported distribution |
| REST and gRPC service | **Absent** |
| Built-in local Web UI/dashboard | **Partial** — Connectome is richer diagnostically but not a collection administration dashboard |
| Distributed self-hosted cluster | **Partial/experimental** |
| Community Helm deployment | **Absent** |
| Managed Cloud clusters | **Absent** |
| Hybrid Cloud on customer Kubernetes with cloud management | **Absent** |
| Fully disconnected Private Cloud operator | **Absent** |
| Embedded offline Qdrant Edge | **Partial** — `rrflow-edge` provides a narrower mmap exact-search runtime |
| Consistent data API across self-hosted and cloud | **Absent** |
| AWS, GCP, Azure and customer-infrastructure placement | **Absent** |

## 2. Data model and collection lifecycle

Sources: [manage data](https://qdrant.tech/documentation/manage-data/),
[collections](https://qdrant.tech/documentation/manage-data/collections/),
[points](https://qdrant.tech/documentation/manage-data/points/), and
[payload](https://qdrant.tech/documentation/concepts/payload/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Collections as independently configured sets of points | **Verified in the engine** — revisioned named-vector and payload-index metadata over the shared point transaction log |
| Collection create, inspect, update, delete and list | **Engine operations verified**; generated HTTP/MCP/CLI/SDK bindings remain G06 |
| Atomic collection aliases for migrations/blue-green cutover | **Absent** |
| Points identified by 64-bit integer or UUID | **Partial** — typed runtime references, different identity contract |
| Point upsert, retrieve, delete, count and existence APIs | **Partial** — atomic batch upsert/retirement plus retrieve/scroll exist; count/existence and generated bindings remain |
| Batch and column-oriented point upload | **Partial** — atomic row-oriented batches, no optimized columnar ingestion contract |
| Point vector update/delete independent of payload | **Verified in the engine** through versioned put/retire mutations |
| Payload set/overwrite/delete/clear independent of vector | **Absent** public point operation |
| Arbitrary JSON payload objects and arrays | **Partial** — typed runtime properties are narrower |
| Payload selectors on read and query results | **Partial** |
| Dense vectors | **Verified locally** |
| Sparse vectors as first-class named values | **Verified locally** for exact search |
| Named multiple vectors with independent dimension/metric/config | **Verified in the collection engine** for dense, sparse, and multi-dense definitions |
| Multivectors with variable row count | **Verified locally** for exact storage/search |
| ColBERT-style MaxSim comparator | **Verified locally** for exact oracle |
| Vector datatypes such as float32, float16 and uint8 | **Partial** — authoritative f32 only; quantized artifacts do not change the point datatype |
| Cosine, dot, Euclidean and Manhattan metrics | **Verified locally** |
| Collection-specific WAL, optimizer, shard, strict-mode and quantization configuration | **Absent** public administration |
| Approximate point/vector counts and detailed collection status | **Absent** public service |

## 3. Query and exploration API

Sources: [similarity search](https://qdrant.tech/documentation/search/search/),
[exploration](https://qdrant.tech/documentation/search/explore/), and
[hybrid/multi-stage queries](https://qdrant.tech/documentation/search/hybrid-queries/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Unified Query API | **Verified in the engine, absent as a public API** — one bounded recursive `ExecuteRetrievalQuery` contract owns sources, prefetch, fusion, reranking, and result shapes; generated bindings remain G06 |
| Nearest-neighbor search by raw vector | **Verified locally** |
| Search by an existing point ID/vector | **Partial** — stored vector references are exact recommendation/discovery/context examples, but nearest-by-ID is not a dedicated operator |
| Exact full-scan search | **Verified locally** |
| Approximate HNSW search with query-time `ef` | **Verified locally** for dense vectors |
| `indexed_only` eventual/partial-result option | **Absent** |
| Score threshold, limit, offset and result vector/payload selectors | **Partial** — bounded limit/candidate limit and named-vector evidence exist; threshold, offset, and arbitrary selectors do not |
| Positive/negative-example recommendation queries | **Verified in the engine** for raw or exact stored vector references |
| Average-vector and best-score recommendation strategies | **Verified in the engine** for dense, sparse, and multi-dense values with strict shape checks |
| Discovery search using positive/negative context pairs plus target | **Verified in the engine** with deterministic zone-first, target-tie scoring |
| Context-only search that partitions vector space | **Verified in the engine** with deterministic zone/margin scoring |
| Scroll through all filtered points | **Verified in the engine** — deterministic point-reference pages at one read stamp |
| Order results by payload field | **Absent** vector API |
| Group search results by payload value | **Verified in the engine** over a bounded candidate universe and an active typed payload index |
| Cross-collection lookup for group/detail enrichment | **Absent** |
| Random sampling | **Absent** |
| Facet counts over payload values | **Verified in the engine** over a bounded candidate universe and an active typed payload index |
| Distance/similarity matrix sampling | **Verified in the engine** as bounded directed ordered pairs; directedness preserves asymmetric MaxSim semantics |
| Point count with exact/approximate modes and filters | **Absent** public API |
| Batch query endpoint | **Absent** |
| Nested prefetch queries | **Verified in the engine** with depth, node, branch, candidate, and total-work bounds |
| Multi-stage retrieval and rescoring | **Verified in the engine** for declared boost, exact, model-bound MaxSim, and MMR stages |
| Matryoshka coarse-to-fine retrieval | **Absent** |
| Reciprocal Rank Fusion (RRF) | **Verified in the engine** with one bounded fixed-point weight per branch |
| Distribution-Based Score Fusion (DBSF) | **Absent** |
| Formula queries combining scores, payload conditions and numeric/geo boosts | **Partial** — governed payload-conditioned fixed-point add/multiply exists; a general arithmetic/geo formula language does not |
| Dense+sparse hybrid search | **Verified in the engine** as one recursive query, including keyword branches |
| Multi-representation and branch-aware retrieval patterns | **Verified in the engine** for bounded named-vector/keyword prefetch and rerank programs |
| Search relevance tuning/evaluation helpers | **Partial** — deterministic recall gates, no product relevance suite |

## 4. Filtering and payload indexes

Sources: [filtering](https://qdrant.tech/documentation/search/filtering/) and
[indexing](https://qdrant.tech/documentation/manage-data/indexing/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Boolean `must`, `should`, `must_not` and minimum-should filters | **Partial** — typed expressions are narrower |
| Exact match, any-of and except matching | **Partial** |
| Numeric and datetime range filters | **Partial/absent** depending on runtime type |
| Full-text and phrase matching | **Absent** |
| Geo bounding-box, radius and polygon filters | **Absent** |
| Empty/null checks and array value-count conditions | **Absent** |
| Point-ID filters | **Absent** |
| Nested-object filters with same-element semantics | **Absent** |
| Filter by payload paths/array members | **Partial** |
| Keyword payload index | **Verified in the engine** for exact scalar filter governance |
| Integer/float/datetime payload indexes | **Partial** — signed/unsigned/decimal exact kinds exist; datetime does not |
| Boolean payload index | **Verified in the engine** |
| Geo payload index | **Absent** |
| Full-text payload index with tokenizer/configuration | **Absent** |
| UUID payload index | **Partial** — exact digest kind exists, not Qdrant UUID semantics |
| Tenant-aware/principal payload index | **Absent** |
| On-disk payload index variants | **Absent** |
| Cardinality estimation and planner selection between scan/index/HNSW | **Partial** — exact/HNSW cost crossover uses typed-filter eligibility; richer payload statistics remain |
| One-stage filterable HNSW with filter-aware graph edges | **Core verified** for in-traversal admission and exact rerank; payload-index-derived edges remain absent |
| ACORN filtered traversal for restrictive combined filters | **Absent** |

## 5. Vector indexing and optimization

Sources: [indexing](https://qdrant.tech/documentation/manage-data/indexing/),
[GPU indexing](https://qdrant.tech/documentation/ops-configuration/running-with-gpu/), and
[memory tiers](https://qdrant.tech/documentation/ops-configuration/memory-tiers/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Mutable/background-built dense HNSW | **Engine core verified** — immediate authoritative delta plus incremental immutable successor generations; automatic scheduler/merge policy remains |
| Per-collection and per-named-vector `m`, `ef_construct`, full-scan threshold | **Partial** programmatic config |
| Sparse inverted index | **Absent** — sparse path is exact scanning |
| Automatic segment optimization, merge and index thresholds | **Partial** in RRD LSM; vector segment optimization is absent |
| Optimizer status and indexing progress | **Partial** catalogue only |
| GPU-accelerated HNSW indexing | **Absent** physical backend |
| Vulkan GPU support across NVIDIA/AMD and selected devices | **Absent** |
| CPU SIMD scoring | **Verified locally** for compact exact and four-metric HNSW traversal with scalar differential |
| mmap/on-disk vector access | **Verified locally** for compact dense exact artifacts |
| Per-structure `pinned`, `cached`, and `cold` memory tiers | **Absent** |
| Independent tiers for original dense vectors, HNSW, quantized vectors, sparse index, payload and payload indexes | **Absent** |
| Startup cache warming and OS-evictable mmap tiers | **Absent** policy/control surface |
| On-disk vectors with in-memory graph | **Partial** — compact vector bytes mmap; HNSW artifact is not compact/on-disk production form |
| Strict-mode rejection of inefficient/unbounded queries and updates | **Partial** — resource contracts exist, not Qdrant-complete controls |
| Rate limits, filter-complexity, batch/result, timeout, index-count and storage caps | **Partial/mostly absent** |

## 6. Quantization

Source: [Qdrant quantization](https://qdrant.tech/documentation/manage-data/quantization/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Scalar int8 quantization (4x compression) | **Partial/experimental** — symmetric per-vector int8 only, not Qdrant-equivalent production format |
| Binary quantization | **Absent** |
| 1-bit, 1.5-bit and 2-bit encodings | **Absent** |
| Asymmetric binary-stored/scalar-query scoring | **Absent** |
| Product quantization | **Absent** |
| TurboQuant random rotation and global distribution-aware mapping | **Absent** |
| TurboQuant 4/2/1.5/1-bit modes (8x–32x) | **Absent** |
| TurboQuant asymmetric full-precision query scoring | **Absent** |
| SIMD TurboQuant scoring for cosine/dot/Euclidean | **Absent** |
| Per-vector quantization settings | **Absent** public config |
| Store quantized and original vectors together | **Absent** production lifecycle |
| Oversampling and exact rescoring controls | **Partial** — exact rerank exists, no quantized oversampling |
| Quantized-vector memory tier and inline storage controls | **Absent** |
| Recall/compression/speed validation guidance and benchmark matrix | **Absent** for TurboQuant; an exact oracle gate is specified only |

The existing `ScalarQuantizedVector` must not be renamed or presented as
TurboQuant. Qdrant 1.19 implements a materially different codec and lifecycle.

## 7. Persistence, mutation semantics, and storage lifecycle

Sources: [storage](https://qdrant.tech/documentation/storage/) and
[migration/recovery](https://qdrant.tech/documentation/migration-recovery-options/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Disk persistence for all collection data | **Verified locally** for canonical RRFlow state |
| Ordered WAL before segment application | **Verified locally** |
| Per-operation sequence/version to ignore stale reapplication | **Verified locally** through MVCC/idempotency, different API contract |
| Crash recovery from WAL | **Verified locally** |
| Mutable and immutable segment lifecycle | **Partial** — RRD LSM LSM segments, vector generations are separate |
| Background flush/index/optimization | **Partial** |
| Upsert idempotency | **Verified locally** for content/request identities |
| Synchronous `wait` option for update completion | **Absent** public API |
| Weak, medium and strong write ordering | **Absent** selectable API |
| Read consistency controls in replicated collections | **Absent** selectable API |
| Batched mutations | **Verified internally** |
| Atomic multi-operation point batches within Qdrant's supported batch contract | **Partial** — RRFlow atomic transaction is broader, but no point API parity |
| Collection aliases for online migration | **Absent** |
| Bulk upload with parallel batches and deferred indexing | **Absent** product path |
| Storage snapshots preserving prebuilt indexes | **Partial** — authenticated snapshots include RRFlow state/artifacts, not collection API |
| Full-storage and collection snapshots | **Partial** |
| Snapshot download/upload/restore API | **Absent** network API |
| Snapshot-based migration without reindexing | **Partial** in cluster artifact/snapshot transfer |
| Full-cluster failure recovery procedure | **Partial protocol evidence**, no product runbook/operator |

## 8. Sharding, replication, consistency, and multitenancy

Sources: [distributed deployment](https://qdrant.tech/documentation/scaling/distributed_deployment/),
[horizontal scaling](https://qdrant.tech/documentation/scaling/horizontal-scaling/),
[resilience](https://qdrant.tech/documentation/scaling/resilience/), and
[multitenancy](https://qdrant.tech/documentation/manage-data/multitenancy/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Collections split into shards | **Partial** — typed shard IDs/placement; no collection service |
| Replication factor per collection | **Partial protocol only** |
| Raft consensus for cluster topology/collection metadata | **Partial/experimental** |
| Configurable write/read consistency | **Absent** public contract |
| Automatic failover with adequate replicas/nodes | **Unqualified** |
| Manual shard move/replicate/abort/drop operations | **Absent** administration API |
| Record-stream shard transfer | **Partial** object/runtime transfer, not point streams |
| Snapshot shard transfer including HNSW/quantization artifacts | **Partial** authenticated artifact closure, different contract |
| WAL-delta shard recovery | **Partial** cluster log catch-up |
| Manual self-hosted shard balancing | **Absent** operator workflow |
| Automatic Cloud shard rebalancing | **Absent** |
| Online Cloud resharding up/down | **Absent** |
| User-defined shard keys | **Absent** public point API |
| Time-based sharding | **Absent** |
| Tenant isolation by payload partition | **Partial** via scopes, no payload planner/index |
| Tenant-dedicated shards | **Absent** public workflow |
| Tiered multitenancy promoting large tenants to dedicated shards | **Absent** |
| Tenant-specific sparse IDF statistics | **Absent** |
| Node failure detection and recovery | **Partial protocol tests** |
| Consensus checkpointing and metadata recovery | **Partial/verified adapter slice** |
| Multi-AZ replica placement | **Absent/unqualified** |

## 9. Inference, embeddings, and edge

Sources: [inference](https://qdrant.tech/documentation/inference/),
[Cloud inference](https://qdrant.tech/documentation/cloud/inference/),
[Qdrant Edge](https://qdrant.tech/documentation/edge/), and
[on-device embeddings](https://qdrant.tech/documentation/edge/edge-fastembed-embeddings/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Unified Inference API accepting documents/images in upsert and query | **Absent** public API |
| In-cluster sparse BM25 embedding | **Absent** |
| Managed dense, sparse and multimodal Cloud models | **External/absent adapter** |
| Proxy to OpenAI, Cohere, Jina AI and OpenRouter embeddings | **External/absent adapter** |
| Client-side FastEmbed inference | **Partial** — caller-supplied local FastEmbed backend |
| Text and image embedding | **Partial** — modality contract exists; verified production models are limited |
| Model identity/configuration associated with vector space | **Verified locally**, stronger digest binding |
| One-round-trip embed-and-query/upsert | **Absent** network surface |
| Qdrant Edge in-process, offline, no background service | **Partial** — `rrflow-edge` satisfies this form for a narrower path |
| Edge dense/sparse/multivector search | **Partial** — dense exact path only in packaged runtime |
| Edge BM25 | **Absent** |
| Edge synchronization patterns with server | **Absent** |
| On-device FastEmbed text/image models | **Partial** caller-managed local model files |

## 10. Security and compliance

Source: [Qdrant security](https://qdrant.tech/documentation/security/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Admin API key | **Absent** general client API |
| Read-only API key | **Absent** |
| JWT-based granular access keys | **Absent** |
| Per-collection read/write RBAC | **Absent** |
| Payload-filter-constrained access tokens | **Absent** |
| TLS for REST/gRPC | **Absent** endpoints |
| TLS for peer traffic | **Verified experimentally** with identity-bound cluster mTLS |
| Network bind controls | **Partial** |
| Cloud client-IP restrictions | **Absent** |
| JSON audit logging of authenticated/authorized API activity | **Partial** — runtime audit is not API-complete |
| Audit rotation and retention controls | **Absent** |
| Secure-by-default managed deployments | **Absent** |
| Strict-mode abuse/resource protection | **Partial** |
| Secret/configuration management for Kubernetes deployments | **Absent** |

## 11. APIs, SDKs, tools, and integrations

Sources: [API reference](https://api.qdrant.tech/),
[local quickstart](https://qdrant.tech/documentation/quick-start/), and
the [documentation index](https://qdrant.tech/documentation/).

| Qdrant capability | RRFlow disposition |
|---|---|
| OpenAPI-documented REST API | **Absent** |
| High-performance gRPC API | **Absent** |
| Official Python client | **Absent** |
| Official JavaScript/TypeScript client | **Absent** |
| Official Rust client | **Absent** supported SDK; internal crates do not qualify |
| Official Go client | **Absent** |
| Official .NET client | **Absent** |
| Official Java client | **Absent** |
| Async client operations | **Absent** public SDK |
| Embedded/local mode in Python client | **Absent** equivalent SDK packaging |
| FastEmbed library and retrieval/reranking helpers | **Partial** backend adapter only |
| Qdrant MCP server | **Partial** — RRFlow has its own narrower MCP server |
| Agent skills/documentation for AI tools | **Partial** runtime registry/hooks; no distributable skill package |
| Web UI collection console, API console and visual point explorer | **Partial** — Connectome focuses runtime traces/graph, not collection ops |
| Ecosystem integrations with orchestration/RAG frameworks | **Absent** supported matrix |

## 12. Observability and operations

Sources: [monitoring and telemetry](https://qdrant.tech/documentation/ops-monitoring/) and
[Managed Cloud](https://qdrant.tech/documentation/cloud/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Health, liveness and readiness endpoints | **Absent** database service endpoints |
| Service and collection telemetry endpoints | **Absent** public endpoint |
| Prometheus/OpenMetrics metrics | **Absent** exporter |
| Cluster/peer/shard status inspection | **Partial** — typed supervisor/Connectome history |
| Structured operational logs | **Partial** |
| Managed monitoring, logging and alerting | **Absent** |
| Grafana/Prometheus guidance | **Absent** product integration |
| Datadog integration for Hybrid/Private Cloud | **Absent** |
| Audit-log operations | **Absent** complete pipeline |
| Optimizer/indexing/segment status | **Partial** local evidence only |
| Configuration file/environment override surface | **Partial** crate/CLI-specific, not unified server config |
| Version upgrades and compatibility policy | **Partial** physical format compatibility, no product upgrade orchestrator |

## 13. Cloud, Kubernetes, and estate management

Sources: [Managed Cloud](https://qdrant.tech/documentation/cloud/),
[Hybrid architecture](https://qdrant.tech/documentation/hybrid-cloud/),
[Private Cloud](https://qdrant.tech/documentation/private-cloud/), and
[installation options](https://qdrant.tech/documentation/installation/).

| Qdrant capability | RRFlow disposition |
|---|---|
| Cloud account/organisation and central management console | **Absent** |
| Create/configure/delete database clusters | **Absent** |
| Horizontal and vertical scale up/down | **Absent** |
| Automatic shard rebalancing | **Absent** |
| Online shard splitting/resharding | **Absent** |
| Automated backups and disaster recovery | **Absent** |
| Zero-downtime HA upgrades | **Absent** |
| HA automatic failover | **Absent/unqualified** |
| Zone-aware Multi-AZ scheduling | **Absent** |
| Kubernetes Operator and CRDs | **Absent** |
| Helm-delivered control plane | **Absent** |
| Hybrid Cloud agent with outbound-only management/telemetry channel | **Absent** |
| Customer-network data residency with remote management | **Absent** |
| Fully disconnected Private Cloud | **Absent** |
| Central management API/UI | **Absent** — local `EstateView` is not a reconciler |
| Integrated recommendations, monitoring and alerting | **Absent** |
| CSI snapshot/restore integration | **Absent** |
| Enterprise support lifecycle | **Absent/not yet a product operation** |

## 14. Immediate conclusion

RRD's strongest overlap with Qdrant today is real but bounded: revisioned
collections and named vectors; atomic point batches and retirements; typed
payload-index lifecycle; exact dense/sparse/multivector values; online filtered
dense HNSW; exact reranking; model-bound provenance; immutable artifacts; mmap
dense search; WAL-backed persistence; snapshots; offline embedding/search; and
detailed causal runtime evidence. One bounded engine query now composes
nearest, keyword, recommendation, discovery, context, dense+sparse RRF,
multimodal reranking, model-pinned ColBERT MaxSim, MMR, groups, facets, and
directed matrices. It does **not** have Qdrant's full vector
product stack.

The critical missing blocks are generated collection/point bindings, richer
payload index families and statistics, ACORN/payload-derived graph edges,
DBSF/general formula operators, promoted quantizer lifecycles, compact
graph storage, memory tiers, physical GPU indexing, server inference, official
SDK qualification, REST/gRPC breadth, and production distribution.

Until those exist and pass fixed-corpus/fixed-hardware differentials, a RRFlow
versus Qdrant superiority claim would be false. The correct near-term claim is
that RRFlow owns an AI-reasoning-aware persistence and evidence kernel with some
verified vector primitives that Qdrant does not attempt to provide.
