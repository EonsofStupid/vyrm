# RRFlow/RRD full-stack capability ledger

**Status:** authoritative sequencing baseline, revised 2026-08-25
**Upstream inventories:**
[SurrealDB](surrealdb-capability-inventory.md) first, then
[Qdrant](qdrant-capability-inventory.md)

## Why this ledger exists

The project previously optimized a bounded RRD LSM/Fjall workload and added a
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

## Product ownership — one RRFlow product and one RRD authority

```text
Clyffy
└── RRFlow
    ├── RRD: Reason Ready Daemon, the one durable engine/runtime authority
    │   ├── catalogue, transactions, security, audit and lifecycle truth
    │   ├── RRFlowQL plus shared planning/execution
    │   ├── document, relational, graph, vector, temporal, geo and AI state
    │   └── rrd-lsm and specialized physical operators behind one contract
    ├── rrflow: project CLI, preflight and exact command boundary
    ├── adapters/SDKs: daemon, embedded, provider and language surfaces
    └── Connectome: local and enterprise operator/developer client
```

This boundary is mandatory:

- **RRD** owns physical durability, the shared catalogue and read stamp,
  multi-model semantics, transactions, query, indexes, live feeds, AI runtime
  state, auth enforcement, audit, recovery, and its public protocol. Internal
  physical packages do not become independent engines or product authorities.
- **RRFlow's control plane** owns accounts, entitlements, organisations,
  estates, desired state, deployment reconciliation, upgrades, backups,
  placement, fleet health and policy.
- **Connectome** owns operator/developer interaction and visualization. It
  consumes authoritative APIs; it is not the source of estate truth.
- **Automaton/LFG** remain external runtime/context components with typed
  integration boundaries. They do not become hidden database behavior.

Existing RRFlow-named prose below is retained only where it describes historical
implementation evidence or durable-format migration input. It is not a target
product boundary. New contracts use RRFlow/RRD names.

## Master execution control board

The machine-readable scope and dependency graph is
[`rrflow.workplan.toml`](../rrflow.workplan.toml). This ledger explains it; it
does not maintain a second status authority. RRD persists the active plan
revision, work-item state, lifecycle events, evidence, and decisions. A box is
crossed only by the runtime's verified transition on the current plan/tree
revision, never because code or prose appears to exist.

Status at this checkpoint: **all gates below are open until G00 enforcement
revalidates existing evidence on the current tree.** Prior code and tests are
inputs to that validation, not permission to pre-check the board.

- [ ] **G00 — enforced work control:** strict work-plan contract; canonical
  lifecycle state machine; active-item planning/mutation gate; evidence-only
  verification; generated status surfaces.
- [ ] **G01 — `0.1.0` alpha identity and one-engine cohesion:** complete RRFlow/RRD rename;
  one `rrd-engine` authority; zero physical bypasses; one capability catalogue.
- [ ] **G02 — durable storage and recovery:** WAL/MVCC/LSM qualification;
  object-complete archives; backup/retention/restore; migrations; time travel;
  S3/tiered persistence.
- [ ] **G03 — unified multi-model query and realtime:** one schema/catalogue;
  concurrent CRUD/transactions; RRFlowQL; Arrow/DataFusion; scalar/full-text
  indexes; push live queries; governed triggers/functions.
- [ ] **G04 — vector, retrieval, compression, and inference:** collections and
  points; filtered persistent HNSW; sparse/hybrid/multivector/late interaction;
  TurboQuant; memory tiers; native inference; GPU qualification.
- [ ] **G05 — service, transactions, security, and audit:** embedded/local/
  remote parity; authenticated sessions and transactions; TLS and granular
  authorization; unavoidable comprehensive structured audit.
- [ ] **G06 — generated public surfaces:** one generated API/MCP/CLI/SDK/UI
  catalogue; complete task-level MCP; daemon MCP; CLI; Rust, TypeScript,
  Python, Go-edge, Java, and .NET SDK qualification.
- [ ] **G07 — provider-neutral Clyffy enforcement:** deterministic topology and
  attunement; task-specific preflight; architecture/pattern gate; exact-argv
  proxy; provider/local adapter conformance and honest coverage.
- [ ] **G08 — estate, distributed, and Kubernetes:** persistent estate and
  reconciler; RRD-backed consensus data plane; Multi-AZ and hybrid-cloud
  qualification.
- [ ] **G09 — Connectome:** native release artifacts; remove SurrealDB aliases
  and temporary UI; generated RRD connection path; complete data/operations
  workspaces; replay/NodeTrace; live enforced-work-plan view.
