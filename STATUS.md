# Runtime status — 2026-08-24

Vyrm implements the complete **local reasoning-runtime loop** described by the
earlier kernel plan. It does not implement the complete database/vector/cloud
product stack. The previous work optimized bounded VyrmKV/Fjall and SurrealDB
3.0.5 cells before inventorying the current SurrealDB and Qdrant products; that
was a sequencing error. The bounded evidence remains valid, but it is not a
full-stack or superiority result.

The corrected baseline is now explicit in
[`docs/full-stack-gap-ledger.md`](docs/full-stack-gap-ledger.md), preceded by
the complete product-capability inventories for
[SurrealDB](docs/surrealdb-capability-inventory.md) and
[Qdrant](docs/qdrant-capability-inventory.md). Public loopback server/session/
transaction, persistent estate, deny-by-default security/audit, and the first
supported Rust client now have executable walking skeletons. The largest
unfilled areas are shared/released SDK conformance, object-complete/signed backup
policy, live subscriptions, general multi-model query and indexes, the full
vector query/payload-index/quantization lifecycle, production distributed/
Kubernetes operation, and authoritative Connectome management.

## Landed

- F0 freezes the first public `rrd-contract` resource, envelope,
  idempotency, capability-negotiation, and error vocabulary independently of
  private Vyrm types. F1 now provides a content-authenticated logical archive,
  exact claim/runtime replay, restore-to-absent-root with hidden staging and
  reopen verification, an authenticated content-addressed backup catalogue,
  corruption/retry tests, and CLI operations. Catalogue coverage deliberately
  reports object payloads as referenced-only and the backup as not yet
  application-complete; object closure, signatures/encryption, retention
  policy, and a broader released-version matrix remain open.
- The authenticated native-format ledger implements the exact-successor
  TextV1→TagV2 transition across all 18 keyspaces with invisible staging,
  retained predecessor/archive evidence, source-drift denial, restart resume
  across every durable/rename boundary, CLI status, and idempotent completion.
  The initial cross-version recovery row also restores a legacy-format logical
  archive into a fresh current-format root.
- F2 implementation has started at the durable boundary: public session and
  claim-transaction payloads are frozen, and client idempotency bindings are
  now committed atomically with claim batches in Memory, Fjall compatibility,
  and native VyrmKV. Same-key replay survives restart without duplicating a
  claim; key/digest collisions fail closed. Engine control state now combines
  compare-and-swap materialization with a monotonic, SHA-256-chained,
  replayable journal in one storage transaction. The persistent RRD
  coordinator journals session create/renew/close/expiry and transaction
  begin/prepare/commit/abort/expiry, renews bounded idle leases, expires child
  transactions, and stores only token hashes. Prepared commit intent prevents
  a changed retry key from duplicating data across either crash window.
  `rrd-server` now ships an async Axum/Tokio loopback process with health,
  readiness, capability, claim-preview/commit, bounded versioned envelopes,
  canonical digest vectors, JSON tracing, stable private token-key files, and
  graceful shutdown. Real-socket tests cover restart, rotation, expiry, quota,
  malformed/oversized requests, deadline precheck, abort/close,
  disconnect/retry, concurrent convergence, collision, and real-binary remote
  bind denial. Cancellation, generalized read-your-writes and administration,
  result/time bounds, metrics, and released-version client qualification keep
  F2 open; these transport leases are not F4 user authentication.
- F3 has started at its persistent authority boundary. `rrd-estate` stores one
  explicitly bounded, versioned aggregate per estate through control-state CAS
  and the authenticated journal. Desired generations, observed evidence,
  operation state, fenced lease epochs, idempotency bindings, append-only
  receipts, heartbeats and meaningful-runtime evidence survive native-engine
  reopen. Activity is derived as unknown/active/idle/stale/neglected from
  explicit thresholds. The typed reconciler advances at most one durable
  boundary per call. Reopen tests cover lease, prepared, lost-effect-acknowledgment,
  applied, observed and completed boundaries; external effects are deduplicated
  by the stable operation ID, expired-worker takeover preserves prepared work,
  and new desired generations supersede stale unfinished operations. The
  restore jobs, deployment/upgrade completeness, and production packaging
  remain open. The stable public
  `EstateSnapshot` is now available through a session-authenticated RRD read
  endpoint and Connectome's read-only `/api/estate`/workbench projection.
  Connectome shows desired→observed generations, activity and operation state;
  if authority is absent it explicitly labels the local manifest row synthetic.
  The first typed local-process driver also authenticates canonical executable
  paths by SHA-256, resolves only typed argument sources, clears inherited
  environment, journals through the existing prepared/applied state machine,
  and verifies PID + start time + executable before a signal. A real
  `rrd-server` child survives controller-object/engine reopen and same-operation
  replay without a PID change, then is actually stopped by a new desired
  generation without deleting its data directory. Forged PID identity is
  denied. A separate one-step controller is now killed by a black-box harness
  after every start and stop boundary, including after the real effect but
  before `applied`; retries preserve the start PID or converge from an already
  completed stop. Managed children now use a bounded, paired request/completion
  file protocol, reauthenticate PID/start/executable ownership before forced
  fallback, and retain per-instance startup logs. The native process matrix
  passes the full authority, process-contract, real child/controller recovery,
  and strict-clippy sequence on Linux, Windows, and macOS. Operator mutation/
  packaging, per-instance restore/retention, and bounded log retention keep F3
  open. The storage-format composition defect exposed by the first matrix run
  is corrected, and the subsequent repository-wide CI run is green.
  `rrd-deployment-catalog` now resolves and hashes the installed sibling server,
  emits the validated typed launch/shutdown contract with owner-private durable
  no-overwrite publication, and passes its real-binary test on all three native
  CI operating systems. Installer/service packaging remains open.
  A local-only operator policy now binds exact operator, key digest, validity
  window, estate and action. `rrd-estate-admin` performs authorized create and
  desired-state mutations and quiesced backup schedules with durable replay and
  public snapshot output;
  unauthorized work is denied before opening storage. Remote mutation and F4
  identity/RBAC/ABAC remain unavailable.
  Backup execution is a separate one-step process with fenced durable job
  boundaries and a fixed canonical state root. A black-box kill test proves a
  retained archive is replayed into one authenticated catalogue entry when the
  controller dies before recording completion. Restore-to-absent-instance and
  retention pruning remain open.

