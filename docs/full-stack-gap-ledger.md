# RRFlow/RRD full-stack capability ledger

**Status:** authoritative sequencing baseline as of 2026-08-23
**Upstream inventories:**
[SurrealDB](surrealdb-capability-inventory.md) first, then
[Qdrant](qdrant-capability-inventory.md)

## Why this ledger exists

The project previously optimized a bounded VyrmKV/Fjall workload and added a
bounded SurrealDB 3.0.5 differential before enumerating either competitor's
complete product surface. That was the wrong order. The measurements are valid
for their fixtures, but they cannot establish a full database, vector product,
or enterprise runtime. This ledger replaces benchmark-led sequencing with
capability- and dependency-led sequencing.

No future README or status claim may promote a feature merely because a type,
prototype, UI card, or design exists. Promotion requires:

1. an explicit owner and public contract;
2. persistent executable behavior where persistence is relevant;
3. failure, restart, compatibility, and security tests proportional to risk;
4. observable operational state;
5. a retained evidence artifact or deterministic CI gate;
6. an honest limitation and edition/deployment boundary.

## Product ownership — stop putting the entire stack inside VyrmKV

```text
Clyffy
└── RRFlow
    ├── RRO: orchestration, accounts, entitlements, policy and estate control
    │   ├── Automaton: workflow execution/integration
    │   └── LFG: JIT context encoding/routing
    ├── RRD: public durable AI data/runtime product
    │   ├── VyrmQL + VyrmMX: query language, planner and execution
    │   ├── vector/inference/realtime/security/API services
    │   └── Vyrm: internal native LSM/MVCC persistence engine
    └── Connectome Panel: local and enterprise operator/developer client
```

This boundary is mandatory:

- **Vyrm/VyrmKV** owns physical durability, MVCC, WAL, manifests, segments,
  compaction, checkpoints, snapshots, recovery, migration and storage evidence.
- **RRD** owns multi-model semantics, query, vector, inference, transactions,
  live feeds, auth enforcement, protocols and SDK contracts.
- **RRO/RRFlow control plane** owns accounts, entitlements, organisations,
  estates, desired state, deployment reconciliation, upgrades, backups,
  placement, fleet health and policy.
- **Connectome** owns operator/developer interaction and visualization. It
  consumes authoritative APIs; it is not the source of estate truth.
- **Automaton/LFG** remain external runtime/context components with typed
  integration boundaries. They do not become hidden database behavior.

## Current truth

### What “true persistence” means today

Vyrm has **real local alpha persistence**, not a wrapper: native WAL and
recovery, MVCC, authenticated manifests/segments, atomic multi-family runtime
commits, snapshots/checkpoints, compaction/GC, crash injection, reopen tests,
and a staged Fjall migration exist. That is substantial.

It is not yet a complete persistent product because these surrounding gates
remain open:

- portable full-database logical export/import;
- version-to-version upgrade and resumable general data/schema migration;
- operator-scheduled backups, retention, restore and restore verification;
- long-duration, disk-full and independent-host qualification;
- estate-owned backup identities and recovery objectives;
- authenticated remote client/session transaction boundaries;
- public stable SDK/API compatibility.

### What “estate” means

An estate is not a UI array of instances. It is a persistent, reconciled
control-plane model with at least:

- organisations, accounts, users, service principals and entitlements;
- products/projects, environments and isolated RRFlow/RRD instances;
- desired and observed versions/configuration/capabilities;
- nodes, shards, zones, placement, endpoints and certificate identities;
- deployment, scale, upgrade, backup, restore and deletion jobs;
- assignments/tasks and last meaningful runtime activity;
- health heartbeats, resource/latency/error summaries and alerts;
- secret **references** (never secret material in the ordinary catalog);
- append-only lifecycle/audit history and idempotent reconciliation receipts;
- policy and authorization for every mutation.

Connectome's current `EstateView` is a useful read-model prototype. It is not
this control plane.

### What “SDK” means

Workspace crates do not constitute a supported SDK. An SDK requires a stable,
versioned client contract for endpoint discovery, authentication, sessions,
transactions, query/CRUD, vector/search, live feeds, retries/idempotency,
timeouts/cancellation, typed errors, capability negotiation, compatibility and
generated/handwritten conformance tests.

## Dependency-ordered delivery plan