- [ ] **G10 — persistent end-to-end qualification:** 14 runtime-control and at
  least 10 cohesive-data reopen scenarios at 100%; real process/surface matrix;
  destructive recovery and security faults.
- [ ] **G11 — bounded competitive evidence:** pinned SurrealDB, Qdrant, and
  Fjall differentials; correctness and recovery before performance; public raw
  results; every loss disclosed.
- [ ] **G12 — firm-alpha release:** repository and architecture gates; Linux,
  macOS/ARM, Windows, edge, and browser CI; signed/package install/update/
  rollback; one consistent public status.

### Runtime transition rules

1. `workplan.loaded` validates the checked-in V1 plan and persists its digest.
2. `workitem.activated` is allowed only when every dependency is verified.
3. `plan.recorded` must bind the active work item, source-tree digest,
   attunement/preflight receipts, architecture decision, and verification plan.
4. `tool.proposed` and every mutating command must bind that same chain.
5. `tool.completed` records an observation; it never completes the item.
6. `verification.completed` carries exact current-revision evidence.
7. `workitem.verified` is the only transition that crosses a box. It fails
   closed if any required evidence is missing, failed, stale, skipped, or from
   another platform/revision.
8. Scope changes require a reviewed plan revision event. They cannot be hidden
   in an implementation diff.
9. RRD reopen must replay the identical plan aggregate and consumed permits.
10. CLI, MCP, hooks, SDKs, and Connectome project this state; none may edit a
    checkbox or manufacture completion independently.

## Current truth

### What “true persistence” means today

RRD has **real local alpha persistence**, not a wrapper: its native `rrd-lsm`
path provides WAL and recovery, MVCC, authenticated manifests/segments, atomic
multi-family runtime commits, snapshots/checkpoints, compaction/GC, crash
injection, reopen tests, and a staged Fjall migration. Native writer ownership
is non-blocking and fails closed when another process owns the root. Ordinary
WAL publication is qualified immediately before append and immediately after
durable sync for crash and storage-full outcomes; a post-sync in-process handle
must reopen before doing more work. Read-stamped transactions persist the exact
`ReadStamp` inside the same atomic audit-bearing batch on native RRD, the Fjall
compatibility oracle, and the in-memory conformance engine.

The single-node process boundary is now project-bound locally: an explicit
provisioning action creates or verifies one immutable instance identity,
`rrd-server` discovers that project root and opens only its canonical
`.rrflow/rrd` store, and RRD persists the canonical project/store binding so a
normal restart cannot silently rebind copied or moved data. The estate process
catalogue and generated Kubernetes workload use the same initialize-then-serve
contract. This is D1 evidence, not distributed or managed-cloud qualification.

It is not yet a complete persistent product because these surrounding gates
remain open: live-provider endpoint certification; long-duration, disk-full,
and independent-host qualification; authenticated remote client/session
transaction boundaries; distributed placement/failover; and public stable
SDK/API compatibility. Object-complete archives, estate-owned recovery policy,
exact-successor format migration, audited structural rollback, and the
provider-neutral tiered-persistence contract are executable foundation rather
than remaining placeholders.

### Frozen cohesive breadth requirements — 2026-08-25

These requirements are product gates and shared-catalogue rows. They may not be
claimed from a UI card, type, or isolated physical component:

| Requirement | Current executable truth | Missing gate |
|---|---|---|
| Concurrent document, relational, native graph-edge, vector, event, time-series and geo storage | The typed RRD transaction can atomically commit these models; `rrflow_data_commit` exercises that path with exact authorization and reopen replay | Complete public administration, indexes, query/search parity and cross-model transaction qualification |
| Real-time live queries and changefeeds | Bounded engine/HTTP/MCP poll, read and follow operations exist | Push/subscription transport, backpressure, retention and distributed ordering qualification |
| Vector and full-text retrieval | Dense/sparse/multi-dense storage plus MCP collection, point and exact-search paths exist; HNSW/TurboQuant engine artifacts also exist | Collection-bound approximate serving, full-text engine/index implementation, hybrid planner and production recall/latency evidence |
| Time travel and rollback | RRFlowQL binds schema, source watermarks, claims, index selection and authoritative replay to one valid-time/known-at coordinate across memory, Fjall, native and reopen; `rrflow_data_rollback` appends idempotent record/relation compensations plus immutable evidence through the normal audited transaction | Extend selected-state rollback beyond records/relations without rewriting append-only event, series, object, schema, vector, geo or security-audit history; qualify the complete public-surface matrix |
| Object and tiered persistence | Authenticated capability admission, resumable/checksummed/conditional S3 multipart archive and artifact transfer, version-bound ranged recovery, and native mmap/io_uring/bounded segment I/O share the existing RRD storage authority with measured physical/cache separation | Certify named provider transports/endpoints and extend qualified hot/cold policy to vector artifacts under G04 |
| In-memory, embedded, single-node, browser/WASM, mobile/edge and distributed modes | Embedded persistent and single-node daemon paths exist; underlying test/memory and experimental edge/distributed pieces exist | One logical conformance suite and platform-specific durability/security/resource qualification for every advertised mode |
| Autonomous agent memory | Bitemporal facts, exact recall, context, routing, reasoning events, attunement and lifecycle enforcement exist | Hybrid recall, reflection, ingestion, semantic code search, governed retirement and evaluation evidence under one RRD authority |
| Managed cloud | Kubernetes and estate foundations exist | Tenant control plane, autoscaling, granular capability administration, secure outbound-network policy, upgrades, recovery and Multi-AZ qualification |