- F4 now has a persistent `rrd-security` authority and HTTP enforcement
  walking skeleton. User/service/node principals, validity and disable state,
  exact action/resource-prefix grants, principal-bound sessions, per-request
  policy re-evaluation, and redacted authorization/outcome audit survive
  reopen. Missing policy or permission denies by default. Protected audit read
  advances through the authenticated control journal. Provisioning, TLS/mTLS,
  secret providers, row/field policy, rate limits, audit retention/archival,
  and application-mutation/audit-completion atomicity keep F4 open.
- F5 publishes one validated catalogue for all 28 current RRD operations and
  serves it from the running process. JSON Schema for every public wire type
  and deterministic OpenAPI 3.1 are derived from the same authority, served by
  RRD, and protected by a frozen digest. The first supported `rrd-client` Rust
  boundary now negotiates that contract and covers session, transaction,
  VyrmQL, vector, changefeed, backup, estate, and audit calls with bounded
  async I/O, absolute deadlines, typed errors, response identity checks, and
  safe transport retry. A real-server fault test proves negotiation recovery,
  auth, exact query, transaction preview/abort, changefeed, audit, and local-
  only enforcement. The TypeScript walking skeleton now generates its
  full operation surface from OpenAPI, validates network envelopes with
  ArkType, and passes Biome, strict TypeScript, generation-drift, retry/auth/
  query/error tests. The Python walking skeleton derives the same closed route
  surface, validates envelopes with Pydantic, and passes uv lock/build,
  generation-drift, Ruff, strict mypy, retry/auth/query/error tests. The
  generated Go walking skeleton covers all 28 operations with a
  standard-library client and passes format, vet, drift, behavioral, and race
  gates. The generated Java 21 walking skeleton covers all 28 operations,
  compiles with warnings as errors, and passes real-loopback JUnit retry/auth/
  query/error tests. Shared conformance, packaging, generated payload models,
  async Python/Java. The generated asynchronous .NET 10 walking skeleton covers
  all 28 operations and passes formatting, warnings-as-errors build, locked
  restore, three xUnit transport/auth/error tests, and NuGet packing. All six
  intended clients now exist; shared real-server/version conformance, complete
  generated payload models, and release publication keep F5 open.
- F6 has started from the existing atomic public data transaction rather than
  duplicating it. VyrmQL/VyrmMX sources now cover records, relations, events,
  claims, series samples, and geo values with explicit valid/known time,
  stamped exact plans, deterministic rows, and bounded execution. Memory,
  Fjall compatibility, and native VyrmKV return identical series/geo results;
  a secured real-RRD transaction/query test proves the same public typed rows.
  Bounded recursive traversal now validates its start/relation against the
  stamped schema, requires direction and depth, returns deterministic shortest
  paths, and terminates cycles; three-engine and secured-RRD tests pass. Only
  frozen series/geo built-ins are bindable today. Scalar equality, inequality,
  and ordering predicates are typed and deterministic; ordering currently
  accepts only integer, unsigned, and string operands. Incremental and broader
  index families, spatial operators, full text, mutating VyrmQL, streaming, and push
  subscriptions keep F6 open.
  The first index lifecycle and serving foundation is also executable: schema-bound
  compound definitions persist through authoritative control-state CAS,
  generation fencing and ready/building/quarantined/retiring state survive
  native reopen, and exact snapshot artifacts are durably content-addressed.
  Matching queries select an artifact only when generation, cursor, schema,
  valid-time, configuration, bytes, and digest agree; stale state falls back to
  the authoritative log and corrupted bytes fail closed. Uniqueness,
  incremental maintenance and broader index families remain open. Authenticated
  RRD ensure/build and list routes have distinct deny-by-default actions,
  durable idempotency receipts, and generated discovery in all six SDKs.
  Resumable semantic polling now evaluates one query at exact resume/head
  cursors and returns deterministic added/updated/removed rows with bounded,
  fail-closed execution on all three engines. An authenticated, separately
  authorized RRD polling route exposes the strict public contract and all six
  SDK route catalogues include it. Bounded waiting now wakes on authoritative
  cursor advancement, respects request deadlines, and distinguishes timeout
  from an empty delta. Streaming/backpressure, retained subscriptions, and
  ergonomic typed SDK helpers remain open.

- F7 has an executable public collection foundation. Named dense, sparse, and
  multi-dense spaces persist in a CAS-protected catalogue with exact field,
  dimension, metric, optional model-digest, and pinned/cached/cold policy.
  Ensure/list use separate authorization actions, durable idempotency receipts,
  stable generations, and the authenticated control journal. Search may address
  a collection plus vector name and fails before execution on kind/dimension
  drift. Atomic vector point writes can bind the same coordinates and validate
  field, kind, dimensions, and model provenance before commit while preserving
  payload properties. Native reopen, replay, collision, list, denial, bound
  point commit, and real-process exact search pass. First-class point deletion
  and batch-mutation APIs remain open. The public search contract now includes
  a bounded recursive payload-filter algebra and real-process matching/non-matching
  evidence over committed point properties. A deterministic MSE TurboQuant
  codec now supports seeded rotation, 4/2/1.5/1-bit packing, norm-corrected
  asymmetric scoring, authenticated binary reopen, planner selection, and
  exact-f32 reranking. Public artifact build/lifecycle administration, SIMD and
  large-corpus quality qualification, payload indexes, persisted one-stage
  filtered ANN serving, and physical tier enforcement remain F7 work.
  Authenticated point scroll now returns deterministic reference-ordered pages
  at one exact read/valid-time coordinate through the same shared visibility
  primitive as search, including model and payload-filter enforcement. Direct
  authenticated batch retrieval preserves request order and explicitly
  returns missing identities at the same snapshot. First-class delete remains
  open.

