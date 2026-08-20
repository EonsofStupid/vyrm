# Vyrm data-runtime architecture research

Research baseline: 2026-08-19.

Status: design input, not an implementation claim. No upstream source was
copied into Vyrm during this pass. The pinned sources below make every adopted
idea reproducible and keep later code imports distinguishable from original
Vyrm work. The Qdrant, SurrealDB, and HelixDB waiver status supplied by the
project owner is recorded as project context; it does not change the technical
acceptance gates.

## Decision

Vyrm should become a multi-model data runtime without turning every data model
into a separate database. One canonical transaction log remains truth. Typed
records, relations, claims, events, vectors, series samples, geospatial values,
and object references join that transaction. Search structures are
independently rebuildable, cursor-grounded projections.

The target layers are:

```text
CLI / MCP / Node / Connectome
              |
          vyrmQL
     parse -> AST -> bind
              |
          vyrmMX
 logical plan -> policy -> physical plan -> stream
              |
          vyrmDS
 session -> catalog -> transaction -> snapshot -> audit -> changefeed
              |
          vyrmKV
 WAL -> MVCC memtable -> immutable segments -> manifest -> checkpoint -> GC
              |
      grounded projections
 scalar | graph | text | vector | time-series | geo
              |
   local files or immutable object storage
```

These names describe real boundaries to build, not aliases for the current
`Engine` trait:

- **vyrmQL** owns syntax, parsing, diagnostics, and AST-to-logical-plan
  lowering. It does not perform storage I/O.
- **vyrmMX** owns catalog binding, capability checking, optimization, physical
  planning, execution budgets, and streaming results.
- **vyrmDS** owns sessions, schema/catalog revisions, logical transactions,
  snapshot handles, authorization, audit, changefeeds, and coordination of
  canonical mutations with derived projections.
- **vyrmKV** owns the native physical key/value engine: WAL, MVCC, immutable
  segments, manifests, compaction, recovery, checkpoints, and garbage
  collection.

`vyrm-core` remains the small portable contract crate. It may contain serialized
query algebra and transaction wire types, but never the parser, optimizer,
backend, network client, GPU runtime, or object-store SDK.

## Current Vyrm baseline

| Capability | Current state | Required boundary |
|---|---|---|
| Atomic canonical write | `RuntimeCommit` atomically writes schema, claims, typed records, relations, events, cursor, and hash chain | Generalize the mutation envelope without weakening current validation |
| Temporal truth | Claim valid/known time and runtime graph `valid_at`/cursor snapshots exist | Make a reusable, leased `SnapshotHandle` rather than reopening an implicit snapshot per call |
| Optimistic conflict detection | Exact expected runtime cursor is checked in the write transaction | Generalize to transaction read stamps and explicit conflict classes |
| Storage abstraction | `vyrm_store::Engine` provides claim/runtime primitives | Split logical DS transactions from physical KV operations; the current trait remains the migration harness |
| Native storage | Absent; Fjall is the compatibility adapter | Build WAL, MVCC, manifests, recovery, checkpoint pinning, and compaction in `vyrmKV` |
| Query language | Absent | Build typed `vyrmQL`; do not grow ad-hoc endpoint parameters into a language |
| Planner/executor | Absent | Build `vyrmMX` with logical/physical plans and observable plan decisions |
| General snapshots | Runtime graph can be reconstructed at a cursor | Add stable snapshot identity, catalog/schema revision, leases, retention pins, and lifecycle APIs |
| Object storage | Absent | Stage immutable content-addressed objects; atomically publish references through `vyrmDS` |
| Vector/embedding | Absent | Exact reference search first, then grounded ANN/quantization and embedding provenance |
| Multi-AZ | Absent | Add only after single-node crash/recovery and transaction semantics are model-checked |
| Audit | Invocation and hash-chained runtime evidence exist, but not a comprehensive API audit stream | Add a typed, JSON-serializable audit envelope covering allow, deny, query, mutation, DDL, and administrative operations |

The existing runtime log, schema registry, bitemporal graph, grounding behavior,
and in-memory differential are assets. Replacing them with a generic database
API would be a regression.

## Primary-source findings

### Waiver-backed systems