### F0 — freeze the product contract and conformance vocabulary — **initial contract complete**

**Deliverables**

- Versioned RRD service contract and capability handshake.
- Canonical resource IDs for organisation/estate/project/instance/node/shard,
  collection/table/record, transaction, snapshot/backup and operation.
- Stable error envelope, idempotency key and request/correlation identity.
- Status vocabulary used by code, API, Connectome and documentation.
- Conformance corpus shared by embedded, local-server and distributed modes.

**Exit gate:** the same fixtures can be consumed without importing internal
Rust types. No SDK or control plane is built against an unstable private API.

**Landed 2026-08-23:** dependency-light `rrd-contract` v1 freezes protocol and
capability negotiation; canonical typed resource paths; request, operation,
idempotency and deadline coordinates; mutation idempotency requirements;
response framing; and stable error codes. Its checked-in JSON fixture and
adversarial decode tests pass without depending on any internal Vyrm crate.
See [`rrd-public-contract.md`](rrd-public-contract.md). Further endpoint payload
schemas extend this versioned boundary in the slice that implements them.

### F1 — close Vyrm persistence as an operable storage product

**Status:** in progress. The current persistence-format changes passed their
format, migration, crash, corruption, disk-full, soak, and lint gate. Logical
archive v1 consistency, integrity, and restore invariants are frozen in
[`rrd-logical-archive.md`](rrd-logical-archive.md).

**Landed 2026-08-23:** backend-independent logical export reconstructs and
validates original runtime commits, correlates their claim mutations with the
independent claim sequence, rejects unstable source watermarks, and writes a
content-authenticated framed archive. Restore validates before mutation,
replays through production Engine paths into hidden staging, compares both
watermarks, flushes, reopens, verifies again, and only then publishes an absent
target. An authenticated catalogue retains content-addressed archives and
declares claims/runtime included, objects referenced-only, projections
rebuild-required, telemetry/leases excluded, and application completeness
false. Library and CLI corruption, truncation, retry, exact-replay, catalogue,
and selected-backup restore tests pass. Object closure, retention policy,
signing/encryption, streaming source spill, and general cross-version migration
remain before the F1 exit gate.

**Native migration ledger landed 2026-08-23:** the only admitted application
format edge is the exact successor TextV1 → `VYRSK002` TagV2. Its authenticated
ledger resumes export/import/verify/two-rename cutover, preserves all 18
keyspaces, retains the predecessor and archive, rejects a source modified after
export, and is idempotent after completion. Fault injection covers every
durable and rename boundary. The first cross-version matrix also proves logical
archive recovery from TextV1 into a fresh current-format root. Broader released
binary-version rows and object-complete backup closure remain before F1 exits.

**Deliverables**

- Finish and verify manifest-v2/application-format migration currently in the
  worktree without changing the bounded benchmark claims.
- Logical archive format covering schema, canonical data, audit coordinates,
  object references and vector catalog metadata.
- Streaming export/import with authenticated checksums and resumable receipts.
- Backup catalogue, retention pins, restore-to-new-root, restore verification
  and explicit recovery objectives.
- General migration ledger with exact-successor versions, restart resume and
  rollback/cutover rules.
- Long-soak, power-loss/disk-full/corruption, cross-version and independent-host
  test matrices.
- Background maintenance scheduling with backpressure, observability and
  bounded resource policies.

**Exit gate:** a clean machine can restore a retained archive/snapshot into a
new root, verify it, reopen it on the target version, and produce identical
canonical reads and audit roots.

### F2 — build the real RRD server and session/transaction boundary

**Status:** dependency contract frozen in
[`rrd-server-v1.md`](rrd-server-v1.md); persistent coordinator and async
loopback HTTP alpha implemented. The contract keeps `vyrmd` as an MCP adapter,
requires public payload types before handlers, denies non-loopback exposure
before F4, and makes accepted-request idempotency durable rather than
process-local.

**Implementation progress 2026-08-23:** `rrd-contract` now owns bounded session
limits/leases, transaction leases/states, the first public claim mutation, and
commit receipts without importing private Vyrm types. Engine now provides an
atomic idempotent claim append: client key, operation SHA-256, and accepted
sequence receipt share the authoritative claim transaction in the existing
metadata keyspace. Memory, Fjall compatibility, and native engines pass the
same collision/replay contract, and both persistent engines replay after
restart without advancing sequence.