- The bi-temporal claim kernel has immutable supersession corrections,
  canonical SHA-256 identities covering provenance and validity, and atomic
  batch parity between Fjall and the in-memory reference engine.
- A typed temporal property graph now sits on one authoritative runtime log.
  `RuntimeCommit` atomically records claims, node versions, relation versions,
  and lifecycle events; every mutation receives a global cursor and joins a
  SHA-256 hash chain. Exact-cursor compare-and-swap rejects concurrent stale
  writers. Relation endpoints and subject-bearing events cannot reference a
  missing node in their scope.
- M4 extends that same transaction—not a parallel database—with exact
  dense/sparse/multivectors, typed series samples, WGS84 values, and verified
  content-addressed object references. Memory, Fjall, and native engines update
  canonical latest-value indexes, deterministic projection outbox work,
  chained accepted-operation audit, and an idempotent retry outcome atomically.
  `vyrmDS::DataRuntime` stages and re-verifies bytes before reference commit.
  Local storage has sync/rename/verify, orphan inventory/reclamation, and
  corruption quarantine; the capability-explicit S3-compatible adapter requires
  real conditional create and passes the same semantic differential fixture.
  Failure injection covers every local publication boundary and both sides of
  commit; mixed-family rollback and native flush/reopen are tested.
- M6 closes the local embedding/edge kernel gate. `vyrm-embed` binds each job
  to source bytes, an exact model digest, network policy, and the originating
  read stamp; it detects source changes around inference and relies on final
  transaction CAS for commit authority. An optional local FastEmbed adapter is
  compiled without hub/TLS features and hashes caller-supplied ONNX/tokenizer
  material. Vector requests and projections now bind model space explicitly.
  Compact dense v1 stores canonical metadata plus aligned raw `f32`, verifies
  corruption/layout/model/freshness, publishes atomically, and opens through a
  real read-only mmap. Scalar and runtime-dispatched AVX2 paths match the exact
  oracle. The feature-gated accelerator boundary admits only verified
  CPU-identical bytes and makes fallback policy visible. `vyrm-edge` packages
  one-call no-network embedding/search; its retained 10k×128 local profile is
  inside binary, artifact, RSS, and latency budgets. This does not certify a
  physical GPU or semantic-model quality.
- The first M7 protocol gate is executable in `vyrm-cluster`. Canonical
  placement enforces ordered unique voters and zone diversity; per-shard read
  stamps compose into a real partial-order snapshot vector. Route evidence,
  grounded snapshot-plus-WAL transfer, metadata-indexed reshard cutover, and
  cross-shard denial are typed. A deterministic single-term quorum simulator
  injects partition, delay, duplication, reorder, crash/restart, clock skew,
  and disk loss. Enumerated three-voter schedules prove that acknowledged
  entries retain a durable copy across every tolerated single-disk loss and
  that a leader minority cannot acknowledge. A feature-gated OpenRaft 0.9.25
  adapter now durably stores votes, logs, committed pointers, state, and
  snapshots in native VyrmKV and passes the complete upstream storage suite. A
  four-node in-process test covers election, quorum commit, snapshot catch-up,
  majority-side failover, post-failover commit, and voter replacement.
  Adapter format v4 physically separates node-local vote/log/commit/purge and
  snapshot-cache state from transferable canonical application state. A typed
  `RuntimeCommit` and Raft applied state still publish in one state-domain WAL
  frame. Authenticated VyrmKV bundles now carry that complete canonical state;
  a four-node run snapshots a real runtime commit, purges the leader log, and
  catches up a fresh learner that reopens the same runtime truth. Local votes
  are not imported. Corrupt, forged-metadata, stale, duplicate, restart, and
  same-frame differentials are green. An opt-in transport now carries OpenRaft
  RPCs over real TCP and TLS 1.3 mutual authentication. CA validation, exact
  SPIFFE-style URI identity, static Raft-id/canonical-id authorization,
  cluster/shard/source/target and request-digest binding, Raft-vote-source
  binding, 16 MiB pre-allocation frame limits, OpenRaft hard TTLs, 256-RPC
  admission, and a 30-second ingress lifetime fail closed. Transport telemetry
  v1 freezes allowed/denied/failed counts, bytes, latency, current/peak
  concurrency, connection denials, and bounded per-identity rate windows.
  Configurable global/identity admission is enforced before dispatch; the
  version-3 supervisor status returns transport-ingress, artifact-session, and
  consensus-trace health with explicit process-reset timestamps. Typed Raft
  timing defaults use 250 ms heartbeats and a 1--2 second election window
  instead of OpenRaft's scheduler-sensitive development defaults. A four-node
  loopback run covers election, replication, post-purge snapshot catch-up,
  certificate impersonation denial, and authenticated vote-source forgery denial. The
  feature also ships a real `vyrm-cluster-node` process boundary with a bounded,
  versioned, request-correlated JSON-lines supervisor protocol over inherited
  stdin/stdout. A four-process black-box run proves abrupt voter restart and
  catch-up, failover write, bidirectional live-leader transport isolation,
  majority-side replacement and write, healing/reconciliation, post-purge
  physical snapshot catch-up, leaf/node identity confusion denial before
  readiness, and corrupt VyrmKV `CURRENT` refusal on restart. The monotonic
  wait contract is stress-tested against learners that advance beyond the
  requested index. Complete TLS states can now be hot-reloaded by exact-successor
  generation: leaf/key, full trust-root set, and complete CRL set swap together
  for every new one-RPC connection. WebPKI checks CRLs fail closed on unknown or
  expired revocation state. Real-TCP evidence preserves Raft through leaf
  replacement, two-root overlap, migration to a second CA, old-root retirement,
  and denial of revoked and retired-root leaves. The process matrix distributes
  revocation, proves a restarted stale leaf cannot catch up, reapplies the full
  set, and then completes partition/reconciliation/snapshot recovery. This is a
  file-fed supervisor contract; SPIFFE Workload API streaming, automatic
  issuance, durable supervisor generation, independent hosts/hardware faults,
  retained telemetry exporters/alerts, and Multi-AZ deployment remain unclaimed.
  OpenRaft snapshots are now seekable files rather than `Cursor<Vec<u8>>`:
  bundle export, receive, install, local object publication, verification, and
  durable reopen avoid whole-bundle allocation; writes are hard-capped at 1 GiB;
  ephemeral files clean up on drop/restart; and ambiguous spool entries fail
  closed. Crash/storage-full export boundaries, corruption/truncation, durable
  cache reopen, post-purge catch-up, and a >16 MiB bundle/≤16 MiB incremental
  RSS Linux regression are green. VyrmKV segment v3 now keeps immutable files
  disk-resident, validates/decodes independent 4 KiB blocks, and shares a
  configurable bounded LRU across a database. A 20 MiB Linux reopen/read
  regression with a 4 MiB cache stays within 16 MiB RSS growth and proves
  eviction; this closes the former resident-segment qualification.
  Placement epochs are now explicit replicated `placement_transition`
  operations: initialization must be epoch 1, advances must be exact successors,
  and declared voter canonical ids/zones must equal the applied OpenRaft
  membership. Ordinary work before initialization or at another epoch is
  durably denied; a later Raft voter identity/zone change invalidates the old
  binding until an exact-successor placement rebinds it. Learner-only metadata
  does not create false invalidation. Request identities retain exactly the last
  4,096 applied-log positions and prune deterministically into state/snapshots;
  runtime commits retain independent content idempotency after that request
  window expires.