The engine-owned product capability catalogue records unfinished full-text,
deployment, autonomous-memory, granular-security and managed-cloud rows as
`planned` across every surface. Historical rollback is available through the
engine-owned runtime tool and its generic HTTP/MCP/Connectome invocation
surfaces; CLI remains explicitly unavailable rather than receiving a false
entrypoint.

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
adversarial decode tests pass without depending on any internal RRFlow crate.
See [`rrd-public-contract.md`](rrd-public-contract.md). Further endpoint payload
schemas extend this versioned boundary in the slice that implements them.

### F1 — close RRD persistence as an operable storage product

**Status:** in progress. The current persistence-format changes passed their
format, migration, crash, corruption, disk-full, soak, and lint gate. Logical
archive v1 consistency, integrity, and restore invariants are frozen in
[`rrd-logical-archive.md`](rrd-logical-archive.md).

**G02-W02 candidate 2026-08-27:** backend-independent logical export now walks
fixed claim/runtime pages without whole-log materialization, preserves original
transaction audit envelopes and audit heads, and resumes export/restore through
checksummed receipts that reconcile the durable-action/receipt crash window.
Export also resumes after the authenticated footer is durable but before final
publication by validating and publishing the complete private archive.
The application-complete backup identity binds the archive, immutable object
payload closure, and vector/index catalogue manifest. Focused tests cover more
than ten persistent scenarios, multi-page logs, all canonical runtime families,
exact audit/read stamps, all three export durability boundaries, both restore
receipt boundaries, tampering, source drift, reopen, object bytes, and catalogue
state. The complete local 13-command matrix and exact cross-platform CI run
`33184505371` are green; G02-W02 is verified.

**G02-W03 candidate 2026-08-28:** the estate now owns revisioned RPO/RTO and
retention policy, policy-bound backup jobs, authenticated recovery-point
identity, policy/explicit pins, deterministic retention decisions, fenced
prune intents, non-deleting prune history, and measured restore evidence.
`rrd-store` requires a complete disjoint retained/candidate partition and the
exact current catalogue digest, embeds bounded replay receipts in the
authenticated successor catalogue, and reclaims only archives, manifests, and
object payloads no retained entry references. `RrdEngine` is the sole
composition root: it produces application-complete estate backups, fences
backup/policy/hold/restore changes during prune, restores only to the fixed
absent per-instance restore hierarchy, verifies reopened watermarks and the
immutable closure, then records RPO/RTO results. Signer/key custody, remote
object transfer, active deployment cutover, and Multi-AZ claims remain in their
later gates.

**Native migration ledger landed 2026-08-23:** the only admitted application
format edge is the exact successor TextV1 → `RRDSK002` TagV2. Its authenticated
ledger resumes export/import/verify/two-rename cutover, preserves all 18
keyspaces, retains the predecessor and archive, rejects a source modified after
export, and is idempotent after completion. Fault injection covers every
durable and rename boundary. The first cross-version matrix also proves logical
archive recovery from TextV1 into a fresh current-format root. At that point,
reverse recovery, broader released-version evidence, and object-complete backup
closure remained open.

**G02-W04 candidate 2026-08-28:** the native application-format matrix is now
explicitly closed to TextV1 → TagV2 plus logical TextV1 archive recovery into an
absent TagV2 root. Native rollback verifies the visible successor and retained
predecessor against the authenticated cutover evidence, retains TagV2, restores
TextV1, and resumes across both reverse rename windows. Successor divergence,
unknown/skipped versions, and ambiguous layouts fail closed. New released
binary-version rows require a separately reviewed exact-successor transition;
they are not implied by a generic numeric upgrader. Object-complete backup
closure landed in G02-W02/G02-W03.

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
loopback HTTP alpha implemented. The contract keeps `rrflow-mcp` as an MCP adapter,
requires public payload types before handlers, denies non-loopback exposure
before F4, and makes accepted-request idempotency durable rather than
process-local.