The Engine control plane now atomically materializes compare-and-swap state and
appends a monotonically sequenced, SHA-256-chained, replayable journal entry.
The RRD coordinator uses it for persistent session create/renew/close/expiry,
transaction begin/prepare/commit/abort/expiry, quota enforcement, and bounded
idle/absolute leases. Only token hashes reach storage. A prepared commit binds
the only permitted key/digest before claim acceptance, so restart, lease
expiry, collision, and post-claim/pre-terminal recovery cannot duplicate a
claim.

`rrd-server` is now an Axum/Tokio process with versioned envelopes, liveness,
readiness, capability negotiation, one-MiB body denial, canonical typed
operation digests, claim preview/commit, JSON tracing, graceful shutdown, and
fail-closed loopback binding. Its real-socket tests cover rotation, expiry,
quota, abort/close, malformed input, deadline precheck, disconnect/retry,
concurrent commit convergence, restart replay, secret-file permissions, and
binary remote-bind denial. These transport leases still do not constitute F4
user authentication or comprehensive audit. Cancellation, generalized
read-your-writes, CRUD/schema/vector/snapshot administration, result/time
bounds, metrics, and released-version client qualification keep F2 open.

The public service now also exposes the first F6 breadth path through
`POST /v1/query`: a transport-neutral, strictly bounded `ExecuteQuery`
contract is authenticated against the persistent session, restricted to the
server's exact instance scope, and executed by the real VyrmQL parser and
VyrmMX catalogue/binder/planner/executor. The response preserves typed values,
read manifest, cursor, schema revision, plan candidates, exactness/order/auth
contracts, and execution evidence. A real-socket test proves unauthenticated
and wrong-scope denial plus an exact persisted-record result. This does not yet
provide mutating VyrmQL, multi-model public transactions, live queries, or
durable service-query spans.

**Deliverables**

- One async server with health/readiness/capability endpoints.
- Versioned HTTP API first; protobuf/gRPC when the resource contract is frozen.
- Authenticated sessions with expiry and bounded server resources.
- Client-owned begin/read/write/commit/rollback transactions.
- Query cancellation, deadlines, request limits and idempotent mutation retry.
- CRUD/schema/vector/snapshot administration APIs mapped to the same internal
  contracts used by embedded Rust.
- Structured logs, metrics and trace export from the first endpoint.

**Exit gate:** an out-of-process black-box test proves crash/reconnect,
transaction atomicity, cancellation, idempotency and version negotiation.

### F3 — persist the estate and run a reconciler

**Deliverables**

- Durable estate schema described above, stored through RRD rather than UI
  process memory.
- Desired-state/observed-state reconciliation with leases and idempotent jobs.
- Local process driver first: create/start/stop/restart/upgrade/delete isolated
  instances without shell-string execution.
- Heartbeat/activity ingestion and stale/idle/neglected-project derivations.
- Deployment and upgrade state machines with failure recovery.
- Backup/restore jobs and per-instance retention policy.
- Read-only Connectome estate API followed by explicitly authorized mutations.

**Exit gate:** kill the control-plane process at every transition boundary;
restart must converge to the same desired state without duplicate destructive
work or loss of operation history.

**Implementation progress 2026-08-23:** the first persistent authority slice is
landed in `rrd-estate` and frozen in
[`estate-control-v1.md`](estate-control-v1.md). A bounded per-estate aggregate
uses Vyrm control-state CAS plus its authenticated journal to advance monotonic
desired generations, observed evidence, idempotency bindings, operation state,
fenced lease epochs, append-only receipts, heartbeat/runtime evidence and
derived activity. Memory/native tests prove key rebinding denial, stale-worker
fencing, lease-epoch recovery and reopen/hash-chain survival. The next slice
adds a typed reconciler that advances only one durable boundary per call.
Native-engine reopen tests cover lease, prepared, an external effect whose
acknowledgement is lost, applied, observed and completed boundaries. The stable
operation ID deduplicates the retried effect; takeover preserves prepared work,
and newer desired generations terminally supersede unfinished stale work. This
does **not** complete F3: deployment/upgrade/restore state machines, packaging,
retention policy, and bounded process-log retention remain open.
The read-only projection is now wired:
`rrd-contract::EstateSnapshot` prevents the internal aggregate becoming the
wire contract, RRD requires a live session plus matching estate/instance path,
and Connectome's `/api/estate` and workbench render desired→observed generation,
activity, and operation state. Missing authority is explicitly labeled as a
synthetic fallback.

