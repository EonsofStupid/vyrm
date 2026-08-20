# Runtime status — 2026-08-20

vyrm now implements the complete runtime loop described by the current plan.
It is pre-release software, but the listed behavior is executable and tested,
not roadmap language.

## Landed

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
  admission, and a 30-second ingress lifetime fail closed. A four-node loopback
  run covers election, replication, post-purge snapshot catch-up, certificate
  impersonation denial, and authenticated vote-source forgery denial. The
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
  production observability, and Multi-AZ deployment remain unclaimed.
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
- `vyrmQL` and `vyrmMX` now form a separate read-only query layer above the
  frozen engine port. The language requires explicit valid and known time;
  catalog binding rejects unknown types/fields and missing parameters; the
  reference planner publishes a content-addressed logical/physical plan,
  resource/authorization contract, and selected/rejected access paths. Its
  exact stamped-log executor returns deterministic bounded batches for records,
  relations, events, and claims with Memory/Fjall/native/direct-graph
  differentials. Connectome's Query Lab exposes this evidence and result
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
  Connectome search/embedding/storage lane tests are green. Provider/tool and
  cluster spans, richer causal rendering, persistent vector artifact catalogs,
  and the live project-scoped pgvector transport remain open.
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
- M3 native storage has passed its first strict local promotion baseline in the
  standalone `vyrm-kv` crate. WAL v1
  now has frozen CRC32C file/frame formats, atomic batch sequence ranges,
  explicit buffered/authoritative acknowledgments, idempotent recovery, and an
  explicit torn-tail-only repair path; complete corruption fails closed.
  Immutable manifest v1 types canonicalize segment reachability and bind it to
  a SHA-256 identity. Checked-in byte/JSON vectors and torn/corrupt/version
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
  onto stable prefixed keys and atomic native batches. Three-backend tests cover
  claims, projections, schema/cardinality enforcement, hash-chain replay,
  leased snapshots, stamped transactions, concurrent global CAS, flush/reopen,
  and exact `vyrmQL` execution. Snapshot-aware compaction retains versions at
  explicitly protected physical sequences; native runtime leases create and
  reconcile manifest checkpoints; GC deletes only manifests, segments, and
  WALs unreachable from `CURRENT` or a checkpoint. Deterministic crash and
  storage-full injection covers each flush and compaction durability boundary,
  followed by reopen and continued writes. Segment v3 adds independently
  authenticated LZ4 blocks, a bounded footer index, runtime tamper denial, and
  database-wide cache telemetry; explicit v1/v2 readers preserve compatibility,
  and exact MVCC streaming remains under a Memtable differential. The five-trial
  isolated Fjall/native baseline verifies
  correctness and now passes every equal-or-better cell: native is 9.1% ahead
  on write throughput and 67.3% ahead on bounded replay throughput, has lower
  write/read p95 and maintained recovery, uses 15.9% less steady RSS, and 69.0%
  less disk for this workload. A follow-on nine-trial matrix also passes all
  cells for small-batch, standard, read-heavy, and sustained profiles after
  native sequence values became self-serving and one-segment scans became
  streaming. This is still scoped append/replay evidence
  awaiting remote reproduction, mixed update/delete soak, and migration
  rehearsal—not a universal database claim.
  Native now also persists access/removal evidence and the invocation/
  effectiveness ledger with Fjall-equality and reopen tests, closing the
  operator-data gap that previously prevented runtime entry points from using
  the native default safely.

## Verification

CI runs the locked full workspace tests, warning-free clippy, evaluation-evidence
validation, and the `vyrm-core` serde-only dependency boundary. Compiled-binary
tests cover operator commands, hooks, explicit recovery, and both MCP eras. A
separate scheduled/manual workflow executes four isolated nine-trial
native/Fjall profiles with `--require-promotion` and retains each raw JSON
artifact even on failure.

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
  Fjall oracle remain open. `vyrmQL`
  and the exact reference slice of `vyrmMX` are implemented. M5 now adds a
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
  2.261× Fjall throughput and 0.379× Fjall p95, narrowly scoped to current hot
  point reads. Automatic bounded maintenance, negative filters, leveled
  streaming compaction, and the full mixed AI matrix remain open. See
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
  and queries. Connectome classifies lifecycle and query/planning events into
  its existing semantic lanes. Schema migration, concurrency, initialization,
  denial, observer safety, secret non-persistence, visual projection, crash
  recovery, and Memory/Fjall/native parity are tested. Storage/projection/
  vector/embedding/provider/cluster emission, causal rendering, export,
  retention enforcement, and full pgvector promotion remain open. See
  `docs/runtime-tracing-operator-knowledge.md`.

## Product and instance boundary

The product chain is **Automaton → LFG → Connectome**. Vyrm is Connectome's
evolving persistence/runtime substrate, not another peer in that chain. A major
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
separate. See `docs/instance-topology.md` and `docs/runtime-graph.md`.
