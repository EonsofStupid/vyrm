# vyrm — Connectome runtime substrate

The product architecture is **Automaton → LFG → Connectome**. Automaton owns
conversation/provider orchestration, LFG constructs and routes just-in-time
context, and Connectome owns persistent runtime intelligence and its developer
workbench. This repository is **Vyrm**, the persistence and lifecycle substrate
being evolved inside Connectome; it is not a fourth peer product.

The system makes operational reasoning observable and enforceable without
claiming access to a model's hidden chain-of-thought. It records goals, plans,
attempts, tool observations, decisions, verification, outcomes, context
injection, routing, and provider-visible events.

## Runtime contract

```text
prompt
  │
  ├─ attune + recall current claims
  ├─ refresh and ground source routing
  └─ goal → plan → attempt → observation → decision → verification → outcome
                         │
                         └─ deny mutation when evidence or authorization is stale
```

Vyrm provides:

- bi-temporal claims with immutable supersession and provenance;
- atomic typed runtime commits spanning claims, graph records, relations,
  lifecycle events, vectors, series samples, geospatial values, and verified
  object references;
- one hash-chained global runtime cursor with resumable, scope-filtered replay;
- bounded persisted runtime traces with W3C-width identities and typed links to
  reasoning runs, cursors, read stamps, snapshots, plans, projections,
  workflows, providers, and external operator-knowledge revisions; project
  initialization installs their strict schema, while lifecycle hooks/MCP write
  separate durable start and finish events so crashes remain visible;
- observer-safe query, storage-read, vector-plan/search, projection-publication,
  and embedding-inference/commit trace trees; native reads include bounded
  manifest/memtable/segment/cache/block deltas, while raw queries, parameters,
  vectors, filters, and embedding source bytes remain outside persisted traces;
- authoritative per-project vector artifact catalogs: exact, compact-dense, and
  HNSW bytes are content-addressed first, then their strict catalog record and
  verified object reference commit atomically through `vyrmDS`; restart
  reconstruction rejects revision gaps, substituted descriptors, non-atomic
  bindings, corruption, and missing objects before serving;
- a retention-filtered causal trace workbench that reconstructs complete,
  incomplete, summary, and invalid lifecycles from the authoritative log,
  identifies a non-double-counted measured critical-path candidate, and joins
  real provider/tool envelopes by digest without retaining their content;
- a versioned project-scoped operator-knowledge port for external systems such
  as pgvector: exact model/tenant/config binding, snapshot and stable-revision
  evidence, explicit vector-kind and path-specific metric capabilities,
  exact/HNSW/IVFFlat controls and fallback, parameterized SQL shapes, and
  idempotent vector-outbox work without a cross-store ACID claim;
- an opt-in live pgvector transport with repeatable-read snapshot/catalog
  capture, atomic project revisions and applied-work receipts, exact/HNSW/
  IVFFlat plan inspection, typed upsert/delete payloads, reconnect recovery,
  and a TLS-only production connector that rejects downgrade-capable modes;
- optimistic concurrency that rejects stale writers instead of losing updates;
- content-addressed read stamps, persisted snapshot leases, and replay that is
  bounded to the exact captured cursor/hash/schema state;
- prospective transaction views that read pending typed mutations without
  presenting them as committed evidence;
- content-addressed local and capability-explicit S3-compatible object tiers,
  with atomic reference visibility, deterministic projection outbox work,
  chained accepted-operation audit, and idempotent commit retry;
- hash-chained, typed reasoning runs;
- one-attempt/one-observation mutation authorization;
- freshness barriers and deny-by-default policy differentials;
- parser-backed routing to complete source files;
- rebuildable projections that quarantine on divergence;
- an exact dense/sparse/multivector oracle plus freshness-gated, filter-aware
  HNSW candidate generation with exact reranking and measured fallback;
- provenance/CAS-bound embedding jobs, exact model-space binding, compact
  dense mmap artifacts, scalar/AVX2 parity, and a no-network edge executable;
