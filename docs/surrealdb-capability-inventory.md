# SurrealDB capability inventory and RRFlow disposition

**Baseline:** SurrealDB `3.2.4`, the latest stable release on 2026-08-23.
SurrealDB `3.3.0-beta.3` additions are isolated in the preview section and are
not treated as stable. This is a product-capability inventory: individual
SurrealQL functions and every CLI/configuration flag are grouped under their
own capability families rather than misrepresented as separate products.

This document exists because RRFlow cannot make a meaningful competitive or
optimization claim against a database whose complete product surface has not
first been enumerated. The old `3.0.5` differential remains a bounded engine
diagnostic only.

## Status language

| Status | Meaning in this repository |
|---|---|
| **Verified** | Executable RRFlow code and a retained test/evidence path exist. |
| **Partial** | A real subset exists, but it does not provide SurrealDB's complete capability. |
| **Absent** | No executable RRFlow product capability exists. |
| **External** | Intentionally supplied by another RRFlow component or provider; the adapter/contract must still exist. |

Maturity is deliberately conservative. A Rust type, a UI label, or a design
document alone is not implementation.

## 1. Product form, tenancy, and deployment

Sources: [deployment models](https://surrealdb.com/docs/build/deployment),
[what SurrealDB is](https://surrealdb.com/docs/what-is-surrealdb), and
[Cloud scaling](https://surrealdb.com/docs/manage/cloud/scaling).

| SurrealDB capability | Edition/state | RRFlow disposition |
|---|---|---|
| One Rust database engine exposed as a server and embeddable library | Community | **Partial** — RRFlow has libraries and several binaries, but not one stable public database distribution/API. |
| In-memory embedded engine | Community | **Verified** — `MemoryEngine`; test/reference use, not a separately versioned SDK product. |
| Browser/WASM embedded engine using IndexedDB | Community | **Absent** |
| Native embedded engines for Rust, Node.js, and WASM clients | Community | **Absent** as supported SDK distributions |
| Offline and edge deployment | Community | **Partial** — `rrflow-edge` is executable and offline, but its database/query surface is narrow. |
| Standalone single-node persistent server | Community | **Partial** — native persistence exists; `rrflow-mcp` is an MCP runtime rather than a full network database server. |
| Docker deployment | Community | **Absent** as a supported, tested product artifact |
| Self-hosted Kubernetes deployment | Community/Enterprise | **Absent** |
| Distributed multi-node database | Enterprise (`SurrealDS`); TiKV development path | **Partial/experimental** — real Raft adapter and TLS process tests exist; production service, independent-host and operational qualification do not. |
| Managed Cloud database | Commercial | **Absent** |
| Vertical and horizontal instance scaling | Cloud/Enterprise | **Absent** |
| High-availability managed Scale clusters | Cloud/Enterprise | **Absent** |
| Namespace and database hierarchy for multi-tenancy | Community | **Partial** — scopes and isolated instance manifests exist; full namespace/database administration does not. |
| Organisation, users, projects/instances, regions and plans as managed estates | Cloud | **Partial UI model only** — Connectome has local `EstateView`/`InstanceView`; no authoritative control plane provisions or reconciles estates. |
| One query/API contract across embedded, server, and distributed modes | Product-wide | **Absent** — RRFlow ports share semantics internally, but no stable multi-language/network contract spans every mode. |

## 2. Persistence, transactions, and lifecycle

Sources: [deployment/storage matrix](https://surrealdb.com/docs/build/deployment),
[3.2 release](https://surrealdb.com/releases/3.2), and
[self-hosted backup guidance](https://surrealdb.com/docs/manage/self-hosted/backups-and-recovery).

| SurrealDB capability | RRFlow disposition |
|---|---|
| Multi-record and multi-table ACID transactions | **Verified locally** for one canonical runtime transaction across RRFlow value families |
| Explicit `BEGIN` / `COMMIT` / `CANCEL` transactions | **Partial** — bounded RRFlowQL transaction programs execute typed mutation bindings through the canonical engine coordinator; public network/session adapters remain open |
| Client-owned transactions over RPC/SDKs | **Absent** |
| Read-your-writes transaction views | **Verified locally** |
| Optimistic conflict detection and atomic rollback | **Verified locally** |
| Durable WAL with corruption/torn-tail recovery | **Verified locally** in RRD LSM |
| MVCC historical versions and repeatable stamped reads | **Verified locally** |
| Versioned/temporal reads where supported by the storage engine | **Verified locally** with explicit valid/known time; wider RRFlowQL coverage is partial |
| Persisted snapshot leases and retention pins | **Verified locally** |
| Named physical checkpoints/snapshots | **Verified locally** |
| Authenticated physical snapshot bundle export/install | **Verified locally** |
| Background compaction and garbage collection | **Verified locally**, but production scheduling/tuning remains partial |
| Resumable datastore migrations and on-disk version history | **Partial** — manifest/codec compatibility and Fjall migration exist; general schema/data migration ledger does not |
| Memory, SurrealKV, RocksDB, IndexedDB, TiKV, and SurrealDS backend choices | **External/absent** — RRFlow has Memory, RRD LSM, and Fjall compatibility, not those backend products |
| Durable RPC sessions with TTL cleanup | **Absent** |
| Bounded abandoned client transactions | **Absent** because public client transactions are absent |
| Logical export/import of namespaces, schema, users, indexes and data | **Absent** as a complete portable database archive |
| Managed scheduled and on-demand backups with configurable retention | **Absent** |
| Managed restore into a new instance | **Absent** |
| Disaster-recovery runbooks and restore verification | **Partial** — engine crash matrices exist; product/estate recovery is absent |

## 3. Native data models and value families

Sources: [data models](https://surrealdb.com/docs/learn/data-models),
[geospatial model](https://surrealdb.com/docs/learn/data-models/geospatial/overview),
and [files](https://surrealdb.com/docs/learn/schema-management/files/working-with-files).

| SurrealDB capability | RRFlow disposition |
|---|---|
| Schemaless JSON-like documents with nested objects and arrays | **Partial** — typed runtime properties exist; general document CRUD/query semantics do not |
| Schemafull, schemaless, and mixed/flexible tables | **Partial** — strict revisioned runtime schema exists; no general schemaless table engine |
| Relational records and record links | **Partial** |
| Directed, typed graph relations whose edges can carry properties | **Verified locally** |
| Direct record links distinct from graph-edge records | **Partial** |
| Multi-hop and recursive graph traversal | **Partial** — routing/graph traversal exists; general query algebra is absent |
| Time-series values and time-oriented queries | **Partial** — series samples are transactional; indexes/window query surface is absent |
| Geospatial geometry/geography values, predicates, distances and indexes | **Partial** — WGS84 values exist; geometry families and spatial indexes/query functions are absent |
| Dense vector values | **Verified locally** |
| Sparse vector values | **Verified locally** for exact storage/search |
| Multiple named vectors per logical object | **Partial** — values can coexist; collection-style named-vector administration is absent |
| Multivectors | **Verified locally** for exact storage/oracle |
| Full-text documents and analyzers | **Absent** |
| Hybrid full-text/vector retrieval and rank fusion | **Absent** as one query operator |
| Key-value access patterns | **Verified internally**; not a supported public KV API |
| First-class file pointers and buckets | **Partial** — content-addressed object references/stores exist; file namespace and full operations do not |
| Memory/filesystem bucket backends | **Partial** — local immutable objects only |
| S3-compatible bucket backend | **Partial** — semantic adapter exists; no certified transport/deployment |
| Broad scalar types: null/none, booleans, numbers/decimal, strings, bytes, UUIDs, datetimes, durations, ranges, regex, records, geometry and unions/options | **Partial** — runtime values are deliberately narrower |

## 4. Schema and database-defined behavior

Source: [schema management](https://surrealdb.com/docs/learn/schema-management) and
the [complete statement catalogue](https://surrealdb.com/docs/reference/query-language/language-primitives/statements).

| SurrealDB capability | RRFlow disposition |
|---|---|
| Define/alter/remove namespaces and databases | **Absent** |
| Define/alter/remove tables and views | **Partial** — revisioned runtime record types, no general DDL/view surface |
| Define/alter/remove fields, types, defaults, assertions, computed and readonly fields | **Partial** — field type/required rules only |
| Typed record IDs and table-specific IDs | **Partial** |
| Define/alter/remove indexes | **Partial** — programmatic vector artifact catalogue only |
| Unique, compound, count and ordinary indexes | **Absent** as general indexes |
| Full-text analyzers, tokenizers and filters | **Absent** |
| Database events/triggers on mutations | **Partial** — typed runtime events and package hooks exist; user-defined transactional triggers do not |
| User-defined SurrealQL functions and closures | **Absent** |
| Parameters/constants | **Partial** — query parameters exist; persisted database parameters do not |
| Sequences | **Absent** as a public schema primitive |
| Users and access methods as schema objects | **Absent** |
| Custom HTTP APIs defined in the database | **Absent** |
| File buckets as schema objects | **Absent** |
| Incrementally maintained table views | **Absent** |
| Concurrent/resumable index creation and rebuild | **Absent** |
| Schema migration tooling (`SurrealKit`) | **Absent** equivalent product workflow |

## 5. Query languages and execution

Sources: [SurrealQL reference](https://surrealdb.com/docs/reference/query-language),
[querying overview](https://surrealdb.com/docs/learn/querying), and
[clauses](https://surrealdb.com/docs/reference/query-language/clauses/overview).

| SurrealDB capability | RRFlow disposition |
|---|---|
| General CRUD: create, insert, upsert, select, update, merge, patch and delete | **Partial** — the internal RRFlowQL transaction program can commit the complete typed create/update/retire mutation vocabulary; direct statement expressions and public adapters remain open |
| Graph relation creation and deletion (`RELATE`) | **Partial** — typed API, no public RRFlowQL mutation statement |
| Multi-statement transactional scripts | **Partial** — bounded `BEGIN; MUTATE $binding; ...; COMMIT|CANCEL` programs are exact; control flow and general statement expressions are absent |
| SQL-like projection, filtering, ordering, grouping, pagination, split, omit and fetch | **Partial** — explicit sources, filters and projection only |
| Joins through record links and graph idioms | **Partial** — exact same-stamp typed equi-joins and bounded graph traversal exist; richer join/path algebra is absent |
| Subqueries and composable expressions | **Absent** |
| Aggregates, math, statistics and vector functions | **Partial** — scoring primitives, not a database function library |
| Control flow, blocks, `IF`, `FOR`, `LET`, `RETURN`, `THROW`, sleep and futures | **Absent** |
| Embedded JavaScript/ES2020 scripting | **Absent** |
| Query planner with index selection | **Partial** — content-addressed read-only plans and vector access-path decisions |
| `EXPLAIN` and `EXPLAIN ANALYZE` | **Partial** — plan explanation/work evidence, no general analyzer |
| Streaming/batched query executor | **Partial** — bounded result batches; no end-to-end network stream |
| Resource/time/write-fanout query bounds | **Partial** — read budgets exist; full server safeguards do not |
| SurrealQL | **Absent** by design; RRFlowQL is not compatibility syntax |
| GraphQL with schema generation, queries, mutations/subscriptions and pagination | **Absent** |
| ISO GQL/OpenGQL graph language | **Absent** |
| Runtime evaluation of nested SurrealQL/GQL strings under capability gates | **Absent** |

## 6. Indexing and retrieval

Sources: [hybrid search](https://surrealdb.com/docs/learn/data-models/vector-search/hybrid-search),
[3.1 release](https://surrealdb.com/releases/3.1), and
[3.2 release](https://surrealdb.com/releases/3.2).

| SurrealDB capability | RRFlow disposition |
|---|---|
| Exact/brute-force KNN | **Verified locally** for dense, sparse and multivector |
| In-memory HNSW ANN | **Verified locally** for deterministic dense search only |
| Concurrent HNSW writes/build behavior | **Partial** — generation rebuild/publish, not equivalent concurrent mutation |
| On-disk DiskANN | **Absent** |
| Vector distances/similarities including cosine, dot, Euclidean and Manhattan | **Verified locally** |
| Exact reranking after ANN | **Verified locally** |
| Metadata-filter-aware ANN | **Partial** — filter-aware candidate admission, not Surreal's indexed prefilter stack |
| Full-text inverted indexes and BM25 scoring | **Absent** |
| Configurable analyzers/tokenizers/filters, highlighting and score functions | **Absent** |
| Reciprocal-rank-fusion hybrid search | **Absent** |
| B-tree/unique/compound/count indexes | **Absent** |
| Geospatial indexes | **Absent** |
| Query-time index hints and forced iterator selection | **Absent** |
| Index build/rebuild lifecycle, progress, interruption recovery and quarantine | **Partial** only for immutable vector generations |

## 7. Realtime, change history, and automation

Sources: [live queries](https://surrealdb.com/docs/learn/querying/real-time/live-queries) and
[changefeeds](https://surrealdb.com/docs/learn/querying/real-time/changefeeds).

| SurrealDB capability | RRFlow disposition |
|---|---|
| Push-based `LIVE SELECT` subscriptions | **Absent** |
| Full-record live notifications | **Absent** |
| JSON Patch-style `DIFF` notifications | **Absent** |
| Filtered live subscriptions and explicit kill | **Absent** |
| Notifications only for committed transactions | **Not applicable until subscriptions exist** |
| Managed SDK resubscription/reconnect | **Absent** |
| Durable changefeeds with configured retention | **Partial** — immutable cursor log exists; retention controls are not a public feed contract |
| Replay with `SHOW CHANGES` from time/versionstamp | **Partial** — resumable cursor pages and temporal UI exist |
| User-defined database events/triggers | **Absent** |
| Custom API endpoints as a controlled alternative to arbitrary queries | **Absent** |

## 8. Security, identity, and governance

Sources: [security](https://surrealdb.com/docs/learn/security),
[security summary](https://surrealdb.com/docs/learn/security/authentication/summary),
[row/field security](https://surrealdb.com/docs/learn/security/authorization/permissions-and-row-level-security),
and [capability gates](https://surrealdb.com/docs/learn/security/authorization/capabilities).

| SurrealDB capability | RRFlow disposition |
|---|---|
| Root-, namespace-, and database-level system users | **Absent** |
| Owner/editor/viewer RBAC for system users | **Absent** |
| Record users with custom signup/signin logic | **Absent** |
| JWT authentication, third-party JWT and JWKS | **Absent** |
| Bearer access grants, refresh tokens, revoke and purge | **Absent** |
| Password/passhash handling with Argon2id | **Absent** |
| Session and token expiration | **Absent** |
| Deny-by-default table permissions | **Absent** as database user policy |
| Per-row and per-field create/select/update/delete policy | **Absent** |
| Guest access constrained by data permissions | **Absent** |
| Server capability allow/deny gates for scripting, networking, functions, routes, files, experimental features and arbitrary queries | **Partial** — deny-by-default runtime/tool policies exist, not full database capabilities |
| Outbound-network target allowlists and SSRF defenses | **Partial** in embedding/runtime adapters; absent server-wide |
| TLS for client endpoints | **Partial** — cluster mTLS exists; no general client API endpoint |
| Network binding and exposure controls | **Partial** — Connectome remote opt-in only |
| Structured audit of statements, queries, transactions, RPC, auth, sessions and HTTP | **Partial** — hash-chained accepted runtime operations, not comprehensive API audit |
| Durable NDJSON audit sink, rotation, redaction, fsync and tamper-evident hash chaining | **Partial** — chaining exists; sink/rotation/redaction/completeness do not |
| Slow-query logging and sensitive parameter controls | **Absent** |

## 9. APIs, protocols, SDKs, and integrations

Sources: [RPC protocol](https://surrealdb.com/docs/reference/rest-api/rpc-protocol),
[SDK list](https://surrealdb.com/docs/learn/querying/surrealql/executing-queries/via-sdks),
and [3.1 MCP release](https://surrealdb.com/releases/3.1).

| SurrealDB capability | RRFlow disposition |
|---|---|
| HTTP SQL endpoint | **Absent** |
| REST record endpoints | **Absent** |
| JSON-RPC over HTTP | **Absent** |
| Bidirectional RPC over WebSocket | **Absent** |
| Typed CBOR and FlatBuffers wire encodings | **Absent** |
| GraphQL HTTP/RPC surface | **Absent** |
| GQL HTTP/RPC surface | **Absent** |
| First-party MCP over stdio and authenticated HTTP | **Partial** — stdio MCP lifecycle/query tools exist; authenticated HTTP and full CRUD/schema surface do not |
| Stateless MCP 2026-07-28 plus legacy handshake revisions | **Verified locally** for RRFlow's narrower tool surface |
| Official Go SDK | **Absent** |
| Official Java SDK | **Absent** |
| Official JavaScript/TypeScript/Node SDK | **Absent** |
| Official Kotlin SDK | **Absent** |
| Official Mojo SDK | **Absent** |
| Official .NET SDK | **Absent** |
| Official PHP SDK | **Absent** |
| Official Python SDK | **Absent** |
| Official Rust SDK with stable public client API | **Absent** — workspace crates are internal alpha libraries, not a supported SDK |
| Official Swift SDK | **Absent** |
| WebAssembly/browser client and embedded engine | **Absent** |
| Framework integrations such as React/TanStack Query and LangChain | **Absent** |

## 10. Extensions, files, ML, and AI surfaces

Sources: [extensions](https://surrealdb.com/docs/learn/extensions),
[file functions](https://surrealdb.com/docs/reference/query-language/functions/database-functions/file),
and the [documentation index](https://surrealdb.com/docs).

| SurrealDB capability | RRFlow disposition |
|---|---|
| Sandboxed WASM extension system (`Surrealism`) | **Absent** |
| Custom modules and namespaced functions | **Absent** |
| Asynchronous/write-capable extension functions | **Absent** |
| File put/get/head/list/copy/rename/delete and conditional variants | **Partial** — immutable put/get/verify/list/delete semantics only |
| In-database ML model import/use (`SurrealML`) | **Absent** |
| Local/cloud embedding pipeline integrated with data writes | **Partial** — governed local embedding exists, not cluster inference |
| AI-agent MCP query/data/schema tooling | **Partial** |
| Agent memory product (`Spectron`) | **External/absent** — reasoning/claim memory is RRFlow-native but not equivalent product packaging |

## 11. Operations, observability, UI, and estates

Sources: [logging](https://surrealdb.com/docs/manage/observability/logging),
[metrics](https://surrealdb.com/docs/manage/observability/metrics),
[audit logging](https://surrealdb.com/docs/manage/observability/audit-logging), and
[Cloud backups](https://surrealdb.com/docs/manage/cloud/backups-and-recovery).

| SurrealDB capability | RRFlow disposition |
|---|---|
| CLI start/stop/query/import/export/version/health and administration workflows | **Partial** — RRFlow CLI/runtime commands, not full database administration |
| Dedicated liveness/readiness endpoints | **Absent** as network service probes |
| Text and JSON structured logs | **Partial** |
| File/socket log sinks and rotation | **Absent** |
| Prometheus/OpenMetrics metrics | **Absent** exporter |
| OpenTelemetry traces, metrics and logs | **Partial** — typed internal tracing exists; OTLP translation/export is absent |
| Query/statement/transaction/storage/backend metrics | **Partial** — strong local storage evidence, no production metric endpoint |
| GraphQL and MCP operation metrics | **Absent** |
| Distributed consensus/replica/recovery metrics | **Partial** — typed snapshots/counters, no exporter/retention/alerts service |
| Slow-query telemetry | **Absent** |
| Audit-pipeline health metrics | **Absent** |
| Web UI for connecting to and exploring local/remote databases | **Partial** — Connectome is a local developer workbench, not full administration |
| Table/record editor | **Absent** |
| Query editor and result inspection | **Partial** — read-only Query Lab |
| Schema exploration and management | **Partial** — read-only schema lens |
| Graph visualization | **Verified locally** for runtime graph |
| Temporal playback, freeze, rewind and differential visualization | **Verified locally** and broader than SurrealDB Studio's database view |
| Cloud organisation/user/instance lifecycle | **Absent** |
| Instance create/configure/resize/pause/delete/upgrade | **Absent** |
| Network/VPC/IP restriction management | **Absent** |
| Backup retention/restore management | **Absent** |
| Usage/billing/support/marketplace integration | **Absent** |

## 12. Distributed consistency and high availability

Source: [distributed deployment](https://surrealdb.com/docs/build/deployment) and
the [3.2 stable release](https://surrealdb.com/releases/3.2).

| SurrealDB capability | RRFlow disposition |
|---|---|
| Horizontally scaled query nodes over shared distributed storage | **Absent** |
| Replication, consensus and fault tolerance | **Partial/experimental** |
| Distributed ACID transactions | **Absent** — RRFlow Raft serializes one canonical state domain; this is not a general distributed transaction service |
| Cluster membership changes and placement epochs | **Verified protocol slice**, not production operations |
| Replica snapshot catch-up and WAL-delta recovery | **Verified protocol slice** |
| Immutable object closure transfer before state activation | **Verified locally** |
| Admission control and bounded request/write sets | **Partial** |
| Multi-AZ placement and failure survival | **Absent/unqualified** |
| Multi-region operation | **Absent** |
| Independent compute/storage scaling | **Absent** |
| Object-storage-backed distributed persistence | **Absent** |
| Rolling upgrade automation and version-skew policy | **Absent** |
| Automated failover, reconciliation and estate health management | **Absent** beyond deterministic tests |

## 13. SurrealDB 3.3 preview — not part of the stable baseline

Source: [SurrealDB 3.3 beta release notes](https://surrealdb.com/releases/3.3).

These capabilities must be tracked so RRFlow does not build against an already
obsolete target, but they are marked preview until SurrealDB ships stable 3.3:

- Postgres wire protocol with simple/extended query flows, TLS, SCRAM-SHA-256,
  prepared parameters, cancellation, and interactive transactions.
- ISO GQL enabled by default.
- Native gRPC engine and end-to-end streamed results.
- WebSocket and embedded JS query streaming with bounded backpressure.
- Open-source S3/S3-compatible, GCS, and Azure bucket storage.
- Bitmap fusion for indexed `AND`/`OR`/`NOT`, index-only counts, and
  pre-filtered HNSW/DiskANN traversal.
- Datastore semantic-version ledger and resumable startup migrations.
- Runtime-swappable capability policy for embedders.
- S3-backed SurrealDS storage plus bounded recovery/admission improvements.

RRFlow currently has no Postgres wire, gRPC query server, bitmap payload-index
fusion, or cloud-backed distributed persistence. Its typed query batches,
capability gates, and S3 semantic port are prerequisites, not equivalents.

## 14. Immediate conclusion

RRFlow has a substantive local runtime kernel: authenticated persistence,
multi-family atomic commits, bitemporal reads, typed graph/history, exact and
dense-HNSW vector paths, snapshots, object closure, reasoning enforcement,
cluster protocol evidence, and unusually rich temporal diagnostics. It does
**not** yet have the SurrealDB full stack. The largest blockers are the public
server/session/transaction surface, complete RRFlowQL mutations and indexes,
live subscriptions, comprehensive identity/security, supported SDKs,
backup/export/restore, production operations, and a real estate control plane.

Those gaps must be implemented and verified before any claim that RRFlow is a
SurrealDB competitor rather than a promising runtime/storage kernel.