- Native VyrmKV now provides the required physical snapshot-bundle primitive.
  Bundle v1 is a deterministic SHA-256-authenticated binary closure over a
  flush-bounded manifest and all reachable immutable segments. Installation
  syncs segments and an empty continuation WAL before one local manifest CAS;
  it never imports the source manifest lineage. Round-trip, reopen, stale and
  corrupt denial, idempotency, continued-write, and crash/storage-full matrices
  are green. Adapter v4 consumes this closure for OpenRaft snapshot build and
  installation while retaining source and target node-local Raft history.
- M7 replica recovery now includes the immutable object closure named by
  canonical runtime truth. `ArtifactTransferManifest` binds the project read,
  shard/epoch, grounded snapshot, source/target, sorted `ObjectReference`
  inventory, and exact digest set. The local adapter streams with fixed-size
  buffers, verifies length and SHA-256 at both ends, deduplicates physical
  content, and makes retries no-copy when the target already verifies. The
  OpenRaft integration hydrates before state activation and independently
  scans the authenticated snapshot's scoped `runtime_objects` keyspace, so a
  self-consistent but incomplete manifest is denied. Tests cover missing and
  corrupt sources, target corruption, substitution, incomplete closure,
  idempotent retry, activation ordering, restart, and preservation of local
  votes. Durable cluster/storage trace trees record only control evidence and
  transfer counters, with equal normalized results across Memory, Fjall, and
  native engines. Restart-reconstructed receiver telemetry separately exposes
  inventory, quotas, receipt replay, GC, denials, failures, and overflow without
  object bytes. S3-compatible semantics exist, but that synchronous adapter
  currently materializes one object; multipart/resumable cross-host transport
  and independent-host evidence remain open.
- A persisted, revisioned schema registry now governs typed runtime records,
  relations, and events. Unknown types and properties fail closed; property
  value types, required fields, event subjects, legal edge endpoints, temporal
  uniqueness, pair uniqueness, and incoming/outgoing cardinality are enforced
  before the atomic commit. Schema migrations share the hash chain and must
  advance exactly one revision. Fjall reopen and in-memory differential tests
  prove the same contract.
- Reasoning and prompt-flight state no longer rewrites authoritative JSON
  ledger blobs. Both append typed node revisions and immutable micro-events to
  the runtime log. Existing v1 projection ledgers remain readable and migrate
  atomically on their next mutation.
- Preflight owns a persisted `vyrm-graph` routing projection. Every project
  mutation refreshes it immediately beforehand; unreadable source, corrupt
  state, root mismatch, or projection quarantine denies the call. Recovery is
  explicit through `reset-routing` or `reset-projection`.
- A typed, hash-chained reasoning contract records `goal → plan → attempt →
  observation → decision → verification → outcome`. Failed verification
  requires an explicit continue/stop decision. A successful outcome cannot
  bypass passing verification.
- Mutation policy is deny-by-default. One recorded attempt authorizes one tool
  result; post-tool dispatch records content-addressed observation evidence and
  closes that authorization. Verification commands become typed pass/fail
  checks. Denials include expected-versus-observed contract differentials.
- `vyrmd` exposes preflight, recall, routing, reasoning, and lifecycle tools over
  newline-delimited stdio MCP. It supports stateless MCP `2026-07-28`
  `server/discover` and legacy `2025-11-25`/`2025-06-18` initialization while
  calling the same node functions as hook runtimes.