- canonical Multi-AZ placement/snapshot/route/transfer contracts, a
  deterministic quorum fault simulator, and a feature-gated real-consensus
  adapter that atomically applies canonical runtime commits over native VyrmKV
  and transfers that runtime truth to post-purge learners through authenticated
  physical snapshots, with explicit membership-bound placement epochs and
  bounded request identity retention; an opt-in TLS 1.3/mTLS transport binds
  every RPC to canonical workload identity and bounded frames. A real node
  executable and versioned local supervisor protocol now prove process-isolated
  crash/restart, leader replacement during a live transport partition,
  reconciliation, post-purge learner snapshot catch-up, identity confusion
  denial, hot leaf and trust-root rotation, CRL revocation, stale restart denial,
  corrupt-pointer refusal, and bounded file-backed snapshot transfer with
  crash-orphan cleanup and disk-resident immutable segment v3 with bounded
  shared-cache telemetry. Transport telemetry v1 adds reset-explicit
  per-operation/per-identity decisions, byte/latency/in-flight accounting,
  configurable global and identity rate bounds, and a golden JSON contract;
  node control v4 binds it beside artifact-session and consensus-trace health
  to the configured project, cluster, shard, Raft id, and canonical node id.
  Connectome can validate and retain imported statuses as immutable per-node
  hash chains with restart-aware deltas, alerts, runtime cursors, and audit
  evidence. A typed Raft timing policy replaces fragile
  library development defaults with per-project heartbeat/election bounds
  (automatic workload issuance, retained exporters, and production clustering
  remain gated);
- grounded replica-artifact hydration for project-scoped vector/index bytes:
  typed manifests bind source/target, placement epoch, shard snapshot, exact
  runtime read, sorted object references, and digest closure. OpenRaft's real
  mTLS path now hydrates that closure through durable, resumable, exact-offset
  sessions with independently digested chunks before sending snapshot byte
  zero; retries re-check the same closure and reuse verified content. The
  target independently scans the authenticated VyrmKV snapshot and denies
  activation if any referenced bytes are missing or corrupt. Typed bounded
  observations expose attempts, progress, duration, counts, bytes, and receipt
  identities without persisting object content; `vyrm-node` commits them as
  causal project traces through the current Raft leader. Restart-reconstructed
  receiver inventories enforce active-session/reserved-byte quotas, bounded
  receipt retention and stale-session GC while allowing distinct sessions to
  transfer concurrently;
- identical lifecycle semantics through hooks, CLI, and MCP;
- isolated per-platform instances with explicit store/root binding.

The persisted runtime graph is structural and causal, not a BM25, embedding, or
semantic-search feature. Records and relations have stable typed identities,
valid-time windows, transaction order, provenance through their enclosing
commit, and reference-integrity checks. See
[`docs/runtime-graph.md`](docs/runtime-graph.md).

## Connectome workbench

Start the local workbench:

```bash
cargo run -p connectome-ui -- --root .
# http://127.0.0.1:4387
```

To inspect it from another machine on a trusted network, bind explicitly and
open `http://HOST_OR_IP:4387` in the remote browser:

```bash
cargo run -p connectome-ui -- --root . --bind 0.0.0.0:4387 --allow-remote
```

The workbench has no authentication. Keep remote binding on a trusted network
or tunnel the loopback address over SSH. Frontier runners remain disabled
unless `--enable-runners` is also explicit.

Connectome provides twelve lenses:

| Lens | Purpose |
|---|---|
| Prompt flights | Launch, replay, freeze, inspect, and compare prompt experiments |
| Temporal stream | Freeze and scrub persisted mutations across instance scopes with their change and audit evidence |
| Schema | Inspect the persisted type, property, endpoint, uniqueness, and cardinality contract |
| Query lab | Parse, bind, explain, and execute exact bitemporal `vyrmQL` reads |
| Cluster | Freeze, rewind, and inspect validated project-node observations, topology, restart boundaries, deltas, alerts, and raw audit evidence |
| Overview | Runtime health, freshness, grounding, and active work |
| Causal traces | Parent/child span lifecycles, incomplete work, measured bottleneck candidates, exact cursors, and control-only JSON export |
| Graph | Selection-centered claim, evidence, run, flight, and source topology |
| Runs | Typed reasoning timelines |
| Claims | Current bi-temporal state and provenance |
| Routes | Ranked complete-file source routes with justification |
| Activity | Lifecycle and operator invocation evidence |

The local API also exposes the authoritative persistence layer:

| Endpoint | Purpose |
|---|---|
| `GET /api/changes?after=N&limit=N&scope=...` | Resume the verified global runtime changefeed, optionally restricted to one scope |
| `GET /api/runtime/events?limit=N` | Read the newest bounded, audit-attached temporal event projection |
| `GET /api/runtime/traces?limit=N&classes=control` | Export bounded per-project causal traces; operator/content classes require explicit inclusion |
| `GET /api/runtime/vector-artifacts?scope=...` | Inspect typed artifact generations, projection/config/source coordinates, object receipts, byte digests, and catalog revisions without returning raw vectors |
| `GET /api/runtime/schema?scope=...` | Read the active persisted schema revision for one scope |
| `GET /api/runtime/retention` | Inspect live snapshot leases and their logical GC retention pins |
| `GET /api/runtime/query?scope=...&ql=...` | Parse, bind, explain, and execute an exact bitemporal `vyrmQL` query |
| `GET /api/runtime/graph?scope=...&valid_at=T&cursor=N` | Freeze one scoped typed graph at valid time and transaction cursor |
| `GET /api/runtime/diff?scope=...&from=A&to=B&valid_at=T` | Inspect exact scoped structural change between cursors |
| `GET /api/cluster/history?limit=N` | Read bounded retained cluster observations plus the per-node baseline anchors needed for exact topology reconstruction |
| `POST /api/cluster/samples` | Validate and commit one control-v4 project-node status as an immutable, source-digested observation |
| `POST /api/demos/prompt-strength` | Persist a deterministic weak/strong trace pair for temporal playback |

The Query Lab and its GET endpoint remain read-only inspection. When the query
itself should become optimization evidence, use the explicit project-bound
operator path (also exposed as MCP tool `vyrm_query`):

```bash
cargo run -p vyrm-cli -- \
  --db .vyrm/store --json query --root . \
  --scope instance:default \
  --ql 'FROM event:runtime_trace AT VALID 18446744073709551615 KNOWN HEAD PROJECT name, phase EXPLAIN CONTRACT'
```

That path captures `KNOWN HEAD` before observability writes, then persists one
parent query span and child parse/bind, planning, execution, and physical-store
read spans. Query and parameter content remains caller-visible but trace and
invocation state retain only digests, counts, budgets, plan/read coordinates,
result metrics, and bounded storage counter deltas.

The temporal stream is a read-only projection of the newest 512 authoritative
runtime mutations in the snapshot (up to 4,096 through the event API), not an
independent telemetry store. It attaches the full mutation and available
hash-chained audit envelope. The lanes describe persisted logical mutations;
search, embedding, and storage lanes now include their persisted runtime spans.
The Causal traces lens rebuilds parent/child lifecycle state and exposes exact
event/audit coordinates. The separate Cluster lens visualizes explicitly
ingested node-status observations; it never upgrades process counters into
consensus truth, and bounded history carries a pre-window anchor per node so
rewind does not silently erase quiet nodes. Armed prompt flights add a durable
provider root, digest-only observable-envelope annotations, and tool-envelope children. They
do not imply per-operation physical WAL micro-events, automatic node polling,
unpersisted activity, or private model chain-of-thought.

At the Rust port today, `MemoryEngine` and the transitional Fjall adapter expose
the same versioned read-stamp, snapshot, and data-transaction semantics. The
portable JSON shapes are frozen in
[`golden-vectors.json`](crates/vyrm-core/fixtures/golden-vectors.json). This is
the logical snapshot boundary: every live lease has a stable retention pin.
Native `vyrmKV` must attach its physical manifests, segments, and objects to
those pins before compaction or garbage collection may reclaim old bytes.

Prompt flights accept three controlled context arms:

| Arm | Context delivered to the provider |
|---|---|
| `fresh` | New ephemeral session, zero Vyrm context |
| `pruned` | Only claims matched by the prompt, within the token budget |
| `full` | Full preflight context plus prompt-matched recall |

Fresh mode purges the experiment's **input context**, not authoritative history.
Claims, reasoning runs, and prior flights remain available for audit and replay.
This separation makes same-prompt comparisons meaningful without destroying the
evidence needed to explain them.

The reasoning flight lab runs **one prompt at a time**. Start from the weak or
strong example, or type a custom prompt, then choose an exact provider-effort
profile. Default, High, Extreme, and Ultra request `medium`, `high`, `xhigh`,
and `max` respectively. Ultra is Vyrm's label for provider `max`; it is not the
separate Codex multi-agent “ultra mode.” Repeat identical prompt bytes at a
different profile or context arm to form a comparable cohort.