**Implementation progress 2026-08-23:** `rrd-contract` now owns bounded session
limits/leases, transaction leases/states, typed public multi-model mutations,
and generalized commit receipts without importing private RRFlow types. Engine
now provides an
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
server's exact instance scope, and executed by the real RRFlowQL parser and
RRD query executor catalogue/binder/planner/executor. The response preserves typed values,
read manifest, cursor, schema revision, plan candidates, exactness/order/auth
contracts, and execution evidence. A real-socket test proves unauthenticated
and wrong-scope denial plus an exact persisted-record result. This does not yet
provide mutating RRFlowQL, live queries, or durable service-query spans.

The same public transaction resource now has a `data` scope whose typed
vocabulary lowers explicitly into one authoritative `RuntimeCommit`. Schema,
claim, record, relation, event, dense/sparse/multi-dense vector, series sample,
geo, and pre-staged object-reference changes share exact-cursor conflict
detection, the runtime hash chain, audit envelope, projection outbox, and one
commit identity. The coordinator freezes that identity in its durable prepared
intent and resolves lost acknowledgements from the runtime commit catalogue.
A real-socket test commits all nine families, restarts, replays the identical
receipt, and observes no duplicate changes. Generalized read-your-writes,
data-scope process-kill qualification, mutating RRFlowQL, object upload/staging,
and model-specific administration endpoints remain open.

`POST /v1/vector/search` now exposes the canonical vector truth path over an
authenticated runtime read stamp. Dense, sparse, and multi-dense/MaxSim query
shapes, four metrics, top-k and scan budgets are public typed contracts; the
result includes manifest/cursor, scan count, plan digest, selected access path,
exactness, typed identities, source cursors, and scores. The socket fixture
proves authentication denial and exact cosine retrieval of a vector written by
the public data transaction. Public filter algebra, named model binding,
persisted exact/HNSW/TurboQuant artifact serving, and vector administration
remain F7 work.

`POST /v1/changes/read` now exposes bounded retained runtime replay after an
exact cursor. Its public entries preserve commit ordinal, scope, time, actor,
lossless claim provenance, typed data mutation, and prior/current change
digests; the page carries authenticated-read method/cost evidence and advances
through unrelated global cursors safely. The socket fixture verifies
authentication denial, `3 + 8` pagination of one eleven-change transaction,
digest-chain continuity, and exact resume after server restart. Streaming
push, subscription leases, backpressure, and reconnect heartbeats remain open
before calling this a real-time live-query surface.

`POST /v1/changes/follow` now adds bounded five-second long-poll delivery over
that same retained coordinate. A concurrent real-socket test waits after
cursor two, commits an event at cursor three, receives exactly that typed
event, and then verifies an explicit empty timeout at cursor three. Streaming
SSE/WebSocket transport, durable subscription ownership, server-driven
heartbeats, cancellation on disconnect, and fan-out backpressure remain open;
the capability is advertised as experimental follow rather than complete live
queries.

Managed logical recovery is now also operable through the public server:
`POST /v1/backups`, `POST /v1/backups/list`, and `POST /v1/restores` use
authenticated catalogues, generated per-instance roots, durable idempotent
operation records, restore-to-absent-root, reopen verification, and explicit
partial-coverage declarations. Real-socket tests prove authentication and
scope denial, collision denial, create/list/restore, watermark preservation,
and restart replay. Public callers cannot supply filesystem paths.
Estate-managed local recovery now adds application-complete closure,
retention/RPO policy, fixed-root restore evidence, and effect-gap replay;
remote transfer and active deployment switch remain open.

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
uses RRFlow control-state CAS plus its authenticated journal to advance monotonic
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
before its completion receipt. The admin and controller executables are now
thin outward adapters owned by `rrflow-cli`, require a distinct estate-control
authority instance, and invoke engine-owned operations; `rrd-estate` no longer
opens an independent physical store. This removes hand-written direct database
mutation from the local workflow without claiming restore, retention, F4 remote
authentication, or organization-wide authorization.

### F4 — establish security and governance before remote management