- `vyrm-eval` runs paired provider trials and normalizes success, retries,
  regressions, tool calls, tokens, and latency. The 2026-08-18 run covered two
  providers, two repositories, and stale-evidence/post-compaction scenarios:
  8/8 success, zero retries, zero paired regressions. Runtime was cheaper in
  the stale-evidence cells and more expensive in the post-compaction cells; the
  sample does not support a universal efficiency claim.
- `vyrm init` creates a versioned, relocatable instance manifest. Preflight,
  lifecycle hooks, reset-routing, and `vyrmd` fail closed on absent identity or
  a store/root mismatch. Dedicated instances are executable; umbrella
  manifests enforce explicit relative membership but remain non-executable
  until every mutable ledger and projection is member-scoped.
- `connectome` serves an instance-bound developer workbench with overview,
  selection-centered/global graph, typed reasoning timelines, current claims,
  ranked source routes, invocation activity, and object inspection. Its prompt
  flight recorder runs exact prompts through fresh, pruned, or full context
  arms; observable micro-events can be played, frozen, expanded, and compared.
  Frontier runners are disabled by default and read-only when explicitly
  enabled. All other write endpoints remain closed; remote binding requires an
  explicit unauthenticated-transport override. Its read API now exposes
  resumable changes, graph-at-cursor/valid-time snapshots, and exact graph
  differentials for freeze/scrub tooling.
- The workbench's Temporal stream projects the newest bounded authoritative
  runtime mutations across all scopes into six semantic lanes. First, rewind,
  freeze, forward, latest, scrub, and packet inspection retain each global
  cursor, scope, commit identity, mutation digest, full mutation, and available
  hash-chained audit envelope. It does not invent physical WAL events,
  unpersisted query activity, or hidden model reasoning. Remote browser access
  is executable through explicit `--bind ... --allow-remote`; it remains
  unauthenticated and is suitable only for a trusted network or SSH tunnel.
- The workbench adds a Schema lens and `GET /api/runtime/schema`, making the
  active revision, migration, types, property rules, subject constraints, and
  edge cardinality visible instead of leaving enforcement buried in code.
- The flight workbench is now a one-run-at-a-time learning instrument. Weak and
  strong prompts are editable presets, custom drafts survive live polling, and
  exact Default/High/Extreme/Ultra controls persist the requested provider
  `medium`/`high`/`xhigh`/`max` effort on every run.
- Observable events render as a typed event-mass river across context/routing,
  model-envelope, tool, and outcome lanes. Operators can freeze any packet,
  scrub, rewind, resume, fast-forward up to 8×, jump to boundaries, inspect the
  entire retained envelope, and compare identical-prompt effort runs using
  provider-reported token, cache, tool, latency, and acceptance evidence.
- Connectome now publishes `vyrm-diagnostics` v1 at
  `GET /api/runtime/capabilities`. The handshake advertises persisted,
  restart-recoverable, seekable, reversible playback and the exact available
  speeds/lenses. Its engine catalog separates alpha, partial, experimental,
  and planned work and carries executable evidence plus the current limitation
  for every item. A reopen test proves prompt-flight packets survive both the
  store and recorder process boundary; enabling frontier runners changes the
  runner envelope without changing replay durability.
- Connectome now has a persistent Connections workspace rather than an
  in-browser endpoint field. Independent loopback RRD profiles must pass the
  public protocol and exact-instance capability handshake before CAS/journal
  persistence; retries are idempotent, key rebinding fails, non-loopback
  cleartext fails, and only credential references are retained. This is the F9
  connection authority baseline, not authenticated source switching or remote
  TLS data access.
- `vyrmQL` and `vyrmMX` now form a separate read-only query layer above the
  frozen engine port. The language requires explicit valid and known time;
  catalog binding rejects unknown types/fields and missing parameters; the
  reference planner publishes a content-addressed logical/physical plan,
  resource/authorization contract, and selected/rejected access paths. Its
  exact stamped-log executor returns deterministic bounded batches for records,
  relations, events, and claims with Memory/Fjall/native/direct-graph
  differentials. Event queries that bind the built-in global `cursor` now use a
  content-addressed `authoritative_event_cursor_lookup`: the executor validates
  the physical/logical match, requests at most one result cursor position, and
  keeps the full exact scan as fallback. A 4,096-event test proves the result
  path fits a one-change operator budget while the unbound query is denied by
  that same budget. New stamps bind an RFC 9162-style accumulator root; Memory,
  Fjall, and native commit the compact frontier and complete-subtree nodes with
  the log mutation, validate current heads without replay, and authenticate a
  cursor result with one change read plus a logarithmic inclusion path. The
  4,096-event case uses at most 13 proof nodes. Historical prefixes reconstruct
  their frontier from retained nodes but transparently report replay-plus-proof
  because scoped schema history is not yet authenticated; legacy stores
  bootstrap atomically on the next commit and retain linear fallback
  beforehand. Catalog schema-history binding and non-cursor source paths remain
  the next optimization boundaries.
  Connectome's Query Lab exposes this evidence and result
  together without mutating on GET. The explicit CLI `query` command and MCP
  `vyrm_query` tool capture the immutable read stamp before tracing, then emit
  a causal parent plus paired parse/bind, plan, and execution spans. Successful,
  parse-failed, and budget-denied trees record digests, candidates, budgets,
  plan/read coordinates, and result counts without persisting raw query or
  parameter values. Observer-effect and Memory/Fjall/native parity tests are
  green.