The visual stage renders every captured event in a typed context/model/tool/
outcome lane. Packet height is derived from the visible event-detail and raw
envelope byte count. Freeze any packet, scrub, rewind, resume, fast-forward, or
jump to the first/latest event. The expanded micro-event exposes the complete
captured provider envelope plus provider-reported input, output, cache, and
reasoning-token fields when available. These are observable traces, never a
claim to expose private chain-of-thought. Local prompt-contract indicators are
lexical editing aids, not quality scores.

The default `observe` provider assembles and visualizes the pipeline without
launching a model. Frontier CLI execution requires an explicit opt-in:

```bash
cargo run -p connectome-ui -- --root . --enable-runners
```

Enabled Codex and Claude flights use new, non-persistent, read-only/plan-mode
sessions. The server accepts only the prompt-flight write endpoint; it does not
expose a general mutation API. It binds to loopback unless remote access is
explicitly acknowledged.

## Baseline experiments

To determine how much vague context actually works:

1. Write one prompt and one observable acceptance marker.
2. Run the exact prompt in `fresh`, `pruned`, and `full` arms.
3. Keep provider, repository revision, and context budget fixed.
4. Compare acceptance, provider tokens, context tokens, tool calls, and latency.
5. Repeat before promoting any result into a product claim.

The prompt digest becomes the cohort identity, so exact-prompt arms are grouped
automatically. Every event can be frozen and expanded; the event ledger contains
only externally observable runtime/provider data. See
[`docs/prompt-flight-experiments.md`](docs/prompt-flight-experiments.md) for the
measurement contract.

## Crates

| Crate | Responsibility |
|---|---|
| `vyrm-core` | Claim, reasoning, typed runtime graph, durable trace, traversal, and differential contracts; serde-only boundary |
| `vyrm-store` | `vyrmDS` coordination plus native `vyrmKV`, transitional Fjall, and memory adapters; unified atomic commits, content-addressed objects, outbox/audit, sequences, projections |
| `vyrm-operator` | External operator-knowledge contracts, exact reference adapter, optional live pgvector transport, SQL planning, and idempotent upsert/delete synchronization |
| `vyrm-graph` | Parsing, incremental freshness, grounding, source routing |
| `vyrm-node` | Runtime lifecycle, instance binding, policy, append-only reasoning composition |
| `vyrm-cli` | Operator surface |
| `vyrmd` | stdio MCP surface for hookless runtimes |
| `vyrm-eval` | Paired frontier-runtime evaluation evidence |
| `connectome-ui` | Prompt-flight recorder and runtime workbench |

For JS/TanStack workflows, successful and failed tool runs are journaled under
canonical, manager-specific subjects—`package:bun:*`, `package:pnpm:*`,
`package:npm:*`, and `package:yarn:*`. Script names remain part of the identity,
so `pnpm run typecheck` and `pnpm run test` do not overwrite each other. These
events now cross a project-owned policy gate. `.vyrm/workflows.toml` binds exact
direct argv to the instance scope, required source-routing projection, strict
freshness, and verification policy. Preflight injects a scoped `ReadStamp`,
pre-tool denies absent/corrupt/undeclared or shell-composed package execution,
and post-tool commits the digest-bound observation, temporal status claim,
runtime change, outcome, and audit atomically. See
[`docs/package-workflows.md`](docs/package-workflows.md).

Vyrm owns the storage contract. `vyrm_store::NativeEngine` now implements the
same `Engine` port as the in-memory reference and transitional Fjall adapter;
claim recall, projections, schema enforcement, hash-chained commits, snapshots,
concurrent CAS, restart, and exact `vyrmQL` results run through a three-backend
differential. Fjall remains live until broader benchmark regression runs
reproduce the native engine's first strict local equal-or-better pass. That
checked-in five-trial workload has native ahead in write/read throughput,
write/read p95, maintained recovery, steady RSS, and disk while preserving
correctness. A nine-trial small-batch/standard/read-heavy/sustained matrix also
passes every cell. These are scoped append/replay M3 results, not a general
database-superiority claim. A separate 20,000-operation physical differential
now proves put/overwrite/delete behavior across reopen and compaction against
both Fjall and an independent ordered-map oracle.
The canonical `PersistentEngine` now creates native stores for missing paths and
reopens them by their authenticated `CURRENT` marker. CLI, `vyrmd`, and
Connectome use that selector. Existing non-native directories remain on the
explicit `fjall_compatibility` path until migration; no bytes are guessed or
silently converted. `vyrm storage migrate|status|rollback` provides an
authenticated, resumable 18-keyspace migration with invisible staging,
retained Fjall/archive evidence, divergence-safe rollback, and fail-closed
normal opens during cutover. Native access/removal evidence and the invocation/
effectiveness ledger now match Fjall and survive reopen, so the default does not
drop trigger-optimization evidence.