| Source snapshot | Pattern worth adopting | Vyrm decision |
|---|---|---|
| [Qdrant `74f3e85`](https://github.com/qdrant/qdrant/tree/74f3e85b9473c62560006c043e13737ce6b48412) | Independent mutable/immutable segments; WAL-applied sequence; background optimization; payload-aware vector planning; segment manifests; CPU/GPU HNSW building; quantization; shard recovery; a reference model compared against stored state | Adapt segment/index lifecycle and differential testing in `vyrm-vector`. Keep the canonical Vyrm commit above vector segments, and require an exact-search oracle before ANN. GPU builds artifacts; it never changes query semantics. |
| [SurrealDB `9d9a5b0`](https://github.com/surrealdb/surrealdb/tree/9d9a5b0693e499e0d030cac6b618062ec02cd2bc) | Query parser separated from executor; transaction creation separated from the byte-oriented `Transactable` API; backend-specific extensions kept out of the common trait; version time propagated through a physical operator; cursor scans tied to transaction lifetime | Adapt the separation, snapshot lifetime, and version propagation. Do not clone SurrealQL or put parser/execution code in `vyrm-core`. Vyrm needs both valid time and known cursor, not a single overloaded version timestamp. |
| [HelixDB `475e805`](https://github.com/HelixDB/helix-db/tree/475e805cd864be7ff81c09ee6ba9a18ccc4d918b) | Valid-by-construction planner/index types; catalog snapshots; explicit index identity, generation, revision, operation state, and cursor ownership; pure vector policy decisions; transaction-local index mutation measurement; production-linked planner and failure tests | Adapt typed planner IR, catalog snapshots, generation publication, progress cursors, pure policy functions, and failure injection. Do not bind Vyrm’s truth model to HelixDB’s current physical layout or treat graph/vector indexes as canonical. |

Concrete code anchors reviewed:

- Qdrant [segment manifest](https://github.com/qdrant/qdrant/blob/74f3e85b9473c62560006c043e13737ce6b48412/lib/segment/src/data_types/manifest.rs), [universal query representation](https://github.com/qdrant/qdrant/blob/74f3e85b9473c62560006c043e13737ce6b48412/lib/collection/src/operations/universal_query/collection_query.rs), [update worker](https://github.com/qdrant/qdrant/blob/74f3e85b9473c62560006c043e13737ce6b48412/lib/collection/src/update_workers/update_worker.rs), [streamed graph builder](https://github.com/qdrant/qdrant/blob/74f3e85b9473c62560006c043e13737ce6b48412/lib/segment/src/index/hnsw_index/graph_layers_builder.rs), [GPU HNSW builder](https://github.com/qdrant/qdrant/blob/74f3e85b9473c62560006c043e13737ce6b48412/lib/segment/src/index/hnsw_index/hnsw/gpu_build.rs), and [model verification](https://github.com/qdrant/qdrant/blob/74f3e85b9473c62560006c043e13737ce6b48412/lib/collection/src/model_testing/verify.rs).
- SurrealDB [transaction API](https://github.com/surrealdb/surrealdb/blob/9d9a5b0693e499e0d030cac6b618062ec02cd2bc/surrealdb/core/src/kvs/api.rs), [transaction builder](https://github.com/surrealdb/surrealdb/blob/9d9a5b0693e499e0d030cac6b618062ec02cd2bc/surrealdb/core/src/kvs/ds.rs), [version-scope operator](https://github.com/surrealdb/surrealdb/blob/9d9a5b0693e499e0d030cac6b618062ec02cd2bc/surrealdb/core/src/exec/operators/version_scope.rs), and [parser crate](https://github.com/surrealdb/surrealdb/blob/9d9a5b0693e499e0d030cac6b618062ec02cd2bc/surrealdb/parser/src/lib.rs).
- HelixDB [catalog snapshot](https://github.com/HelixDB/helix-db/blob/475e805cd864be7ff81c09ee6ba9a18ccc4d918b/crates/planner/src/catalog/snapshot.rs), [index lifecycle model](https://github.com/HelixDB/helix-db/blob/475e805cd864be7ff81c09ee6ba9a18ccc4d918b/crates/db/src/index_lifecycle/model.rs), [vector write transaction](https://github.com/HelixDB/helix-db/blob/475e805cd864be7ff81c09ee6ba9a18ccc4d918b/crates/db/src/search/vector/write_transaction.rs), and [production vector planner tests](https://github.com/HelixDB/helix-db/blob/475e805cd864be7ff81c09ee6ba9a18ccc4d918b/crates/db/tests/production_vector_planner.rs).

### Open systems used as bounded inspiration

| Source | Pattern worth adopting | Decision |
|---|---|---|
| [SlateDB `2a2fdd1` manifest schema](https://github.com/slatedb/slatedb/blob/2a2fdd146a95c0e2eb6cad84519b019f30a5ecfb/schemas/manifest.fbs) and [checkpoint RFC](https://github.com/slatedb/slatedb/blob/2a2fdd146a95c0e2eb6cad84519b019f30a5ecfb/rfcs/0004-checkpoints.md) | Immutable manifests, writer/compactor epochs, WAL replay bounds, snapshot minimum sequence, expiring checkpoint references, and GC pinning | Adapt the manifest/checkpoint invariants into native `vyrmKV`. Do not replace Fjall with SlateDB and call the native-engine work complete. |
| [Lance `6bad378` transaction specification](https://github.com/lance-format/lance/blob/6bad378f768e37dd87f993471bbee05005f27868/docs/src/format/table/transaction.md) and [commit boundary](https://github.com/lance-format/lance/blob/6bad378f768e37dd87f993471bbee05005f27868/rust/lance-table/src/io/commit.rs) | Immutable table versions, object-store commit handlers, operation-specific conflict classes, stable row identity, and explicit index coverage | Adapt operation-aware conflicts and manifest publication. Do not claim an object upload itself participates in the local KV transaction. |
| [Apache DataFusion architecture](https://datafusion.apache.org/contributor-guide/architecture.html) | Frontends lower to logical plans, logical rewrites precede physical lowering, physical rewrites account for resources, and results stream as batches | Mirror the boundary in `vyrmMX`. Delay a DataFusion dependency until relational/analytical benchmarks prove it is cheaper than a small native executor. |
| [Apache OpenDAL](https://github.com/apache/opendal/tree/013df6a9bf4e1183b539f60bb560f57ab1289f4e) | Capability-aware storage operators with composable retry, timeout, tracing, metrics, throttling, and concurrency layers | Use as an implementation candidate behind a small Vyrm object port. Never assume rename, conditional put, or listing order without checking backend capability. |
| [NVIDIA cuVS `eb7b342`](https://github.com/rapidsai/cuvs/tree/eb7b342922349b944824714b32ba2dad90d2bc4e) | GPU index construction/search with CPU-loadable HNSW serialization and multiple ANN families | Keep behind a feature-gated builder interface after CPU truth and recall baselines exist. Qdrant’s Vulkan path remains the portable reference. |
| [FastEmbed-rs `045d591`](https://github.com/Anush008/fastembed-rs/tree/045d59182819dd9145b762685e14c25868b6b0b3) | Synchronous local ONNX inference; caller-supplied model/tokenizer bytes; optional hub/cache features; dense, sparse, image, and multimodal families | Use only behind a provider-neutral job contract. The Vyrm local adapter disables hub/TLS features and hashes every supplied model component; cached downloads are never equivalent to an explicit deny-network build boundary. |
| [TiKV `877912f`](https://github.com/tikv/tikv/tree/877912ffc232caf257463d17f577b942b2a66e6c) | MVCC transaction scheduler, conflict latches, per-range Raft groups, snapshots, and explicit flow control | Study for the later cluster tier. Do not introduce distributed transactions before single-node semantics survive exhaustive crash points. |

The license/waiver ledger is evidence metadata, not a performance filter.
Qdrant and the pinned HelixDB snapshot carry Apache-2.0 files; the pinned
SurrealDB snapshot carries BSL 1.1 and is covered by the owner-reported waiver.
The additional systems above are used for architectural study only in this
phase.

## Canonical transaction contract

The first implementation target is a typed contract, not a query parser.
Illustrative names below are intentionally more precise than a generic
`begin/commit` pair:

```rust
pub struct ReadStamp {
    pub scope: ScopeId,
    pub schema_revision: u64,
    pub catalog_revision: u64,
    pub commit_cursor: u64,
    pub manifest_id: ManifestId,
}

pub enum DsMutation {
    Claim(Claim),
    Record(RuntimeRecord),
    Relation(RuntimeRelation),
    Event(RuntimeEvent),
    Vector(VectorValue),
    SeriesSample(SeriesSample),
    BlobRef(ObjectReference),
    Schema(RuntimeSchemaRegistry),
    Catalog(CatalogMutation),
}

pub struct DsTransaction {
    pub id: TransactionId,
    pub read: ReadStamp,
    pub durability: Durability,
    pub mutations: Vec<DsMutation>,
    pub audit: AuditIntent,
}
```

Documents and relational rows are typed record views, not separate canonical
copies. Graph edges are typed relations. Geospatial points/shapes are canonical
property values with spatial projections. Time-series data is a typed sample
identity plus time/value fields with time/series projections. A vector may be
canonical when supplied by the caller, or derived when produced by an embedding
model; the latter must include the source digest, model identity, model digest,
dimensions, normalization, and generation parameters.

One accepted transaction must atomically publish:

1. all canonical mutations;
2. schema and catalog revision changes;
3. the commit cursor and hash-chain entry;
4. synchronous integrity indexes required to validate the next write;
5. the accepted-operation audit record; and
6. outbox work describing asynchronous projection updates.

No projection may become authoritative merely because it is fast.

## Snapshots, manifests, and retention

`SnapshotHandle` must be a real owned resource:

```text
snapshot id
├─ scope
├─ commit cursor
├─ schema revision
├─ catalog revision
├─ manifest id
├─ projection generation map
├─ created/expires timestamps
└─ lease owner + retention policy
```

A snapshot pins the manifest and every immutable segment/object reachable from
it. Expired snapshots stop pinning data. Named checkpoints may be permanent but
must appear in operator inventory. GC may delete only an object that is absent
from the current manifest, every live snapshot/checkpoint, every clone/fork
reference, and the configured recovery grace window.

For a future distributed snapshot, `commit_cursor` becomes a scoped shard
vector. Vyrm must not invent a fake total cluster order when no protocol
provides one.

## Object storage and ACID

S3-compatible storage is an immutable-byte tier, not the transaction
coordinator. The safe publication protocol is:

1. encode the object and compute its content digest locally;
2. upload to a staging or final content-addressed key;
3. verify length/digest and record the backend version/ETag as evidence;
4. commit the `ObjectReference` and all related canonical mutations in
   `vyrmDS`/`vyrmKV`;
5. make the object reachable only through the committed manifest/reference;
6. reclaim abandoned uploads after a grace period.

This gives atomic **visibility** of the reference. It does not pretend the S3
PUT rolls back with a local transaction. Backend conditionals are selected by
declared capability; unsafe read-check-write is never silently substituted.

## Projection contract

Every scalar, graph, text, vector, series, or spatial index publishes a common
descriptor:

```rust
pub struct ProjectionStamp {
    pub id: ProjectionId,
    pub generation: u64,
    pub source_cursor: u64,
    pub config_digest: Digest,
    pub artifact_digest: Digest,
    pub state: ProjectionState,
}

pub enum ProjectionState {
    Building,
    Ready,
    Quarantined,
    Retiring,
}
```

The query planner compares the requested snapshot with this stamp. It may:

- use the projection when generation/configuration match and its source cursor
  covers the snapshot;
- combine indexed coverage with an exact scan of the uncovered delta;
- wait for a bounded refresh when explicitly requested;
- use an exact fallback; or
- deny with a typed freshness/capability differential.

It may not silently serve stale derived state. Approximate search must return
its index generation, coverage cursor, algorithm, parameters, and exact-rerank
policy in the observable plan.

## `vyrmQL` and `vyrmMX`

The language should begin small and composable:

```text
FROM record:document
AT VALID $valid_at KNOWN $cursor
WHERE project = $project AND status = "open"
TRAVERSE OUT relation:depends_on DEPTH 1..3
NEAREST embedding TO EMBED($query) LIMIT 20
PROJECT id, title, score, path
EXPLAIN CONTRACT
```

The exact syntax is not frozen. The execution boundary is:

```text
source text / typed SDK
  -> parsed AST
  -> bound typed logical plan
  -> policy and capability differential
  -> optimized logical plan
  -> physical operator DAG
  -> bounded result stream + plan evidence
```

Every operator declares required properties such as ordering, snapshot,
projection freshness, memory, approximation, network access, GPU availability,
and authorization. `EXPLAIN CONTRACT` reports what was requested, what each
candidate path could deliver, why paths were rejected, and what the chosen path
will actually provide.

## Vector, embedding, GPU, and edge profile

Vector work proceeds from an exact semantic oracle:

1. canonical dense/sparse/multivector types and dimension/metric validation;
2. exact CPU scan with deterministic top-k and filter semantics;
3. immutable vector segments and payload-aware planning;
4. HNSW plus exact reranking and recall measurement;
5. scalar/binary/product/TurboQuant experiments behind the same oracle;
6. GPU-assisted build, then optional GPU search, with CPU/GPU parity tests;
7. hybrid dense/sparse/text fusion only after each input score is observable.

Embedding in one client round trip must not hold a storage transaction open
while a model runs. The server validates and authorizes, embeds outside the
transaction, then opens a fresh transaction, revalidates the source digest, and
atomically commits source/vector/provenance. A changed source yields a conflict,
not a mismatched vector.

The edge build uses the same wire contracts with local filesystem objects,
exact or compact CPU indexes, optional local embedding, and no cluster runtime.
Feature gates must allow an offline binary without S3, GPU, Raft, or remote
model dependencies.

## Multi-AZ boundary

Multi-AZ is a separate execution tier, not a flag on the local store.

- Replicate shard/range state through a formally specified consensus and MVCC
  protocol.
- Keep an entity and its integrity-critical modalities colocated initially.
- Make per-shard serializable transactions the first distributed guarantee.
- Deny cross-shard writes until durable intents, recovery, idempotency, and a
  tested commit protocol exist.
- Record placement, replica health, consistency level, routing decision, and
  read stamp in audit/plan evidence.
- Treat vector indexes as rebuildable replica artifacts; transfer a grounded
  snapshot plus ordered WAL delta when that is cheaper than rebuilding.

SurrealDS/TAPIR-style leaderless transactions are research input, not the first
implementation milestone: the public material does not supply enough code and
operational evidence to substitute for a Vyrm protocol specification and fault
model.

## Audit contract

Every API action produces a versioned JSON envelope with:

- request/trace/transaction IDs and parent causality;
- actor, auth method, tenant/scope, node, and client;
- normalized operation and resource identities;
- snapshot/catalog/schema/projection generations;
- allow or deny decision plus policy differential;
- mutation cursor or query result summary;
- duration and CPU/I/O/network/GPU accounting;
- error class and retryability;
- previous/current audit digest.

Secrets, raw credentials, full prompts, and unrestricted document bodies are
excluded by default. Accepted writes include their audit record in the canonical
transaction. Denied writes and audited reads append to a dedicated hash-chained
audit lane; compliance mode fails closed if that lane cannot durably accept the
event. Archival may use immutable object storage with signed manifests and
retention locks.

## Implementation sequence and acceptance gates

### M0 — freeze contracts and evidence

- Publish this source ledger as the initial architecture decision record.
- Add serialized golden fixtures for read stamps, transactions, projection
  stamps, plans, and audit envelopes.
- Extend the `MemoryEngine` reference model before changing the physical store.

Acceptance: fixtures round-trip, unknown enum versions fail explicitly, and
the existing 118-test runtime suite remains unchanged.

Status (2026-08-19): **complete.** Data-runtime contract version 1 now
freezes read stamps, snapshot handles, read-bound transactions, projection
stamps, audit envelopes, and the complete typed reasoning lifecycle in
checked-in golden JSON. Digest/lease tampering and unknown contract versions
fail closed. Query-plan serialization remains correctly gated to M2 because no
query planner contract exists yet.

### M1 — transaction and snapshot port

- Introduce `ReadStamp`, `SnapshotHandle`, `DsTransaction`, typed conflicts,
  and snapshot leases.
- Adapt `Store` and `MemoryEngine` behind the new port while retaining current
  `Engine` callers.
- Add retention pins and snapshot inventory without deleting anything.

Acceptance: randomized backend differential; concurrent same/different-key
writes; read-your-writes; repeatable scans across pages; lease expiry; and no
cursor/hash/schema regression.

Status (2026-08-19): **complete.** MemoryEngine and Fjall share the
snapshot/transaction port. Tests prove a snapshot never reads beyond its
captured hash-chain head, leases expire and release deterministically, Fjall's
lease catalog survives restart, and two writers from one read stamp yield one
commit plus one explicit CAS conflict. A deterministic 64-write mixed-scope
differential produces identical stamps and replay pages. Stamped transaction
reads reject forged schema state and expose prospective record, relation,
event, claim, and schema mutations without changing committed evidence. The
conflict matrix proves the declared global-serializable policy for both same
and disjoint identities, repeatable scans agree across page sizes, and every
live lease produces a stable logical retention pin. Native physical
manifest/segment attachment remains correctly assigned to M3.

### M2 — `vyrmQL` algebra and `vyrmMX` reference executor

- Create parser/AST, catalog binder, logical plan, physical operators, streaming
  results, `EXPLAIN CONTRACT`, and budgets.
- Support typed record/claim/relation/event reads and bitemporal selectors first.

Acceptance: parser corpus and fuzzing, AST golden files, equivalent typed-SDK
and text plans, deterministic plan diagnostics, and result differentials against
direct current APIs.

Status (2026-08-19): **complete.** `vyrm-ql` owns a canonical, dependency-light
AST/parser with mandatory `VALID` and `KNOWN` selectors, scalar parameters,
record/relation/event/claim sources, corpus/negative/mutation-fuzz tests, and
checked-in golden vectors. `vyrm-mx` captures an immutable catalog/read stamp,
binds types and fields against schema history, emits content-addressed logical
and physical plans, and executes deterministic budgeted batches through the
shared `Engine` port. `EXPLAIN CONTRACT` records exactness, ordering,
authorization scope, resource needs, and why the authoritative log path won
while an ungrounded projection was rejected. Memory/Fjall/native, typed/text,
and direct graph API differentials are green. Connectome's Query Lab renders the
same plan contract beside its rows. The reference path deliberately contains
no ANN, embedding, network, or GPU behavior; those remain gated to M5.

### M3 — native `vyrmKV`

- Implement checksummed WAL records, MVCC sequence assignment, memtables,
  immutable segments, manifests, recovery, compaction, checkpoints, and GC.
- Keep Fjall as the measured compatibility oracle until the native engine wins
  every correctness gate and meets or beats agreed performance thresholds.

Acceptance: crash at every durable boundary; torn/truncated/corrupt WAL cases;
manifest CAS races; compaction with pinned snapshots; disk-full behavior;
recovery idempotency; reference differential; latency/throughput/RSS benchmarks.

### M4 — objects and unified model mutations

**Local executable gate complete (2026-08-19).** Canonical contracts,
three-engine atomic persistence, deterministic outbox/audit, idempotent retry,
local object publication/quarantine/reclamation, and the S3-compatible semantic
differential are implemented. A named cloud endpoint still requires transport
certification, and automatic object GC remains gated on physical retention-pin
reachability. See `vyrmds-object-contract.md`.

- Add content-addressed local and S3-compatible object adapters.
- Add vector, series, geo, and blob-reference mutation contracts.
- Publish object references, outbox work, and audit atomically.

Acceptance: failure injection before/after every upload and commit step; orphan
reclamation; missing/corrupt object quarantine; transaction rollback across all
canonical mutation families; local/S3 adapter differential.

### M5 — vector/search subsystem

Local reference gate passed on 2026-08-19. See `vyrm-vector-search.md` for the
frozen semantics, retained matrix, and the compact-artifact/SIMD/GPU limits that
remain before competitive proof.

- Ship exact dense/sparse/multivector search, filtered planning, immutable
  segments, HNSW, reranking, quantization experiments, lifecycle generations,
  and coverage-aware query planning.
- Incorporate only reviewed Qdrant/Helix code with file-level provenance entries.

Acceptance: exact oracle differential, recall/latency/memory/index-build matrix,
filter cardinality cases, delete/update/reopen/compaction soak, stale generation
denial, and deterministic model-based mutation traces.

### M6 — embedding, GPU, and edge

- Local kernel gate passed on 2026-08-19. Provider-neutral jobs bind source,
  model, backend, network policy, and original read stamp; source changes are
  checked both around inference and at commit CAS.
- Compact dense v1 separates canonical metadata from aligned raw values and
  supports verified owned/mmap reads plus scalar/AVX2 differential.
- A feature-gated accelerator boundary verifies untrusted builder output
  against deterministic CPU bytes and makes fallback explicit.
- `vyrm-edge` packages one-call local embedding/search with no networking stack;
  optional FastEmbed accepts caller-supplied ONNX/tokenizer bytes only.

Acceptance: source-change conflict during inference; model/dimension mismatch;
CPU/GPU semantic and recall parity; fallback after GPU failure; offline startup;
binary/RSS budgets; no network access in edge tests.

Local acceptance is green for the source/model/CAS, scalar/AVX2, adversarial
accelerator-output/fallback, startup, binary/RSS, and dependency boundaries.
Physical-GPU and real-model quality certification remain explicitly external
hardware/model evidence rather than being inferred from the adapter tests. See
`vyrm-embedding-edge.md`.

### M7 — cluster and Multi-AZ

- Specify shard placement, replication, consistency, snapshot vectors,
  transfer, resharding, and cross-shard transaction policy before code.
- Start with per-shard consensus and snapshot-plus-WAL-delta recovery.

The first protocol/simulation slice landed on 2026-08-19. `vyrm-cluster`
provides canonical contracts and a deterministic single-term/per-shard quorum
model covering all listed fault classes. Cross-shard writes fail closed. This
does not yet implement elections, reconfiguration, networking, or production
consensus and therefore does not close M7. See `vyrm-cluster-m7.md`.

Acceptance: deterministic simulation/model checking; partition, delay,
duplication, reorder, crash, clock-skew, and disk-loss scenarios; linearizable
metadata; declared transaction isolation; no acknowledged-write loss within the
configured fault tolerance.

### M8 — workbench and continuous regression

- Visualize transaction spans, snapshot pins, planner alternatives, projection
  coverage, index generations, object reachability, replication, and audit
  chains in Connectome.
- Add benchmark histories and regression budgets to CI.

Acceptance: every visual value links to its source cursor/snapshot/manifest;
freeze/rewind uses real persisted events; no synthetic UI state is presented as
database truth; performance and recall regressions fail CI at explicit bounds.

## Immediate next implementation slice

M2 is closed and the first M3 storage lifecycle is implemented beside—not
underneath—the compatibility adapter. WAL, atomic mutation, immutable segment,
manifest, and `CURRENT` formats are frozen; torn/corrupt recovery, MVCC,
checkpoints, manifest CAS, segment flush, WAL rotation, and reopen at the
manifest replay boundary are covered. The native `Engine` adapter and
Memory/Fjall/native semantic/query differential are green. Snapshot-aware
compaction/GC, physical lease pins, and crash/storage-full recovery matrices are
also green. The isolated five-trial comparative baseline and strict
equal-or-better policy are frozen in `docs/vyrmkv-benchmark.md`. Disk-resident
segment-v3 blocks, optimized SHA-256, streaming immutable reads, self-serving
native sequence values, and fresh steady-state probes moved every measured cell
green while preserving correctness. The local M3 gap is closed; a nine-trial
small-batch/standard/read-heavy/sustained matrix also passes. Next reproduce it
under remote CI. The local mixed update/delete gate now passes 20,000 operations
with 10 reopens and 8 compactions against Fjall and an independent model. The
existing-store rehearsal copies all 18 canonical keyspaces through an
authenticated streaming archive, verifies invisible native staging, resumes at
seven durable/rename boundaries, retains Fjall, and denies rollback after
native divergence. See `docs/vyrmkv-migration.md` and
`docs/evidence/m4-storage-mixed-soak.json`.
The runtime default decision is implemented through `PersistentEngine`: missing
paths become native, native `CURRENT` paths reopen native, and existing
non-native paths remain compatibility-bound until the explicit migration.
Fjall source removal waits for remote reproduction and a compatibility-
retirement release. Typed runtime deletion remains a separate referential-
integrity contract. Do not infer SurrealDB or Qdrant superiority from this
Fjall-scoped result.