**Status:** in progress. The persistent authority core is implemented in
`rrd-security` and specified by [`rrd-security-v1.md`](rrd-security-v1.md).
It stores bounded user/service/node principals with credential verifiers,
validity/disable state, and exact action/resource-prefix grants. Missing policy,
unknown or expired principals, wrong credentials, ungranted actions, and wrong
resource prefixes deny by default. Typed redacted audit records are immutable,
idempotent, and replayed through the authenticated RRFlow control journal.
Native reopen tests prove policy and audit persistence without journaling the
raw credential. When security state exists, the real server now authenticates
session creation with a principal/API key, durably binds that principal to the
session, maps every current authenticated route to a closed action, and
re-evaluates current exact resource policy on every request. A real-socket
differential proves missing/wrong credential denial, allowed query, and denied
ungranted backup with no data mutation. Routed success, authorization denial,
and execution failure are now written as redacted typed audit completions and
available through a policy-protected bounded public read whose cursor advances
over unrelated control history. The socket/reopen test proves all three
decision classes and absence of API-key material from public and journal
evidence. Pre-effect audit reservation is now implemented for authorized
routed work. An experimental TLS 1.3 mTLS listener/client now admits remote
binding only with initialized application security, requires a trusted client
certificate, verifies the exact server name, and advertises its transport mode;
plain HTTP remains loopback-only. Provisioning APIs, certificate reload/
revocation, secret providers, row/field policy, rate limits, application-
mutation/audit-completion atomicity, oversized-body/handler-failure coverage,
retention/rotation, and external archival remain open.

The first provisioning boundary is now executable through
`rrd-security-bootstrap`. A strict versioned manifest references mounted
credential files; only credential digests enter the persistent authority. It
is idempotent for identical material, denies drift after initialization, accepts
only bounded/private in-mount credential files, and has restart/reopen evidence.
This unblocks fresh Kubernetes volumes without adding an unauthenticated server
bootstrap route. Authorized ongoing policy/credential administration remains
open.

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

**Status:** in progress. `rrd-contract::EndpointCatalogue` now freezes all 31
current routes by canonical operation, method/path template, authentication,
mutation rule, fixed or descriptor-derived security action, and public
request/response type. Every public
wire type derives JSON Schema, and deterministic OpenAPI 3.1 is built from that
catalogue rather than maintained separately. The server publishes both at
`GET /v1/schema/endpoints` and `GET /v1/schema/openapi`; a frozen SHA-256 fails
unreviewed schema drift. This removes handwritten route and payload discovery
as SDK sources. Rust, TypeScript, Python, Go, Java, and .NET now have executable
walking skeletons; shared black-box conformance and release qualification
remain open.

The first Rust client is now implemented in `rrd-client` and specified by
[`rrd-rust-client-v1.md`](rrd-rust-client-v1.md). It is async, depends only on
the public contract/HTTP stack, bounds response bytes and per-attempt time,
honours absolute deadlines, maps typed errors, rejects remote cleartext, and
retries reads/idempotency-bound mutations only after transport loss or timeout.
Its methods cover the entire current endpoint catalogue. A hermetic real-server
test drops the first TCP connection, proves bounded recovery and negotiation,
then exercises authentication, exact query, deadline denial, transaction
begin/preview/abort, changefeed and protected audit. A second real-server test
proves mTLS success plus anonymous-client and wrong-server-name denial. Broader method rows,
released-version compatibility, packaging/reference generation, and the other
language clients keep F5 open.

The TypeScript walking skeleton is implemented in `sdks/typescript` and
specified by [`rrd-typescript-client-v1.md`](rrd-typescript-client-v1.md).
OpenAPI TypeScript generates the complete request/response surface and the
closed runtime endpoint map, ArkType validates untrusted response envelopes,
and Biome 2 plus strict TypeScript gate the package. Mock-transport tests cover
bounded retry, API-key session creation, bearer query, identity, deadline,
remote-cleartext and typed-error behavior. Shared real-server conformance,
package publication, and generated per-payload runtime validators keep this
client at walking-skeleton status.

The Python walking skeleton is implemented in `sdks/python` and specified by
[`rrd-python-client-v1.md`](rrd-python-client-v1.md). Its shell-free generator
derives the closed operation/route/auth/mutation map from the same OpenAPI
authority. The synchronous HTTPX client covers all 31 operations and enforces
bounded responses, correlation, resource identity, mutation idempotency,
absolute/per-attempt deadlines, safe retry, API-key/session authentication,
typed errors, and loopback-only cleartext. Pydantic validates response
envelopes; uv, Ruff, strict mypy, pytest, generation drift, and distribution
builds gate the package. Async transport, generated per-payload models, shared
real-server conformance, and publication remain open.