## Instance boundary

Each major platform receives a dedicated Vyrm/Connectome instance. Related
small projects may eventually share an explicitly enumerated umbrella, but
filesystem proximity never grants membership. Runtime entry points refuse a
foreign store/root pairing. See [`docs/instance-topology.md`](docs/instance-topology.md).

Initialize a dedicated checkout:

```bash
cargo run -p vyrm-cli -- \
  --db .vyrm/store \
  init --harness claude-code --root .
```

## Evidence and status

The current controlled evaluation contains 8/8 successful trials with zero
paired regressions across two providers, two repositories, and stale-evidence
and post-compaction scenarios. It validates the harness; it is not yet a
statistically significant model-performance claim.

- [`STATUS.md`](STATUS.md) — executable current state and deliberate limits
- [`SPEC.md`](SPEC.md) — authoritative contract and vocabulary
- [`PLAN.md`](PLAN.md) — historical execution journal and measured decisions
- [`docs/clyffy-kernel-alpha.md`](docs/clyffy-kernel-alpha.md) — release gates,
  lifecycle naming, deployment tiers, competitive proof, and clean Clyffy handoff
- [`docs/runtime-tracing-operator-knowledge.md`](docs/runtime-tracing-operator-knowledge.md)
  — persisted trace contract, HelixDB comparison, per-project operator surface,
  and the non-authoritative pgvector knowledge-adapter boundary
- [`docs/vyrmds-architecture-research.md`](docs/vyrmds-architecture-research.md)
  — pinned upstream research, target data-runtime boundaries, and gated build
  sequence for `vyrmQL`/`vyrmMX`/`vyrmDS`/native `vyrmKV`
- [`docs/vyrmds-object-contract.md`](docs/vyrmds-object-contract.md) — M4
  canonical vector/series/geo/object values, object publication, atomic
  visibility, outbox/audit, failure recovery, and adapter evidence
- [`docs/vyrm-vector-search.md`](docs/vyrm-vector-search.md) — M5 exact/ANN
  semantics, projection lifecycle, filtered recall/latency/memory evidence, and
  explicit limits
- [`docs/vyrm-embedding-edge.md`](docs/vyrm-embedding-edge.md) — M6
  provenance-bound embedding jobs, model-space binding, compact mmap vectors,
  accelerator admission, and offline edge evidence
- [`docs/vyrm-cluster-m7.md`](docs/vyrm-cluster-m7.md) — M7 placement,
  consistency, snapshot-vector, transfer, reshard, deterministic fault
  simulation, and real-consensus adapter evidence with explicit production
  limits
- [`docs/vyrmkv-format.md`](docs/vyrmkv-format.md) — frozen native WAL, segment,
  recovery, manifest, and authenticated physical snapshot-bundle contracts
- [`docs/vyrmkv-benchmark.md`](docs/vyrmkv-benchmark.md) — isolated
  Fjall/native methodology, baseline, and promotion verdict
- [`docs/vyrmkv-fjall-ai-audit.md`](docs/vyrmkv-fjall-ai-audit.md) — exact
  Fjall/native boundary, AI-specific physical opportunities, and hot-set proof
- [`eval/results/2026-08-18-summary.json`](eval/results/2026-08-18-summary.json)
  — retained evaluation evidence

## Verification

```bash
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo run --locked -p vyrm-eval -- verify \
  eval/results/2026-08-18-summary.json
```

Browser acceptance runs against a live workbench:

```bash
CONNECTOME_URL=http://127.0.0.1:4387 \
  bunx --bun playwright test \
  crates/connectome-ui/tests/workbench.spec.js --workers=1
```

Pre-release. The runtime contract is executable and tested; umbrella execution,
larger repeated frontier evaluations, and measured high-volume graph rendering
remain open work.
