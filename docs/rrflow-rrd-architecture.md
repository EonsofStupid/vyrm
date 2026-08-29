# RRFlow and the Reason Ready Daemon

Status: authoritative pre-release product architecture.

This document defines the target system boundary. It supersedes older text that
treated the retired pre-release identity or any physical crate as a separately
governed product. The in-place RRFlow `0.1.0` alpha identity contract is enforced by
[`rrflow-rename-ledger.md`](rrflow-rename-ledger.md).
The sole canonical platform vocabulary and hierarchy are in
[`platform/README.md`](platform/README.md); this document defines how those
resources execute and must not create a second glossary.

## Decision

**RRFlow means Reason Ready Flow and is the single product. RRD means Reason
Ready Daemon and is its one native engine/runtime.**

RRD is RRFlow's internal durable data and AI-runtime authority and the actual
Fjall competitor.
It joins persistence, transactions, the catalogue, query planning and
execution, indexes, reasoning state, lifecycle enforcement, security, audit,
recovery, and diagnostics through one engine contract.

RRFlow is not a storage layer below RRD. RRFlow owns the product, CLI, project
configuration, adapters, SDK family, and RRFlowQL language. RRD owns the single
engine/daemon contract. The retired pre-release identity has no compatibility
reader, alias, or forwarding shim in V1.

## Reference architecture precedence

RRFlow has one architectural spine. References are applied in this order and
are not blended into competing abstractions:

1. **SurrealDB-shaped logical database flow.** RRD follows one Rust engine and
   one API/query contract across embedded, single-node, and distributed service
   faces; namespace → database → table/record/relation is the logical hierarchy;
   schema, transactions, permissions, indexes, live changes, backup, and
   deployment remain capabilities of that engine. See
   [SurrealDB architecture](https://surrealdb.com/docs/architecture) and its
   [namespace/database architecture](https://surrealdb.com/docs/learn/schema-management/multi-tenancy/namespace-and-database-architecture).
2. **RRD execution-plane integration.** RRFlowQL retains RRD semantics, but
   bound logical plans lower to DataFusion and stream Arrow `RecordBatch`es.
   This is the deliberate query-execution integration, not another catalogue,
   storage authority, or public data model. See the official
   [DataFusion introduction](https://datafusion.apache.org/user-guide/introduction.html)
   and [Arrow streaming model](https://datafusion.apache.org/user-guide/arrow-introduction.html).
3. **Qdrant-shaped vector subsystem.** Collections, points, named vectors,
   payloads, payload indexes, strict-mode resource protection, HNSW, sparse and
   hybrid retrieval, segments, shards, replicas, memory tiers, quantization,
   oversampling, and exact rescoring follow Qdrant's public capability flow.
   They remain internal to the same RRD catalogue, transaction, read stamp,
   security, and audit authority. See [Qdrant data management](https://qdrant.tech/documentation/manage-data/),
   [distributed deployment](https://qdrant.tech/documentation/scaling/distributed_deployment/),
   and [quantization](https://qdrant.tech/documentation/manage-data/quantization/).
4. **TurboQuant for qualified memory pressure.** TurboQuant is a vector
   artifact and scoring path, not a replacement engine. RRD keeps the canonical
   full-precision vector, uses full-precision queries, and may select a prebuilt
   4-, 2-, 1.5-, or 1-bit artifact when an explicit memory policy and measured
   recall budget permit it. The algorithm reference is
   [TurboQuant](https://arxiv.org/abs/2504.19874); operational behavior is
   constrained by the Qdrant quantization lifecycle above.

HelixDB remains bounded research for graph/vector/text transactional cohesion.
It does not define RRFlow's hierarchy, control plane, public terminology, or a
second implementation route.

## System boundary

```text
operators, applications, local/frontier AI
                     │
        RRFlow CLI, SDKs and adapters
                     │
        canonical commands and events
                     │
┌────────────────────▼──────────────────────────────────────────┐
│ RRD — Reason Ready Daemon                                    │
│                                                              │
│  identity / policy / audit                                   │
│              │                                               │
│  transaction coordinator ─ read stamps ─ snapshot ownership  │
│              │                                               │
│  authoritative catalogue and multi-model data contract       │
│              │                                               │
│  RRFlowQL → logical plan → DataFusion physical execution     │
│              │                         │                     │
│  lifecycle / knowledge state        Arrow RecordBatch stream │
│              │                         │                     │
│  WAL / MVCC / LSM / columnar / object persistence            │
│              │                                               │
│  scalar / graph / vector / temporal / geo projections        │
│              │                                               │
│  durable events / outbox / live query / replay / tracing     │
└────────────────────┬──────────────────────────────────────────┘
                     │
       Connectome control and visualization
```

The diagram is a logical ownership model, not a claim that every box is already
implemented. Typed Arrow conversion and DataFusion 55 execution exist for
bounded relational operations over one materialized stamped snapshot. An
immutable RRD provider now returns schema-stable bounded batches through a
physical stream, reports exact projection/limit hints and unsupported filters,
and enforces snapshot/operator memory, temporary spill, batch, and elapsed-time
budgets. Lazy storage-to-Arrow scans and broader physical optimization remain
incomplete; reference row semantics remain the conformance oracle.

## Complete end-to-end RRFlow flow

Every embedded, local-daemon, remote, project, and estate operation follows the
same ordered path. A surface may omit unavailable operations; it cannot invent
another path.

```text
project or estate command
  → resolve organization/project/environment/instance and exact resource path
  → authenticate session and load revision-bound AuthorizationContext
  → select namespace/database and capture catalogue revision + RRD read stamp
  → parse RRFlowQL or validate the equivalent typed operation
  → bind table/collection/record/point/relation/index identities
  → inject tenant/row/field privileges as indexed logical predicates
  → optimize the logical plan
  → lower to DataFusion plus typed RRD extension nodes
  → execute storage/graph/vector/temporal/geo operators
  → stream schema-bound Arrow RecordBatches with budgets and backpressure
  → commit mutation + audit + outbox atomically, or return a stamped read
  → publish live/change/reasoning events from the same commit identity
  → project generated API/SDK/CLI/MCP/Connectome results
```

### Data and transaction path

1. The instance resolves exactly one project and environment; the estate is a
   manager of instances, never an implicit data scope.
2. Namespace and database selection is explicit. Session defaults may be
   ergonomic aliases only after the fully resolved resource path is retained.
3. One immutable catalogue snapshot resolves schemas, tables, collections,
   relations, aliases, indexes, strict-mode policy, functions, and capabilities.
4. One read stamp fixes MVCC visibility, valid time, known-at time, catalogue
   revision, security revision, and required projection generations.
5. A mutation stages every logical model through one transaction coordinator.
   WAL, MVCC, LSM, object, index-integrity, audit, and outbox changes publish
   one commit cursor or none publish.
6. Async graph, text, vector, columnar, embedding, and live projections retain
   the source cursor and never become unstamped truth.
7. The same logical request and fixtures run through embedded, daemon, remote,
   and distributed faces.

### Query and authorization path

1. RRFlowQL or a generated typed request parses into one RRD logical plan.
2. A compiled `AuthorizationContext` admits the operation once. Tenant, row,
   and field restrictions become plan predicates and projections before
   physical selection.
3. DataFusion performs common logical and physical work over Arrow; RRD
   extension nodes own temporal reads, graph traversal, vector retrieval,
   lifecycle operations, and other semantics DataFusion does not natively own.
4. Exact/inexact/unsupported pushdown is explicit. Security predicates may
   never disappear during pushdown, post-filtering, approximation, or fallback.
5. Record, byte, memory, spill, time, concurrency, candidate, and result budgets
   are fixed before execution and enforced while batches stream.
6. No storage read, policy interpretation, or allocation for RBAC occurs inside
   record, graph-edge, vector-candidate, or segment scoring loops.

## One engine product

RRD is not `rrd-server` placed in front of unrelated RRFlow verticals. The target
has one engine composition root, used directly for embedded execution and
hosted by the `rrd` process for local or remote execution. That composition root
owns the catalogue, transaction coordinator, read stamps, query execution,
security decisions, durable events, and recovery. Lower crates are internal
physical components and cannot be composed independently into another RRFlow
truth.

RRD has exactly one of each authoritative concern:

- catalogue for schema, models, relations, indexes, projections, embeddings,
  lifecycle types, and deployment-visible capabilities;
- transaction coordinator for canonical writes across supported models;
- snapshot and read-stamp model for repeatable reads;
- identity and policy-decision path;
- durable mutation/event log and transactional outbox;
- query contract and execution result contract;
- recovery and version-migration authority.

Physical components may maintain specialized files and indexes. They must not
own competing catalogues, clocks, identities, authorization decisions, commit
cursors, or product lifecycle state.

### Canonical commit

One accepted RRD transaction atomically establishes:

1. canonical data mutations;
2. schema and catalogue revision changes;
3. one commit cursor and hash-chain transition;
4. synchronous integrity indexes required to validate the next write;
5. the accepted-operation audit record; and
6. outbox work for asynchronous indexes, live queries, embeddings,
   invalidation, and diagnostics.

Derived work may finish later, but its source cursor, target generation,
freshness, failure, and retry state are authoritative RRD data. A query that
requires a projection must either prove its freshness at the transaction's read
stamp, use an authoritative fallback, or fail explicitly.

## Persistence and recovery

RRD owns persistence as a complete product capability, not as a branded engine
underneath another database. The target storage plane includes:

- authenticated WAL, manifests, segments, and atomic current-root publication;
- MVCC visibility and conflict detection;
- LSM structures for the mutable ordered working set;
- Arrow/columnar batches and immutable analytical artifacts where measured
  workloads justify them;
- content-addressed object storage for large artifacts;
- snapshots, checkpoints, retention pins, compaction, and garbage collection;
- logical archives, backup catalogues, restore-to-new-root, resumable format
  migration, and cross-version recovery;
- local, object-store, and later distributed durability implementations behind
  capability-checked ports.

Immutable V3 segments share one configured physical-I/O boundary: mmap,
runtime-probed Linux io_uring, or bounded positional fallback all feed the same
authenticated block decoder and bounded decoded cache. Backend operations,
bytes, peak request size, and fallback are measured separately from compute
cache residency. Large archives/artifacts use the same immutable object port;
an S3-compatible transport is admitted only with authenticated signed payloads,
conditional publication, checksums, resumable multipart state, pagination, and
version-bound ranged reads. This is tiering inside RRD, not another database or
transaction authority.

The existing native LSM/MVCC code is retained and migrated into RRD. This is the
physical work that competes with Fjall. The Fjall path remains a temporary
compatibility reader and differential oracle; it is never an RRD engine mode or
default and does not define RRFlow architecture.

## Arrow and DataFusion execution plane

RRFlowQL remains RRFlow's language. It lowers into a versioned RRFlow logical
plan bound to one catalogue revision and read stamp. DataFusion is the execution
framework, not the owner of RRD semantics or persistence.

```text
RRFlowQL / typed SDK request
  → parse and normalize
  → bind names and types to an immutable RRD catalogue snapshot
  → produce a content-addressed RRFlow logical plan
  → apply semantics-preserving logical rewrites
  → lower through DataFusion and RRFlow extension nodes
  → apply resource-, index-, and hardware-aware physical rewrites
  → stream schema-bound Arrow RecordBatches
  → record plan, read-stamp, budgets, metrics, and outcome
```

RRD exposes authoritative models through snapshot-bound table providers. A
provider describes schema and pushdown capabilities during planning; physical
I/O occurs while the returned record-batch stream is polled. Planning must not
silently read mutable state or perform expensive I/O. This follows DataFusion's
current separation between `TableProvider`, `ExecutionPlan`, and
`SendableRecordBatchStream`; see the official
[custom table provider guide](https://datafusion.apache.org/library-user-guide/custom-table-providers.html)
and [logical-plan guide](https://datafusion.apache.org/library-user-guide/building-logical-plans.html).

Arrow `RecordBatch` is the internal and streaming tabular result boundary. It
does not replace canonical row identities or on-disk recovery formats. Arrow
IPC may be a transport/diagnostic representation only when its schema and
version negotiation are explicit. See the official
[Arrow Rust RecordBatch documentation](https://arrow.apache.org/rust/arrow/record_batch/index.html).

### Required execution invariants

- A query uses one immutable catalogue snapshot and one RRD read stamp.
- Logical plans contain no storage-engine handles or provider-specific policy.
- Pushdown is reported as exact, inexact, or unsupported; unsupported work is
  retained above the scan.
- Index selection proves model, schema, source cursor, configuration, artifact,
  and visibility compatibility before I/O.
- All scans, joins, traversals, vector operations, and outputs have explicit
  row, byte, time, memory, spill, and concurrency budgets.
- Streaming applies cancellation and backpressure and never buffers an
  unbounded result before returning the first batch.
- Existing reference-executor fixtures remain the semantic oracle until the
  DataFusion path passes model, engine, restart, and property differentials.
- DataFusion upgrades require plan/behavior conformance; serialized DataFusion
  internals are not a stable RRD persistence format.

## Multi-model and index ownership

Documents, records, relations, graph edges, temporal values, time-series
samples, geospatial values, and supplied vectors are canonical typed RRD data.
An embedding derived from other data additionally binds source digest, model
identity and digest, dimensions, normalization, and generation parameters.

Scalar, graph, full-text, HNSW, compressed-vector, temporal, geo, and columnar
structures are projections unless a contract explicitly identifies a supplied
value as canonical. Multi-vector and late-interaction retrieval use one object
identity and one transaction boundary; application middleware must not create
parallel records solely to join those vectors later.

Query indexes form one engine-owned catalogue, not independent embedded
databases. Scalar/compound, geo, filtered materialized-view, global-count,
grouped-count, and BM25 definitions all publish content-addressed immutable
artifacts with generation, source cursor, catalogue revision, valid time,
configuration digest, and maintenance evidence. A planner may select an
artifact only when those coordinates exactly cover its captured read; stale
artifacts remain visible as rejected candidates and execution falls back to
canonical RRD data. Corrupt selected bytes fail closed.

Unique scalar/compound definitions are different from optional accelerators:
the engine checks their keys and overlapping validity windows against the
prospective transaction at the serialized validation/commit boundary. Derived
maintenance never enters that authoritative commit path. BM25 artifacts retain
deterministic analyzer configuration, term frequencies, positions, and UTF-8
source offsets so score, matched-term, offset, and highlight evidence all come
from the same stamped generation.

## Qdrant-shaped vector and TurboQuant flow

The vector path is one branch of the RRD plan, not a side database:

```text
database catalogue
  → resolve collection or same-kind alias
  → resolve point record + named vector schema + payload schema
  → bind metric, dimensions, strict mode, tenant policy and read stamp
  → plan payload indexes and shard route
  → choose exact / HNSW / sparse / hybrid / multivector operator
  → choose full-precision or qualified quantized artifact
  → retrieve an oversampled candidate set
  → exact-rescore from canonical vectors when policy requires and budget permits
  → deterministic top-k point records + payload projection
  → Arrow RecordBatch stream and stamped query evidence
```

### Memory-pressure policy

Memory pressure changes artifact placement and the chosen access path; it never
changes logical results silently or destroys the canonical vector.

1. **Full-precision hot path:** keep canonical vectors and the selected index
   pinned or cached when the collection budget permits.
2. **Separated hot/cold path:** move canonical vectors to mmap/cold storage
   while keeping the routing graph and compact scoring artifact pinned or
   cached.
3. **TurboQuant path:** prefer a prebuilt 4-bit artifact for the first compact
   tier; select 2-, 1.5-, or 1-bit only when corpus-specific recall, latency,
   and compression gates pass. Queries remain full precision.
4. **Candidate and rescore path:** retrieve more than `top_k` from the compact
   artifact, then rescore canonical full-precision vectors when the collection
   policy requires it. Cold-vector I/O is budgeted and reported.
5. **Fallback:** if the required artifact is absent, stale, corrupt, or outside
   its recall contract, use an allowed exact/full-precision path or fail
   explicitly. Never build or select an unrecorded codec because free memory
   happened to fall during a request.

Every vector artifact binds collection, named vector, source cursor, catalogue
revision, dimensions, metric, codec, bit depth, rotation/seed, build version,
memory tier, checksum, and benchmark corpus. TurboQuant must pass exact-oracle
recall plus end-to-end compression, build-time, latency, SIMD/scalar, restart,
and corruption differentials before becoming selectable.

Qdrant documents 4-bit TurboQuant as an 8× representation and the lower bit
depths as progressively smaller, with quantized vectors stored alongside
originals and optional oversampling/rescoring. RRD adopts that lifecycle and
validates it against its own data rather than treating a published ratio as a
local performance claim.

## AI-runtime ownership

RRD stores and enforces the provider-neutral lifecycle described in
[`rrd-engine-foundation-cheat-sheet.md`](rrd-engine-foundation-cheat-sheet.md):
project attunement, preflight, context evidence, reasoning attempts, tool
authorization, observations, invalidation, verification, outcomes, compaction,
and replay.

These transitions use the same catalogue, transactions, read stamps, security,
audit, graph/index freshness, and recovery path as application data. They do
not live in a sidecar memory database. Provider hooks and MCP translate or
expose this contract; they do not own it.

The catalogue is concretely the revisioned `RuntimeSchemaRegistry`, not a
diagram-only abstraction. It binds one namespace/database to typed table
entries for every logical model and carries an explicit strict or schemaless
mode. Record-like document, relational, graph-node, key-value, reasoning, and
lifecycle views converge on the canonical record identity; vector, series,
geo, object, relation, and event families remain structurally distinct but are
admitted by the same registry revision and commit. See
[`rrd-unified-catalogue.md`](rrd-unified-catalogue.md).

## Service faces

| Face | Purpose | Required equivalence |
|---|---|---|
| Embedded | In-process latency-sensitive use | Same transactions, snapshots, events, policy, and recovery |
| Local daemon | Per-project desktop/tooling service | Same logical operations through the RRD protocol |
| Remote | Authenticated network and later distributed deployment | Same contract plus qualified transport/placement guarantees |

The `rrd` process is not the engine wrapped in HTTP. It composes the engine and
serves its authoritative contract. A face may add transport concerns but may
not change mutation, visibility, authorization, or replay semantics.

## Connectome boundary

Connectome consumes authoritative RRD APIs and event streams. It visualizes
catalogues, models, estates, topology, reasoning hops, query plans, indexes,
latency, freshness, and historical replay. Freeze, rewind, fast-forward, and
comparison operate over durable event identities and snapshots rather than UI
timers or fabricated demo state.

Connectome may submit commands and desired state. RRD/RRFlow control logic
validates and persists them. Connectome never becomes the only location where
an operation, note, policy decision, or diagnostic history exists.

## Target source organization

Crates are compilation boundaries inside one product, not separately branded
applications. The target names are:

```text
crates/
├─ rrd-engine             RRD composition root, lifecycle, and engine API
├─ rrd-core               canonical identities, values, events, transactions
├─ rrd-store              persistence port, archives, recovery, engine selection
├─ rrd-lsm                native WAL/MVCC/LSM physical implementation
├─ rrd-query              RRFlowQL, catalogue binding, planning, execution
├─ rrd-graph              graph and project-knowledge physical operators
├─ rrd-vector             exact/ANN/compressed-vector physical operators
├─ rrd-inference          provider-neutral embedding/inference jobs
├─ rrd-operator-knowledge external operator-knowledge projection
├─ rrd-cluster            replication, placement and distributed evidence
├─ rrd-security           identity, authorization and audit
├─ rrd-contract           versioned RRD wire contract
├─ rrd-server             hosts `rrd-engine` as the `rrd` service process
├─ rrd-client             supported Rust client
├─ rrd-estate             estates, reconciliation, backup and deployment jobs
├─ rrd-kubernetes         Kubernetes control adapter
├─ rrflow-eval              controlled evaluation harness
├─ rrflow-cli               `rrflow` command surface
└─ connectome               local/enterprise operator client
```

This tree is the canonical package vocabulary. `rrd-query` may
retain private parser, logical-plan, physical-plan, and executor subcrates if
compile times justify them, but it exposes one query contract. The same rule
applies to storage and indexes.

Only `rrd-engine` composes the lower data/runtime components. Embedded users
link it directly; `rrd-server` hosts the same engine. The daemon, protocol,
SDKs, CLI, adapters, and Connectome must not bypass it to assemble their own
catalogue, transaction, query, vector, graph, or lifecycle behavior.

The RRD executable is `rrd`. User/project commands use `rrflow`. Cooperative
MCP tools and provider adapters use RRFlow names because they are product-facing
surfaces, not the daemon itself.

## Work-plan ownership map

The enforced 65-item RRFlow board is the implementation order for this flow:

| Flow slice | RRFlow gate/work items |
|---|---|
| Enforced planning, mutation authorization, and verification evidence | G00-W01 through G00-W05 |
| One RRD composition root, vocabulary, authority, and capability catalogue | G01-W01 through G01-W04 |
| WAL/MVCC/LSM, recovery, archives, backup, migration, time travel, and tiered persistence | G02-W01 through G02-W06 |
| Namespace/database catalogue, multi-model transactions, RRFlowQL, Arrow/DataFusion, indexes, live queries, and functions | G03-W01 through G03-W07 |
| Qdrant-shaped collections/points/payloads, ANN/hybrid retrieval, TurboQuant and other codecs, memory tiers, inference, and GPU | G04-W01 through G04-W07 |
| Equivalent service faces, sessions, client transactions, RBAC/privileges, TLS, and audit | G05-W01 through G05-W04 |
| Generated API, MCP, CLI, and SDK surfaces | G06-W01 through G06-W05 |
| Project attunement, context, architecture assessment, exact tool control, and provider/local-model adapters | G07-W01 through G07-W05 |
| Estate reconciliation, distributed consensus/data placement, Kubernetes, and hybrid-cloud operation | G08-W01 through G08-W03 |
| Connectome application, canonical client, connection profiles, operations, diagnostics, and board control | G09-W01 through G09-W06 |
| Persistent scenarios, surface/process matrices, destructive faults, and recovery/security qualification | G10-W01 through G10-W04 |
| Reproducible SurrealDB/Qdrant/Fjall differentials and evidence-backed reports | G11-W01 through G11-W05 |
| Repository/platform quality, packaging, upgrade/rollback, and firm-alpha declaration | G12-W01 through G12-W04 |

The work plan is complete as a target map, not as implementation. Its persisted
RRD state is the only completion authority; run
`rrflow --db .rrflow/rrd work-plan status` for the current verified and active
items. No unchecked row may be described as a finished capability, and this
architecture map intentionally carries no fixed verified-count snapshot.

## Current implementation truth

As of 2026-08-29:

- substantial persistence, query, vector, runtime, protocol/server, SDK,
  estate/security, cluster, and Connectome capabilities are real but distributed
  across canonical RRD physical crate boundaries;
- `rrd-engine` is the only product composition and storage-opening authority;
  Connectome is client-only, CLI embedded operations use the project-bound
  engine, and security/estate product executables are thin outward adapters;
  the architecture test rejects any return of `EmbeddedOperator`,
  `runtime_store`, outward `PersistentEngine::open`, or physical-crate
  executable ownership;
- query parsing/execution and vector artifact planning retain specialized
  derived catalogues and execution paths that must consume the now-unified RRD
  logical catalogue through G03/G04 rather than becoming authorities;
- the SurrealDB-shaped namespace/database hierarchy now has canonical public
  resource terms and a persisted, revisioned multi-model table catalogue;
  one runtime transaction and exact-stamp snapshot now cover CRUD and
  history-preserving retirement across document, key-value, relational/graph,
  event, vector, series, geo, and object models. Bounded RRFlowQL programs now
  compose the same typed mutations under explicit begin/commit/cancel semantics,
  and exact equi-joins reduce both sources from one authenticated read stamp.
  Dedicated protocol administration and richer mutation expressions remain
  open. Executable multi-project
  instance topology has been removed while
  an explicit successor-format migration is still required to persist the
  environment identity without invalidating format-1 authority digests;
- typed Arrow conversion, DataFusion filter/projection/limit execution, and
  deterministic bounded result batches are present. The stamped snapshot
  provider, pushdown-disposition evidence, physical batch stream, cancellation,
  and memory/spill caps are executable. Joins retain a hard intermediate-
  cardinality budget; lazy direct storage scans and broader physical
  optimization remain open;
- TurboQuant contract variants and an experimental physical artifact path are
  present, but they are not a qualified Qdrant-complete collection lifecycle or
  an automatic memory-pressure policy;
- current authorization reloads security state and linearly scans direct
  grants per request; compiled roles/privileges and the locked warm-path
  performance invariant remain incomplete;
- several source files exceed 2,000 lines, so migration must extract reviewed
  responsibilities rather than merely rename oversized modules;
- existing RRD wire contracts and SDK names are closer to the target than the
  RRFlow surfaces, but generated descriptions still leak legacy internal types.

## Completion gates

The RRFlow `0.1.0` alpha identity cutover and cohesion checkpoint are complete only when:

1. every public package, binary, protocol description, configuration, SDK,
   environment variable, UI label, and current document uses the canonical
   RRFlow/RRD vocabulary;
2. every V1 path, magic byte, domain separator, and persisted identifier uses
   canonical RRFlow/RRD bytes and passes its decode/reopen fixture without a
   retired-name reader or shim;
3. RRD exposes one catalogue, transaction/read-stamp model, security authority,
   event/outbox path, and recovery model across every supported data model;
4. embedded execution and the `rrd` process use the same `rrd-engine`
   composition root, with no consumer rebuilding the engine from lower crates;
5. RRFlowQL streams Arrow RecordBatches through a DataFusion-backed execution
   plane with semantic differential coverage against the existing executor;
6. embedded, local-daemon, and remote paths pass the same conformance corpus;
7. restart, corruption, migration, cancellation, backpressure, stale-index,
   fail-closed authorization, and cross-version recovery tests pass; and
8. all required Linux, macOS/ARM, and Windows CI jobs are green.