- The local M3–M6 data path now emits one durable causal evidence model rather
  than disconnected telemetry. Query execution brackets an exact storage-read
  span; all engines report logical work and native VyrmKV additionally reports
  bounded manifest, memtable, segment, shared-cache, block-load, and byte
  deltas. Vector search seals and revalidates its prepared plan against request
  digest and catalog revision, links the selected projection generation, and
  derives freshness from vector-family work rather than trace traffic.
  Projection publication records source/config/artifact identities. Embedding
  records inference and authoritative vector commit separately; it can rebase
  only over valid trace events and denies any intervening data/schema mutation.
  Three-engine parity, failure/denial, privacy, native reopen, and audited
  Connectome search/embedding/storage lane tests are green. Connectome prompt
  flights now add provider roots plus digest-only model/tool envelope evidence,
  and the Causal traces lens reconstructs lifecycle integrity and measured
  bottleneck candidates. Projection publication now stages exact/compact/HNSW
  bytes and atomically commits their strict catalog record plus verified object
  reference before changing the serving view; native reopen reconstructs the
  catalog and fails closed on missing or mismatched bytes. Connectome exposes
  the typed generations without raw vectors. Cluster spans, broader
  storage-write coverage, OTLP translation, and cross-run resource analysis
  remain open.
- The portable operator-knowledge gate is now executable in `vyrm-operator`.
  One immutable binding fixes instance/member, Vyrm scope, adapter/config,
  external source/relation/tenant, model space, dimensions, and projection
  generation. Search requests expose exact/HNSW/IVFFlat and iterative-scan
  controls plus a required Vyrm cursor; stale/future projections fail closed.
  Results must return bounded identities, finite scores, the sealed controls
  and actual path, plan/index digests, a PostgreSQL-snapshot digest, catalog
  identity, and an optional stable project revision. WAL LSN remains supporting
  evidence rather
  than being misrepresented as the query snapshot. The pgvector SQL builder
  quotes validated identifiers and keeps vector, tenant, and limit as query
  parameters. Vyrm vector outbox work becomes a content-addressed external work
  identity; the reference writer proves retry returns the same revision without
  reapplying payload. Traced search/sync, foreign-project and stale-revision
  denial, payload substitution, privacy, Memory/Fjall/native parity, native
  reopen, and Connectome search-lane tests are green.
- The opt-in `pgvector-postgres` feature now implements the first live endpoint
  gate. Its non-secret deployment and typed upsert/delete payloads have golden
  JSON. Explicit control migration binds source identity and per-project stable
  revision. Search uses one read-only repeatable-read transaction to capture
  the PostgreSQL snapshot, supporting WAL position, extension/relation/column/
  index catalog, `EXPLAIN (FORMAT JSON)` path, stable revision, and bounded
  results. Synchronization uses a serializable project advisory lock and one
  transaction for row mutation, revision increment, and stored replay receipt.
  A digest-pinned PostgreSQL 18/pgvector 0.8.6 CI service verifies ordered
  exact/HNSW/IVFFlat parity, project/source/cursor isolation, stale revision,
  update/delete, retry-once, and reconnect. The production connector requires
  `sslmode=require` with certificate/hostname validation, but an actual TLS
  endpoint handshake, typed payload-expression filters, server process restart,
  concurrent serialization recovery, and performance evidence remain open. No
  live pgvector superiority claim is made.
- M3 native storage persistence and semantic gates pass in the standalone
  `vyrm-kv` crate. The corrected nine-trial local general performance fixture
  passes, and compact authenticated keyspace tags close the former extended RSS
  cell; remote reproduction and a fresh read-heavy tail diagnostic remain. WAL
  v1 now has frozen CRC32C file/frame formats; atomic batch
  v2 compacts operation
  framing while the recovery reader retains the frozen v1 vector. Batches reserve
  atomic sequence ranges and expose explicit buffered/authoritative
  acknowledgments. Recovery is idempotent, repairs only a torn tail when asked,
  and fails closed on complete corruption.
  Immutable manifest v2 types canonicalize segment reachability, bind an opaque
  application format, and retain exact manifest-v1 reads. Checked-in byte/JSON
  vectors and torn/corrupt/version
  tests protect this boundary. The native mutation codec allocates one MVCC
  sequence per operation inside one atomic WAL frame; ordered memtables retain
  historical values/tombstones for repeatable point/range reads across reopen.
  Content-addressed immutable segments now retain MVCC history and fail closed
  on corruption. Locked manifest publication syncs immutable bytes before an
  atomic, separately checksummed `CURRENT` update and rejects stale parent CAS.
  Named checkpoints now pin historical manifest generations with canonical
  names, idempotent creation, and explicit directory-synced release. Database
  flush synchronizes the active WAL, writes and synchronizes a
  content-addressed segment, creates its successor WAL, then publishes the new
  manifest by expected-parent CAS. Reopen validates every reachable segment and
  replays at the manifest WAL boundary, preserving historical snapshots across
  repeated flushes. `NativeEngine` now maps the complete semantic `Engine` port
  onto manifest-authenticated compact keyspace tags and atomic native batches;
  legacy manifest-v1 stores retain their textual-prefix codec. Three-backend tests cover
  claims, projections, schema/cardinality enforcement, hash-chain replay,
  leased snapshots, stamped transactions, concurrent global CAS, flush/reopen,
  and exact `vyrmQL` execution. Snapshot-aware compaction retains versions at
  explicitly protected physical sequences; native runtime leases create and
  reconcile manifest checkpoints; GC deletes only manifests, segments, and
  WALs unreachable from `CURRENT` or a checkpoint. Deterministic crash and
  storage-full injection covers each flush and compaction durability boundary,
  followed by reopen and continued writes. Segment v3 adds independently
  authenticated LZ4 blocks, a bounded footer index, runtime tamper denial, and
  database-wide cache telemetry; block-local negative filters are derived while
  validating those authenticated bytes and skip candidate block I/O without
  changing the v3 wire format. Explicit v1/v2 readers preserve compatibility,
  and exact MVCC streaming remains under a Memtable differential. Compaction is
  now a bounded leveled step rather than an all-segment materialization: it
  selects deterministic L0/lower-level ranges, merges them through forward-only
  block cursors, partitions output at key boundaries, enforces non-overlap above
  L0, and exposes input/output bytes, debt, failures, and peak merge-buffer
  evidence. Automatic maintenance retains all MVCC history; explicit compaction
  alone prunes against its protected snapshots, retaining tombstones that still
  shadow unselected lower levels. The former Fjall/native promotion applied
  native-only maintenance and counted sparse apparent length as disk usage; it
  is now legacy diagnostic evidence. The corrected lifecycle harness measures
  active, clean-reopen, and maintained states for both engines and records
  physical allocation. Exact semantics pass. Compact sequence references,
  cached batch validation, exact-length values, one-or-many memtable chains,
  streaming one-pass WAL recovery, keyspace-local writes, and bounded first-WAL
  reservation now record 1.238× write throughput, 0.781× write p95, 1.726×
  clean-reopen read throughput, 0.603× read p95, 0.154× recovery time, 0.874×
  peak RSS, and 0.915× allocated footprint. The separate eight-profile AI-read
  matrix passes its bounded correctness, throughput, p95, and clean-reopen
  allocation gates.
  These remain workload-scoped results—not a universal database claim.
  Format-4 verification now checks every corpus ordinal in bounded read-width
  pages. A fallible borrowed scan visitor removes the prior whole-range
  materialization without weakening verification. Read-heavy and sustained are
  green across nine trials at 0.908×/0.979× RSS and 0.934×/0.764× write p95.
  Manifest-authenticated one-byte keyspace tags remove exactly 1,400,004 live
  key-payload bytes in the extended workload. Its fresh nine-trial row now
  passes at 0.984× RSS and 0.945× allocated footprint, alongside 1.390× write
  and 1.715× read throughput. A post-format read-heavy rerun is red only on
  bimodal authoritative-write p95 and remains an explicit remote-reproduction
  gate; backend-native maintained reads remain diagnostic.
  Native now also persists access/removal evidence and the invocation/
  effectiveness ledger with Fjall-equality and reopen tests, closing the
  operator-data gap that previously prevented runtime entry points from using
  the native default safely.