The Go walking skeleton is implemented in `sdks/go` and specified by
[`rrd-go-client-v1.md`](rrd-go-client-v1.md). Its shell-free generator emits a
closed operation constant set and route/auth/mutation map for all 31 operations
from the same OpenAPI authority. The standard-library client uses caller
contexts, bounded response reads, strict envelopes, correlated identities,
resource validation, mutation idempotency, combined deadlines, safe transport
retry, API-key/session authentication, redirect denial, and loopback-only
cleartext. Generation drift, `gofmt`, `go vet`, behavioral tests, and the race
detector gate it. Generated payload types, shared real-server/version
conformance, and publication remain open.

The Java walking skeleton is implemented in `sdks/java` and specified by
[`rrd-java-client-v1.md`](rrd-java-client-v1.md). Its generator emits a closed
operation enum with route/auth/mutation metadata for all 31 operations. Java's
HTTP client plus Jackson 3.2 enforce bounded reads, exact envelopes, correlation,
resources, mutation idempotency, combined deadlines, safe I/O retry,
authentication, redirect denial, and loopback-only cleartext. Maven/Java 21
warnings-as-errors compilation, generator drift, packaging, and real-loopback
JUnit 6 tests gate it. Async/caller cancellation, generated payload types,
dependency verification, shared conformance, and publication remain open.

The asynchronous .NET walking skeleton is implemented in `sdks/dotnet` and
specified by [`rrd-dotnet-client-v1.md`](rrd-dotnet-client-v1.md). Its generator
emits a closed operation enum/switch with route/auth/mutation metadata for all
31 operations. `HttpClient` and `System.Text.Json` provide a third-party-free
runtime with caller cancellation, bounded streaming, exact envelopes,
correlation, resources, mutation idempotency, combined deadlines, safe retry,
authentication, redirect denial, and loopback-only cleartext. .NET 10 nullable
analysis/warnings-as-errors, formatting, locked restore, xUnit v3 behavioral
tests, drift detection, and NuGet packing gate it. Generated payload types,
shared real-server/version conformance, and publication remain open.

**Deliverables**

- Generated protocol types plus intentional ergonomic layers.
- Auth/session, transaction, RRFlowQL, CRUD/schema, vector/query, live feed,
  snapshot/backup and estate clients.
- Async, timeout, cancellation, retry and idempotency behavior.
- Capability/version negotiation and typed error mapping.
- Hermetic conformance suite run against local server and supported distributed
  deployment; examples and API reference generated in CI.

**Exit gate:** every supported SDK passes the same black-box semantic fixtures;
unsupported server/client version pairs fail explicitly.

### F6 — complete the Surreal-class RRD data/query surface

**Status:** in progress. The public transaction contract and RRD coordinator
already atomically commit schema, claim, record, relation, event, vector,
series, geo, and object-reference mutations; this is retained as the write
authority rather than rebuilt. The first read-breadth slice adds
`series:<kind>` and `geo:<kind>` to the existing record/relation/event/claim
RRFlowQL grammar. They bind and execute through the same explicit valid/known
time, captured read stamp, content-addressed exact plan, deterministic ordering,
and scan/row/output/batch budgets. Memory, Fjall compatibility, native RRD LSM,
and secured real-RRD tests prove typed results and time cutoffs. Details and
current built-in-field limits are in
[`rrflowql-multimodel-v1.md`](rrflowql-multimodel-v1.md).
The next slice adds a `traverse:<relation>` source with a mandatory canonical
start reference, outgoing/incoming/both direction, and depth in `1..=32`.
Binding proves the types against the stamped schema; execution walks only the
visible bitemporal graph snapshot, orders edges canonically, suppresses cycles,
and emits one deterministic shortest-path row per reached node. Memory, Fjall
compatibility, native RRD LSM, and secured RRD tests cover it.
Typed predicates now carry their comparison operator through the AST, bound
plan, digest, and executor. `=`, `!=`, `<`, `<=`, `>`, and `>=` round-trip to a
canonical query; ordering is fail-closed to integer, unsigned, and string
operands. The event-cursor point path remains eligible only for equality.
The first index lifecycle slice persists schema-bound compound definitions in
CAS-protected control state, journals every lifecycle transition, fences stale
builder generations, and binds readiness to configuration/artifact digests and
an exact per-scope source cursor. RRD query executor emits matching prefix/freshness
candidates. It now builds durable content-addressed exact snapshot artifacts
and selects them only at the exact source cursor and valid-time after complete
identity/digest revalidation; stale queries use the authoritative log. See
[`rrflowql-index-catalogue-v1.md`](rrflowql-index-catalogue-v1.md).

**Deliverables**