The first local process driver is also implemented and specified in
[`local-process-driver-v1.md`](local-process-driver-v1.md). An operator-trusted
catalogue binds canonical executable SHA-256 and typed argument sources; launch
uses no shell and clears inherited environment. Durable process records bind
PID/start-time/executable plus operation/deployment/configuration identity, and
signals fail closed on mismatch. Current-host integration launches the real RRD
server, reopens the controller objects at every state-machine step, proves start
replay preserves PID, actually stops the child for a new desired generation,
and retains its data. A black-box harness now kills a separate one-step
controller after every start/stop transition and in the external-effect-before-
`applied` gap; replay preserves the started PID or converges after the stop
already occurred. Managed RRD children drain through paired durable request and
completion files with a bounded timeout, PID/start/executable reauthentication,
and forced fallback. Per-instance stdout/stderr logs retain startup evidence.
The native matrix passes this complete sequence and strict clippy on Linux,
Windows and macOS. A native-qualified generator now produces the complete
trusted catalogue from the installed sibling RRD server without hand-authored
paths or digests. Installer/service packaging, operator-policy provisioning,
bounded log retention, and the remaining deployment/upgrade/restore jobs are
still required for F3.

The first mutation boundary is intentionally local and specified in
[`local-estate-authorization-v1.md`](local-estate-authorization-v1.md). Exact
operator/key/time/estate/action policy is checked before storage opens;
`rrd-estate-admin` then uses the existing CAS state machine and authenticated
journal for replay-safe create, desired-state mutation, and quiesced backup
scheduling. Durable per-instance backup jobs have fenced leases and prepared,
completed, and failed receipts. The one-step `rrd-backup-controller` fixes
source and catalogue paths under one canonical state root, denies a retained
process record before mutation, authenticates the F1 catalogue before and after
creation, and converges across a process kill after the archive effect but
before its completion receipt. This removes hand-written direct database
mutation from the local workflow without claiming restore, retention, F4 remote
authentication, or organization-wide authorization.

### F4 — establish security and governance before remote management

**Deliverables**

- Local users/service principals, password/key handling and short-lived tokens.
- Organisation/estate/project/instance RBAC plus scoped ABAC conditions.
- Row/field policy where RRD exposes application data directly.
- TLS client endpoints and mTLS node/control-plane identities.
- Secret-provider interface; catalogs retain references and rotations only.
- Deny-by-default capability policy for query, network, files, extensions,
  inference, administration and arbitrary execution.
- Comprehensive JSON audit for query, transaction, CRUD, schema, collection,
  backup, auth, estate and API actions, with rotation/redaction/hash chaining.
- Rate/resource limits and strict-mode safeguards.

**Exit gate:** authorization differential and audit completeness tests cover
every public mutation and prove denied actions do not partially apply.

### F5 — ship supported SDKs from one schema

**Order:** Rust embedded/client → TypeScript → Python → Go → Java/.NET.

**Deliverables**

- Generated protocol types plus intentional ergonomic layers.
- Auth/session, transaction, VyrmQL, CRUD/schema, vector/query, live feed,
  snapshot/backup and estate clients.
- Async, timeout, cancellation, retry and idempotency behavior.
- Capability/version negotiation and typed error mapping.
- Hermetic conformance suite run against local server and supported distributed
  deployment; examples and API reference generated in CI.

**Exit gate:** every supported SDK passes the same black-box semantic fixtures;
unsupported server/client version pairs fail explicitly.

### F6 — complete the Surreal-class RRD data/query surface

**Deliverables**

- Mutating VyrmQL and multi-statement transactions.
- General document/record/edge CRUD and schemafull/schemaless policy.
- Relational links, recursive graph traversal, series and geospatial operators.
- Ordinary/compound/unique/count, spatial and full-text indexes.
- Full-text analyzers and a replacement lexical stack measured against the old
  BM25 path; this is where the later LFG/TurboQuant retrieval integration lands.
- Planner cardinality/cost evidence, `EXPLAIN ANALYZE`, streaming batches and
  bounded execution.
- Push live subscriptions with backpressure/reconnect plus retained changefeed
  replay from exact cursors.
