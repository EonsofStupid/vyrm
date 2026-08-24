# RRFlow and the Reason Ready Daemon

Status: authoritative pre-release product architecture.

This document defines the target system boundary. It supersedes older text that
describes Vyrm as a permanent product, engine brand, or independently governed
subsystem. The current `vyrm-*` code remains migration input until the controlled
migration in [`rrflow-vyrm-migration-ledger.md`](rrflow-vyrm-migration-ledger.md)
is complete.

## Decision

**RRFlow means Reason Ready Flow and is the single product. RRD means Reason
Ready Daemon and is its one native engine/runtime.**

RRD is what the pre-release Vyrm implementation was becoming. It is RRFlow's
internal durable data and AI-runtime authority and the actual Fjall competitor.
It joins persistence, transactions, the catalogue, query planning and
execution, indexes, reasoning state, lifecycle enforcement, security, audit,
recovery, and diagnostics through one engine contract.

Vyrm is not a layer below RRD in the target architecture. `vyrm-*`, `vyrmd`,
VyrmQL, `.vyrm`, and related identifiers name pre-release implementation that
must be migrated into RRFlow/RRD. They may survive temporarily only where a
versioned reader or forwarding shim is required to recover existing data or
complete an atomic migration.

## Controlled vocabulary

| Term | Meaning | Must not mean |
|---|---|---|
| RRFlow | Reason Ready Flow: the complete product, repository, CLI, configuration namespace, SDK family, and user-facing platform | One component beside Vyrm or RRD |
| RRD | Reason Ready Daemon: RRFlow's single native engine/runtime and service process | A second product brand, a thin HTTP wrapper, an alias for only the LSM, or only the reasoning loop |
| Connectome | RRFlow's local and enterprise operator/developer client | An authoritative database, policy store, or second runtime |
| RRFlowQL | RRFlow's query language and typed query contract | A separately persisted engine |
| adapter | A provider, runtime, protocol, package-runner, object-store, accelerator, or compatibility implementation behind an RRFlow port | An owner of policy, catalogue, transaction, or lifecycle truth |
| projection | Rebuildable data derived from an authoritative RRD commit | A competing source of truth |
| physical operator | A specialized LSM, columnar, graph, vector, temporal, geo, inference, or distributed execution implementation | A separate logical database |

New public names use `RRFlow`. `RRD` is reserved for the internal daemon role,
its process identity, and its service protocol. New code must not introduce a
Vyrm name.

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
implemented. In particular, the repository currently has no Arrow or
DataFusion dependency. The existing VyrmQL/VyrmMX path is a bespoke row
executor and remains a conformance oracle while the RRD Arrow/DataFusion plane
is introduced.

## One engine product

RRD is not `rrd-server` placed in front of unrelated Vyrm verticals. The target
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

## AI-runtime ownership

RRD stores and enforces the provider-neutral lifecycle described in
[`rrflow-runtime-foundation-cheat-sheet.md`](rrflow-runtime-foundation-cheat-sheet.md):
project attunement, preflight, context evidence, reasoning attempts, tool
authorization, observations, invalidation, verification, outcomes, compaction,
and replay.

These transitions use the same catalogue, transactions, read stamps, security,
audit, graph/index freshness, and recovery path as application data. They do
not live in a sidecar memory database. Provider hooks and MCP translate or
expose this contract; they do not own it.

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
├─ rrflow-engine            RRD composition root and only authoritative engine API
├─ rrflow-core              canonical identities, values, events, transactions
├─ rrflow-storage           persistence port, archives, recovery, engine selection
├─ rrflow-storage-lsm       native WAL/MVCC/LSM physical implementation
├─ rrflow-query             RRFlowQL, catalogue binding, planning, DataFusion execution
├─ rrflow-graph             graph and project-knowledge physical operators
├─ rrflow-vector            exact/ANN/compressed-vector physical operators
├─ rrflow-inference         provider-neutral embedding/inference jobs
├─ rrflow-runtime           attunement, lifecycle and enforced tool boundary
├─ rrflow-cluster           replication, placement and distributed evidence
├─ rrflow-security          identity, authorization and audit
├─ rrflow-protocol          versioned RRD wire contract
├─ rrflow-daemon            hosts `rrflow-engine` as the `rrd` service process
├─ rrflow-client            supported Rust client
├─ rrflow-control-plane     estates, reconciliation, backup and deployment jobs
├─ rrflow-kubernetes        Kubernetes control adapter
├─ rrflow-eval              controlled evaluation harness
├─ rrflow-cli               `rrflow` command surface
└─ connectome               local/enterprise operator client
```

This tree is a destination, not permission for a bulk move. `rrflow-query` may
retain private parser, logical-plan, physical-plan, and executor subcrates if
compile times justify them, but it exposes one query contract. The same rule
applies to storage and indexes.

Only `rrflow-engine` composes the lower data/runtime components. Embedded users
link it directly; `rrflow-daemon` hosts the same engine. The daemon, protocol,
SDKs, CLI, adapters, and Connectome must not bypass it to assemble their own
catalogue, transaction, query, vector, graph, or lifecycle behavior.

The RRD executable is `rrd`. User/project commands use `rrflow`. Cooperative
MCP tools and provider adapters use RRFlow names because they are product-facing
surfaces, not the daemon itself.

## Current implementation truth

As of 2026-08-24:

- substantial persistence, query, vector, runtime, protocol/server, SDK,
  estate/security, cluster, and Connectome capabilities are real but distributed
  across legacy and newer crate boundaries;
- `rrd-server` directly composes multiple `vyrm-*` crates rather than one
  finished RRD engine composition root;
- query parsing/execution and vector planning have separate catalogues and
  execution paths that still require unification behind the RRD catalogue and
  transaction/read-stamp contract;
- no Arrow/DataFusion dependency is present, so columnar execution is target
  architecture rather than delivered capability;
- several source files exceed 2,000 lines, so migration must extract reviewed
  responsibilities rather than merely rename oversized modules;
- existing RRD wire contracts and SDK names are closer to the target than the
  Vyrm surfaces, but generated descriptions still leak legacy internal types.

## Completion gates

The Vyrm-to-RRFlow migration is complete only when:

1. no public package, binary, protocol description, configuration, SDK,
   environment variable, UI label, or current documentation introduces Vyrm;
2. every legacy path, magic byte, domain separator, and persisted identifier is
   either migrated through a tested reader or deliberately retired before the
   first stable release;
3. RRD exposes one catalogue, transaction/read-stamp model, security authority,
   event/outbox path, and recovery model across every supported data model;
4. embedded execution and the `rrd` process use the same `rrflow-engine`
   composition root, with no consumer rebuilding the engine from lower crates;
5. RRFlowQL streams Arrow RecordBatches through a DataFusion-backed execution
   plane with semantic differential coverage against the existing executor;
6. embedded, local-daemon, and remote paths pass the same conformance corpus;
7. restart, corruption, migration, cancellation, backpressure, stale-index,
   fail-closed authorization, and cross-version recovery tests pass; and
8. all required Linux, macOS/ARM, and Windows CI jobs are green.