- Mutating RRFlowQL and multi-statement transactions.
- Dedicated document/key-value/record/edge CRUD and retirement surfaces. The
  engine now has one typed mutation and exact-stamp read contract across all
  catalogue models, including history-preserving retirement, event-cursor
  correction, mixed-model atomicity, and reopen parity. Mutating RRFlowQL and
  the dedicated HTTP/MCP administration adapters remain G03-W03 and G06 work;
  the internal G03-W02 engine contract is an implementation candidate pending
  qualification.
- Relational links, richer graph/path algebra, and spatial operators.
- Materialized ordinary/compound/unique/count, spatial and full-text indexes;
  exact scalar snapshot artifacts now execute and authenticated ensure/list
  administration is public, while uniqueness, incremental maintenance, count,
  spatial, and full-text remain open.
- Full-text analyzers and a replacement lexical stack measured against the old
  BM25 path; this is where the later LFG/TurboQuant retrieval integration lands.
- Planner cardinality/cost evidence, `EXPLAIN ANALYZE`, streaming batches and
  bounded execution.
- Push live subscriptions with backpressure/reconnect plus retained changefeed
  replay from exact cursors.
- Semantic live-query polling now has exact-cursor replay, deterministic row
  deltas, bounded three-engine differentials, a strict authenticated RRD route,
  a distinct deny-by-default security action, and generated route discovery in
  all six SDKs. Bounded server-side waiting now wakes on authoritative cursor
  advancement with deadline and timeout evidence. Streaming, typed ergonomic
  SDK methods, retained subscription leases, and backpressure remain open.
- User-defined triggers/functions only after capability and audit gates exist.

**Exit gate:** multi-model and live-query differentials cover semantics,
failure, restart, permissions and bounded performance. SurrealDB comparisons
remain per-capability, never one synthetic “database score.”

### F7 — complete the Qdrant-class vector surface

**Status:** in progress. The first public vertical persists collection and
named-vector definitions under control-state CAS with stable generations,
idempotency receipts, and journal history. Definitions bind field, dense/
sparse/multi-dense kind, dimensions, metric, optional model digest, and
pinned/cached/cold policy. Authenticated ensure/list routes have distinct
actions, all six SDK route catalogues discover them, and collection-addressed
exact search enforces the definition. Atomic vector writes can bind the same
collection/name and validate field, kind, dimensions, and model provenance
before committing payload properties with the unified transaction. Native
reopen plus real-process denial, replay, collision, list, bound point commit,
mismatch, and search tests pass. First-class point deletion and batch-mutation
APIs, payload indexes, physical tier enforcement, filtered persisted ANN, and public
inference administration remain open. A deterministic MSE TurboQuant codec and
authenticated binary artifact now implement seeded rotation, fixed
distribution-matched 4/2/1.5/1-bit packing, norm-corrected asymmetric scoring,
catalogue/planner selection, corruption denial, and exact-f32 reranking. Public
artifact build/lifecycle routes, SIMD, mmap, and broad quality/recovery evidence
remain open. Public exact search now includes bounded equality, inequality,
membership, range, existence, and recursive all/any/not payload filters with
matching/non-matching real-process evidence. Authenticated point scroll now
shares exact visibility semantics with search and returns bounded
reference-ordered vector/provenance/payload pages with resume evidence. Direct
batch retrieve returns found points in request order plus explicit missing
identities. First-class delete remains open.

**Deliverables**

- Collections/points/named-vector/payload administration. Collection and
  named-vector ensure/list, collection-bound atomic vector/payload writes, and
  deterministic point scroll, and direct batch retrieval are implemented;
  first-class delete and payload-index administration remain open.
- Compact mutable dense HNSW and sparse index lifecycle.
- Typed payload indexes and true one-stage filtered traversal; ACORN-quality
  restrictive-filter path evaluated on fixed selectivity corpora.
- Unified nearest/id/recommend/discover/context/scroll/group/facet/matrix,
  hybrid/prefetch/multistage/formula query algebra.
- Dense+sparse rank fusion and ColBERT late interaction without middleware.
- Complete TurboQuant promotion: the landed MSE path has seeded rotation,
  fixed distribution-aware mapping, 4/2/1.5/1-bit packing, asymmetric scoring,
  authenticated artifact lifecycle, planner selection, and exact reranking.
  Add public lifecycle administration, SIMD/mmap, broad recall/bias/latency and
  recovery evidence; keep scalar/binary/product methods separate.
- Per-structure pinned/cached/cold memory policy.
- Physical GPU indexing with CPU byte/semantic differential and fallback.
- Server/client inference for local and provider models with digest provenance.