## Verification

CI runs the locked full workspace tests, warning-free clippy, evaluation-evidence
validation, and the `vyrm-core` serde-only dependency boundary. Compiled-binary
tests cover operator commands, hooks, explicit recovery, and both MCP eras. A
separate scheduled/manual workflow executes five isolated nine-trial
native/Fjall profiles with `--require-promotion` and retains each raw JSON
artifact even on failure. The corrected local canonical profile now passes;
remote repetition remains required before compatibility retirement.

## Deliberate limits

- The evaluation sample is a harness validation, not statistical significance:
  one trial per cell on synthetic repository fixtures.
- MCP cannot intercept another runtime's private tools. Hookless clients receive
  identical semantics through `vyrm_lifecycle` and must place that call around
  their mutations; server-owned operations remain directly enforceable.
- Runtime scopes are present in every commit and feed query. Current reasoning
  and flight composition uses the physically isolated store's
  `instance:default` scope; umbrella member routing and capability-based remote
  authorization remain deliberately non-executable.
- Native `vyrmKV` is the default for missing store paths through
  `PersistentEngine`; CLI, `vyrmd`, and Connectome expose the selected backend.
  Existing non-native directories reopen as `fjall_compatibility` and are never
  reinterpreted. Fjall source removal remains gated by explicit migration,
  mixed update/delete soak, and remote matrix reproduction—not by preserving it
  as an architectural dependency.
- M4 object storage is executable locally and has an S3-compatible semantic
  adapter, but no particular cloud transport/endpoint is certified yet.
  Automated retention-aware object GC remains gated on mapping logical runtime
  snapshot pins to physical object reachability; reclamation currently requires
  an explicit caller-proven digest set.
- Prompt-flight acceptance without a marker proves process completion only.
  Model-quality conclusions require non-trivial evaluators and repeated trials;
  the flight UI deliberately does not turn one attractive trace into a claim.
- `vyrmDS` remains a researched target subsystem. Native `vyrmKV` now has an
  executable WAL/MVCC/segment/manifest/checkpoint/compaction/GC engine and a
  locally passing promotion baseline; broader promotion and removal of the
  Fjall oracle remain open. `vyrmQL` and the exact reference slice of `vyrmMX`
  are implemented; its first narrower result path is the exact stamped event
  cursor lookup. New stamps bind an authenticated accumulator and cursor
  execution reports its actual inclusion-proof/change-read evidence, while
  other sources and schema history deliberately retain the authoritative replay
  fallback. M5 now adds a
  separate `vyrm-vector` exact dense/sparse/multivector oracle, deterministic
  filter-aware HNSW, exact reranking, immutable authenticated artifacts,
  freshness/cost planning, CAS generation retirement/quarantine, a portable
  fixture, backend differential, recall gate, and update/delete/reopen soak.
  The retained 10k×128 local profile records 0.98 recall@10 at `ef=256`, but
  also records 3.90× JSON artifact overhead; no Qdrant comparison or general
  superiority is claimed. The
  shared data-runtime v1 contract now includes content-addressed read stamps,
  leased snapshot handles,
  read-bound transactions and prospective read-your-writes views, projection
  stamps, logical retention pins, and hash-chained audit envelopes. M1 is
  complete: MemoryEngine and Fjall implement frozen/repeatable snapshot replay,
  lease/pin inventory, expiry, release, restart persistence, stamped reads, and
  a tested global-serializable same/disjoint conflict policy. A deterministic
  64-write mixed-scope backend differential is green. Physical segment pins
  are implemented in native M3. M0 through M6 are complete at their local
  executable kernel gates, and the M7 protocol/simulation plus first
  real-consensus adapter slice are green. Compact HNSW/sparse/multivector
  layouts, payload indexes, a physical GPU adapter and benchmark, real-model
  quality evidence, external vector comparison, and Multi-AZ capabilities
  remain sequenced work, not current product claims. See
  `docs/vyrmds-architecture-research.md`,
  `docs/vyrm-vector-search.md`, `docs/vyrm-embedding-edge.md`, and
  `docs/vyrm-cluster-m7.md`.
  The older symmetric-int8 experiment remains separate from TurboQuant. The
  new MSE TurboQuant path uses deterministic seeded rotation, fixed
  distribution-matched 4/2/1.5/1-bit scalar codes, bit packing, norm correction,
  asymmetric query scoring, authenticated reopen, and exact-oracle reranking.
  It does not claim the paper's residual QJL estimator or full production
  qualification; SIMD, broad recall/bias/latency evidence, and public lifecycle
  administration remain open.