- User-defined triggers/functions only after capability and audit gates exist.

**Exit gate:** multi-model and live-query differentials cover semantics,
failure, restart, permissions and bounded performance. SurrealDB comparisons
remain per-capability, never one synthetic “database score.”

### F7 — complete the Qdrant-class vector surface

**Deliverables**

- Collections/points/named-vector/payload administration.
- Compact mutable dense HNSW and sparse index lifecycle.
- Typed payload indexes and true one-stage filtered traversal; ACORN-quality
  restrictive-filter path evaluated on fixed selectivity corpora.
- Unified nearest/id/recommend/discover/context/scroll/group/facet/matrix,
  hybrid/prefetch/multistage/formula query algebra.
- Dense+sparse rank fusion and ColBERT late interaction without middleware.
- Actual TurboQuant: seeded rotation, global distribution-aware mapping,
  4/2/1.5/1-bit packing, asymmetric query scoring, SIMD, lifecycle and recall
  evidence. Keep scalar/binary/product methods separate.
- Per-structure pinned/cached/cold memory policy.
- Physical GPU indexing with CPU byte/semantic differential and fallback.
- Server/client inference for local and provider models with digest provenance.

**Exit gate:** exact-oracle correctness first, then recall/latency/memory/build/
update/recovery matrices on fixed hardware and corpora against current Qdrant.

### F8 — qualify distributed and Kubernetes operation

**Deliverables**

- Independent-host replication and failure evidence.
- Shard/replica administration, online transfer, rebalancing and resharding.
- Explicit read/write consistency modes and documented guarantees.
- Multi-AZ topology enforcement and zone-failure drills.
- Kubernetes CRDs/operator, safe rolling upgrades, disruption budgets, CSI
  backup/restore and secret/certificate rotation.
- Hybrid/private control-plane agents with outbound-only management option.
- Retained Prometheus/OTLP telemetry, alerts and capacity recommendations.

**Exit gate:** multi-node destructive fault campaigns demonstrate stated RPO,
RTO, availability and consistency; anything not demonstrated stays unclaimed.

### F9 — make Connectome the faithful developer/operator instrument

**Deliverables**

- Connection profiles for embedded/local/remote RRD instances.
- Estate, instance, node/shard, table/collection, schema, index, backup,
  security and audit workspaces backed by authoritative APIs.
- Existing prompt-flight/time-travel visuals joined to real query, storage,
  vector, inference, network and reconciliation spans.
- Freeze/rewind/fast-forward over retained events without inventing hidden
  reasoning or physical events that were never observed.
- Baseline cohorts and regression views for latency, tokens, retries, recall,
  memory, I/O, compaction, indexing and model/provider behavior.
- Layman summaries paired with full raw evidence and exact coordinates.

**Exit gate:** every visual datum links to a persisted event/metric or is
visibly labeled as a derived calculation; UI writes use the same auth/audit/
idempotency contracts as SDK clients.

## Competitive evidence policy

- SurrealDB and Qdrant versions, hashes, engine/configuration and edition must
  be pinned in every comparison.
- Compare capability to capability: persistence recovery, transaction
  semantics, graph traversal, live delivery, filtered ANN, quantization,
  ingestion, estate reconciliation, and so on.
- Correctness and recovery gates precede performance.
- Report every losing cell and resource tradeoff.
- Separate embedded library, local server, distributed, cloud and control-plane
  results; they are not interchangeable.
- “Beats SurrealDB/Qdrant” is prohibited unless an explicitly published matrix
  defines the bounded scope. A single fixture never becomes a blanket claim.

## Next executable slice

The next implementation slice is **F0 → F1**, not another benchmark tweak:

1. ~~freeze the public resource/error/idempotency vocabulary~~ — complete;
2. ~~finish verification of the current manifest/application-format migration~~ — complete;
3. ~~introduce the logical archive and backup catalogue contracts~~ — initial
   operable v1 complete with explicit partial-coverage declaration;
4. restore-to-new-root, same-version reopen, exact-successor native migration,
   and the first TextV1→current logical-recovery row are proven; expand the
   released-version matrix next;
5. then establish the first out-of-process RRD server boundary.

Estate types may be designed during F0, but remote provisioning is prohibited
until F2 authentication, idempotency, audit and recovery semantics exist.
