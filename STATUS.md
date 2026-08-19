# Runtime status — 2026-08-19

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
  differentials.
  Connectome's Query Lab exposes this evidence and result together.
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
  followed by reopen and continued writes. Segment v2 adds authenticated LZ4
  block compression with explicit v1 read compatibility; sparse immutable
  indexes replace decoded ordered maps and exact MVCC streaming remains under a
  Memtable differential. The five-trial isolated Fjall/native baseline verifies
  correctness and now passes every equal-or-better cell: native is 10.4% ahead
  on write throughput, 1.3% ahead on bounded replay throughput, has lower
  write/read p95 and maintained recovery, uses 9.4% less steady RSS, and 86.0%
  less disk for this workload. A follow-on nine-trial matrix also passes all
  cells for small-batch, standard, read-heavy, and sustained profiles after the
  sparse index stopped cloning keys. This is still scoped append/replay evidence
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
  executable kernel gates. Compact HNSW/sparse/multivector layouts, payload
  indexes, a physical GPU adapter and benchmark, real-model quality evidence,
  external vector comparison, and Multi-AZ capabilities remain sequenced work,
  not current product claims. See `docs/vyrmds-architecture-research.md`,
  `docs/vyrm-vector-search.md`, and `docs/vyrm-embedding-edge.md`.

- JavaScript application-run claims now use script-sensitive canonical event
  subjects such as `package:bun:test`, `package:pnpm:run:typecheck`, and
  `package:npm:run:test-unit`. They are observed through the existing post-tool
  lifecycle; workflow declarations and pre-tool enforcement against these
  identities are the next trigger layer, not silently implied by naming alone.

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