- JavaScript application-run claims use script-sensitive canonical event
  subjects such as `package:bun:test`, `package:pnpm:run:typecheck`, and
  `package:npm:run:test-unit`. A strict project-owned workflow manifest now
  declares exact direct argv, scope, projection, freshness, and verification
  policy. Preflight captures its scoped read stamp, pre-tool denies undeclared,
  stale, shell-composed, or unauthorized execution, and post-tool atomically
  commits the safe observation, temporal claim, runtime outcome, and audit.
  A durable cross-process authorization lease remains open.

- A fresh exact-source audit confirms Fjall 3.1.8 remains only the compatibility
  engine and migration/differential oracle; native `vyrm-kv` owns its physical
  formats and recovery. The first AI-specific optimization resolves current
  hot memtable point reads and multi-gets before immutable blocks. Its MVCC test
  proves zero segment-cache traffic for current overwrites/tombstones and exact
  historical fallback. A five-trial 8,192-cold/128-hot local profile measured
  2.227× Fjall throughput and 0.374× Fjall p95, narrowly scoped to current hot
  point reads. Cooperative bounded leveled maintenance and authenticated
  block-local negative filters now pass crash recovery, exact MVCC tests, and
  the 20,000-operation Fjall/model mutation differential. Asynchronous
  memtable flush, family-aware maintenance/cache policy, and
  threshold-crossing latency remain open; the full mixed AI matrix is green.
  See
  `docs/vyrmkv-fjall-ai-audit.md`.

- The first persisted runtime-tracing contract is now implemented in the
  serde-only kernel. It uses W3C-width trace/span IDs, immutable
  start/annotation/finish phases, bounded typed attributes, data classes, and
  exact causal links for runtime/read/plan/projection/workflow/provider and
  external operator-knowledge coordinates. Trace events enter the ordinary
  scoped, hash-chained runtime log through a cursor-CAS recorder that repairs
  the strict schema atomically with the first event. `vyrm init` records the
  contract and a bounded initialization annotation. Hooks and the shared MCP
  lifecycle path durably write start before dispatch and finish afterward,
  including explicit denial outcomes; an interrupted native span remains
  incomplete after reopen. The explicit query surface adds observer-safe parent
  and child spans for parse/bind, planning, and exact execution; the shared
  consumable span helper now enforces the same finish semantics for lifecycle
  and queries. Local storage-read/projection/vector/embedding paths and armed
  prompt-provider/tool envelopes now join the same causal model. Connectome
  reconstructs complete, incomplete, summary, and invalid lifecycles, exposes
  exact cursor/change/audit coordinates, renders the longest measured
  root/child candidate without adding nested durations, and provides an
  explicit data-class export whose default is control-only. Complete read and
  projection stamp fields survive persisted link serialization. Schema
  migration, concurrency, initialization, denial, observer safety, secret
  non-persistence, causal visualization, crash recovery, and Memory/Fjall/
  native parity are tested. Cluster transport/artifact/trace health is now
  available through control-v4 project-node status. Connectome validates and
  retains explicitly submitted statuses as immutable per-node hash chains,
  derives restart-aware deltas and alerts, preserves bounded-window topology
  anchors, and provides freeze/rewind/raw-audit inspection. Automatic node
  collection/export, storage-write coverage, OTLP translation, persistent trace
  histories/regression budgets, cross-node artifact-catalog
  replication, and full pgvector promotion remain open. See
  `docs/runtime-tracing-operator-knowledge.md`.

## Product and instance boundary

The product umbrella is **RRFlow**. RRO owns orchestration around Automaton and
LFG; RRD owns the composed data/runtime contract; Connectome Panel is the
operator UI; and Vyrm is RRD's native LSM persistence engine. A columnar path is
a future evidence-gated engine capability, not a current claim. A major
platform receives one isolated Connectome/Vyrm instance molded to that platform.
A set of related small projects may share an umbrella instance only through
explicit membership. The default remains the existing per-checkout
`.vyrm/store`.

The current routing projection is bound to one canonical project root and
refuses implicit rebinding. The instance manifest now prevents a different
store from being paired with that root. Explicit umbrella execution is the
remaining topology work. SurrealDB inspired the record-edge, transaction,
changefeed, reference-integrity, and temporal-query capability analysis; no
SurrealDB code or database dependency was imported. Search/vector work remains
separate. A new external-process claim-runtime differential pins SurrealDB
3.0.5/SurrealKV and verifies both complete corpora. Vyrm wins the bounded
throughput, latency, restart, and RSS cells, including against Surreal's
server-reported execution time, but loses reopened allocated disk at 1.380×;
therefore no all-cells or general superiority claim is made. See
`docs/instance-topology.md`, `docs/runtime-graph.md`, and
`docs/vyrm-surrealdb-differential.md`.