**Exit gate:** exact-oracle correctness first, then recall/latency/memory/build/
update/recovery matrices on fixed hardware and corpora against current Qdrant.

### F8 — qualify distributed and Kubernetes operation

**Status:** experimental distributed and Kubernetes foundations present; no
Multi-AZ product qualification. `rrd-cluster` freezes canonical three-zone placement,
per-shard quorum semantics, explicit partial-order snapshot vectors, transfer
and reshard cutovers, transport identities, bounded telemetry, and restart-safe
artifact sessions. Its OpenRaft adapter runs both an in-process three-voter
cluster and independent node processes over native RRFlow storage. Tests prove
replication, leader loss/failover, membership change, snapshot install,
corruption denial, mTLS peer-identity denial, minority-partition write/read
denial, and survival of every single disk loss in the modeled topology.

`rrd-kubernetes` now adds a real kube-rs watch/controller, generated namespaced
`RrdInstance` CRD with status/CEL admission, finalizer, server-side apply,
least-privilege RBAC, and deterministic headless/client Services, one-replica
StatefulSet, retained PVC, PDB, and default-deny NetworkPolicy. A security-
bootstrap init container plus TLS 1.3 mTLS server makes a fresh persistent
volume executable. Tests freeze CRD drift, RBAC exclusions, unsafe-spec denial,
and exact rendered resources.

This remains a single-node Kubernetes alpha, not Multi-AZ. The public RRD data
plane is not yet backed by `rrd-cluster`; the controller therefore refuses to
misrepresent independent RRD pods as replicas. There is no published release
image/SBOM/signature, real-cluster conformance, RRD/Raft integration, safe
rolling-upgrade controller, CSI recovery workflow, certificate rotation
controller, independent-host fault lab, or supported outbound control-plane
agent yet. See [`rrd-kubernetes-v1alpha1.md`](rrd-kubernetes-v1alpha1.md).

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

**Status:** local developer foundation present; authoritative remote operations
remain open. `connectome-ui` is a real loopback application over one physically
bound RRFlow instance. It renders estates, logical tables, data models, source
routes, query results, vector artifacts, retention, reasoning flights, temporal
changes, causal traces, runtime graphs, and retained cluster observations.
Freeze, rewind, forward, speed control, raw-event inspection, weak/strong prompt
cohorts, and explicit capability maturity are backed by retained evidence.

Commit `e21eb6e` adds the first connection-profile authority. The Connections
workspace negotiates the public RRD capability handshake against an exact local
instance before committing a bounded profile through RRFlow control-state CAS and
its hash-chained journal. Retries are idempotent, key rebinding is denied, only
credential references are retained, non-loopback cleartext is denied, and
remote profiles require HTTPS. Authenticated data access, selecting a connected
RRD as the active data source, remote TLS, profile deletion/rotation, and estate
mutation controls remain open.

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

## Next executable sequence: breadth before optimization

F0 is frozen, F1 has an operable partial-coverage archive/recovery path, F2 has
a real loopback process, and F3 has persistent authority/reconciliation plus a
local driver. Work now establishes one runnable, tested vertical baseline for
each remaining product layer before returning to engine benchmark tuning:

1. close only correctness gaps that prevent safe use of the existing F0-F3
   paths; do not expand them into optimization campaigns;
2. implement F4 identity, deny-by-default authorization, secret/TLS boundaries,
   and comprehensive audit over every currently public operation;
3. generate and qualify the F5 Rust, TypeScript, Python, Go, Java, and .NET
   clients against one schema and conformance corpus;
4. complete the baseline F6/F7 administrative, live, filtered-index,
   TurboQuant/inference, and memory-tier paths;
5. retain the executable F8 cluster and F9 local-panel foundations, then add
   Kubernetes packaging/control and authenticated RRD source switching rather
   than rebuilding their already-tested local/cluster semantics;
6. integrate Automaton → LFG → Connectome/RRD lifecycle contracts and run the
   firm-alpha recovery/security/capability matrix;
7. only then resume bounded performance optimization and comparative claims.

Remote provisioning remains prohibited until the F4 baseline is executable.

### Cross-cutting foundation — governed context maintenance

Connectome's context-maintenance contract is frozen in
[`context-maintenance-v1.md`](context-maintenance-v1.md). It treats the claim and
typed runtime logs as immutable authority, requires an authenticated F1 archive
before proposing a smaller working projection, and gates application on
instance-specific recall/replay differentials. The executable state machine,
real Connectome controls, prompt-flight validation, projection activation, and
compensating rollback are the current F9 slice; no fixed 50–75% target is
presented as universally safe.
