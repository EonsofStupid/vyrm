# vyrm — Work Plan

| Field | Value |
|-------|-------|
| Status | Runtime completion plus persisted schema pass landed 2026-08-19; historical execution journal retained below. |
| Governs | Sequencing and acceptance of work against `SPEC.md` |
| Does not govern | Contracts, terminology, or requirements. Those are `SPEC.md` only. |

This document sequences work. It does not restate requirements. Where the two
disagree, `SPEC.md` is authoritative and this document is wrong.

> **Current-state overlay (2026-08-18).** The original grounding table below is
> retained as historical evidence and is not the current status board. The
> completed runtime now includes `vyrm-node`, persisted graph freshness,
> hash-chained reasoning runs, deny-by-default contract differentials, `vyrmd`
> MCP, controlled provider evaluations, and CI regression checks. See
> `STATUS.md` for current state and `eval/results/2026-08-18-summary.json` for
> measured evaluation evidence.

> **Instance-topology overlay (2026-08-18).** vyrm/connectome is rolled out as
> an isolated, platform-molded instance for each major platform. Related small
> projects may use an explicitly configured umbrella instance. There is no
> implicit estate-wide store. The active follow-on is to enforce this boundary
> with a versioned instance manifest and scoped runtime state. SurrealDB and
> Qdrant capability adoption is postponed. See `docs/instance-topology.md`.

> **Prompt-flight overlay (2026-08-18).** Connectome now records controlled
> fresh/pruned/full prompt flights. Fresh means a new provider session with zero
> injected Vyrm context; it never deletes the authoritative claim or reasoning
> ledgers. Each externally observable micro-event is replayable, and exact
> prompts form comparison cohorts. The next evidence pass is repeated vague-
> prompt trials with non-trivial acceptance markers, not additional UI chrome.
> See `docs/prompt-flight-experiments.md`.

> **Temporal runtime-graph overlay (2026-08-19).** Vyrm now persists typed
> records, relations, claims, and lifecycle events through one atomic
> `RuntimeCommit`. Every mutation receives a monotonic global cursor and joins a
> verified hash chain; stale expected cursors fail rather than overwrite.
> Reasoning and prompt-flight ledgers have moved from whole-projection rewrites
> to this append-only log. Connectome exposes cursor replay, graph-at-time, and
> graph differential endpoints. A strict persisted schema registry now denies
> unknown types/properties, wrong value types, illegal endpoints, temporal
> uniqueness collisions, and edge-cardinality violations; its migrations are
> atomic runtime mutations and its active contract is visible in Connectome.
> This work deliberately excludes BM25, embedding execution, and semantic
> retrieval. Content-addressed artifact bytes/backreferences landed in M4;
> incremental grounded graph lenses and member-scoped authorization remain. See
> `docs/runtime-graph.md`.

> **Native-engine decision (2026-08-18).** Fjall is transitional and will be
> removed in favor of a Vyrm-native substrate. Existing Fjall measurements and
> tests remain comparison evidence, not an architectural veto. The
> `vyrm_store::Engine` differential is now the migration harness: the native
> engine must preserve wire/temporal/runtime semantics and meet or beat the
> compatibility adapter on latency, throughput, durability, recovery, and
> memory. Licensing is not the optimization boundary; measured behavior is.

> **Data-runtime implementation overlay (2026-08-19).** The proposed `vyrmQL`,
> `vyrmMX`, `vyrmDS`, and native `vyrmKV` names have explicit boundaries; they
> are not represented as complete subsystems. A pinned primary-source
> review of Qdrant, SurrealDB, HelixDB, SlateDB, Lance, DataFusion, OpenDAL,
> cuVS, and TiKV produced an adopt/adapt/reject matrix and milestones M0–M8.
> M0/M1 now have a versioned portable contract (`ReadStamp`, leased
> `SnapshotHandle`, `DataTransaction`, `ProjectionStamp`, and hash-chained
> `AuditEnvelope`), checked-in golden JSON, and matching MemoryEngine/Fjall
> behavior for frozen replay, expiry, restart persistence, and concurrent CAS.
> M1 is now closed with stamped transaction reads, prospective read-your-writes,
> same/disjoint global-CAS coverage, repeatable paged replay, and logical
> retention-pin inventory exposed to Connectome. Physical segment attachment
> belongs to native M3. M2 is now closed with explicit-time `vyrmQL`, catalog
> binding, deterministic logical/physical `vyrmMX` plans, exact stamped-log
> execution, budgets, diagnostics, backend/direct-API differentials, and a
> browser-visible Query Lab. Native `vyrmKV` M3 implementation and its first
> local promotion gate are now closed; repeated CI/broader workload evidence and
> the backend-default decision remain. Vector, object, and cluster work stay
> sequenced behind that explicit decision. See
> `docs/vyrmds-architecture-research.md`.
> M3 is now active in a standalone `vyrm-kv` crate. Its v1 WAL, atomic mutation
> batch, and immutable-manifest formats are frozen by checked-in vectors.
> Checksummed replay, explicit torn-tail repair, MVCC allocation, ordered
> memtables, repeatable snapshots, and reopen are green. Content-addressed
> immutable segments now retain MVCC history, and locked manifest publication
> uses expected-parent CAS plus sync-before-`CURRENT` ordering. Checkpoints and
> segment/WAL lifecycle integration followed. Named, checksummed checkpoints
> pin historical manifests and release explicitly. Database flush now orders
> WAL sync, content-addressed segment sync, successor-WAL creation, and manifest
> CAS publication; reopen validates reachable segments and replays only the
> manifest-declared WAL range. `NativeEngine` now implements the complete
> semantic port over prefixed native keys and one atomic `vyrmKV` batch per
> commit. Memory/Fjall/native differentials cover claims, projections, runtime
> schemas and hash chains, snapshots, restart, concurrent CAS, and exact query
> results. Snapshot-aware compaction retains only versions visible at protected
> physical snapshots; runtime leases create/reconcile manifest checkpoints; GC
> derives reachability from `CURRENT` plus those pins. Crash and storage-full
> injection now cover every flush and compaction durability boundary. Native
> performance proof has a five-trial isolated-process baseline. After segment-v2
> compression, sparse immutable reads, hardware CRC32C, and fresh steady-state
> probe processes, native passes correctness and every strict equal-or-better
> cell: write/read throughput, write/read p95, maintained recovery, steady RSS,
> and disk. This closes the local M3 gap; Fjall remains live as an oracle until
> CI and broader workload matrices reproduce the result. A second nine-trial
> small-batch/standard/read-heavy/sustained matrix now passes every strict cell;
> the sparse index holds byte ranges into canonical segment storage rather than
> duplicate heap keys. Remote repetition, mixed mutation soak, and migration
> rehearsal gate Fjall source removal—not another append/replay microbenchmark.
> The default switch itself is now safe and explicit: `PersistentEngine` creates
> native stores for missing paths, reopens native by authenticated `CURRENT`,
> and keeps existing non-native directories on `fjall_compatibility`. CLI,
> `vyrmd`, and Connectome share that selector and expose its decision.
> The product handoff, canonical package-event grammar, alpha release gates,
> deployment/update tiers, and separate SurrealDB/Qdrant proof protocols are
> frozen in `docs/clyffy-kernel-alpha.md`.

> **M4 unified-data overlay (2026-08-19).** The local executable gate is closed.
> Canonical vector, series, geo, and object-reference mutations now join the
> same atomic cursor/hash-chain commit as claims, records, relations, and
> events. Each accepted commit atomically persists latest-value indexes,
> deterministic projection work, chained audit, and an idempotent outcome.
> `vyrmDS::DataRuntime` implements stage → verify → commit visibility over a
> synced local content-addressed store and a capability-explicit S3-compatible
> adapter. Three-engine mixed-family rollback/reopen tests, publication/commit
> fault injection, quarantine, orphan reclamation, and local/S3 semantic
> differential are green. Live cloud transport certification and automatic
> retention-aware object GC are deployment work. See
> `docs/vyrmds-object-contract.md`; the M5 overlay below records the next gate.

> **M5 vector/search overlay (2026-08-19).** The local semantic and evidence
> gate is closed. `vyrm-vector` now supplies the borrowing exact
> dense/sparse/multivector oracle, bounded typed filters, immutable exact
> segments, deterministic dense HNSW with filter-aware admission and exact
> reranking, a freshness-aware planner/executor, unified CAS generation
> lifecycle, scalar-quantization experiment, portable fixture, backend
> differential, recall gate, and eight-generation update/delete/reopen/
> replacement soak. The retained 10k×128 profile reaches 0.98 recall@10 at
> `ef=256` while exposing a 3.90× JSON-artifact overhead; this is a local
> baseline, not a Qdrant superiority claim. Compact binary/mmap, SIMD/GPU,
> sparse/multivector ANN, embeddings, and external comparison remain gated.
> See `docs/vyrm-vector-search.md`.

> **M6 embedding/edge overlay (2026-08-19).** The local kernel gate is closed.
> `vyrm-embed` now provides source-digest/model/read-stamp-bound jobs, two-read
> inference race detection, final commit CAS, and a no-network local FastEmbed
> adapter behind an optional feature. Search requests and projections can bind
> exact model identity. Compact dense v1 adds authenticated aligned `f32`, true
> read-only mmap, atomic publication, and scalar/AVX2 parity. Accelerator output
> is untrusted until byte-identical to the CPU artifact; fallback is explicit.
> `vyrm-edge` supplies one-call offline embed/search and retained 10k×128
> binary/RSS/artifact/latency evidence. No physical GPU or real-model quality
> result is claimed. See `docs/vyrm-embedding-edge.md`.

> **M7 cluster-contract overlay (2026-08-19).** The first protocol/simulation
> slice is executable. `vyrm-cluster` freezes canonical three-zone placement,
> consistency requests, partial-order snapshot vectors, route evidence,
> snapshot-plus-WAL transfer, metadata-indexed resharding, and an explicit M7
> denial for cross-shard writes. Its deterministic single-term/per-shard model
> injects partition, delay, duplication, reorder, crash, restart, clock skew,
> and disk loss, and enumerates quorum paths against every tolerated single
> disk loss. This is not a production consensus, networking, election, or
> Multi-AZ claim. See `docs/vyrm-cluster-m7.md`.

> **M7 real-consensus overlay (2026-08-19).** OpenRaft `0.9.25` is pinned behind
> an opt-in feature and adapted to authoritative VyrmKV storage. Votes, logs,
> committed pointers, application state, and snapshots survive the upstream
> conformance suite. A real four-node in-process run covers election, quorum
> writes, log-purged snapshot catch-up, majority-side failover, post-failover
> commit, and voter replacement. Command application binds shard, placement
> epoch, full idempotency identity, payload digest, and optional commit-index
> CAS. Production transport, unified `RuntimeCommit` dispatch, bounded state,
> independent-process chaos, and Multi-AZ evidence remain open.

> **M7 physical-snapshot prerequisite (2026-08-19).** VyrmKV snapshot-bundle
> v1 now exports a flush-bounded manifest plus every authenticated immutable
> segment in a deterministic binary envelope. Installation validates the full
> closure, syncs content-addressed segments and an empty continuation WAL, then
> atomically advances a new local manifest rather than importing the source's
> history. Round-trip, corruption/truncation/stale denial, idempotent reinstall,
> target replacement, reopen, continued writes, and crash/storage-full tests at
> all three install boundaries are green. This froze the physical prerequisite
> consumed by adapter v3 in the following overlay.

> **M7 transferable-runtime overlay (2026-08-19).** Adapter format `v3` now
> gives canonical state and node-local Raft history separate VyrmKV ownership
> domains. Votes, logs, committed/purged cursors, and the content-addressed
> current-snapshot cache remain local; the applied cursor and canonical runtime
> mutations remain atomic in the transferable state database. OpenRaft builds
> and installs authenticated snapshot-bundle v1 bytes, binds metadata and
> snapshot id before manifest publication, and preserves the target vote.
> Upstream storage conformance plus corrupt/forged/stale/duplicate/reopen tests
> pass. A real four-node run snapshots a `RuntimeCommit`, purges the leader log,
> catches up a new learner, and reopens the runtime truth on all four nodes.

> **M7 epoch/retention overlay (2026-08-19).** Adapter format `v4` makes
> placement initialization and advance an explicit replicated operation. Epoch
> 1 must match the applied OpenRaft voter canonical ids and zones; later epochs
> must be exact successors and bind the same way. Normal operations can no
> longer establish an epoch implicitly, and later voter-binding changes
> invalidate it until the next placement transition while learner-only metadata
> does not. Full identity responses are retained for exactly 4,096 applied-log
> positions, with deterministic pruning in the state machine and its snapshots;
> canonical runtime commit identity remains independently durable.
> Initialization, successor, skipped/stale, membership mismatch/rebinding,
> duplicate, retention-boundary, snapshot, and reopen evidence pass.

> **M7 authenticated-transport overlay (2026-08-19).** The opt-in
> `openraft-transport` feature implements real TCP RPCs over TLS 1.3 mutual
> authentication. Transport v1 binds one exact SPIFFE-style URI identity to the
> CA-validated leaf, statically authorizes numeric/canonical node pairs, binds
> cluster/shard/source/target/request digest and the inner Raft vote source, and
> enforces bounded frames, client hard TTLs, ingress lifetime, and concurrent
> admission. A four-node loopback run elects and replicates over this transport,
> transfers a physical runtime snapshot to a post-purge learner, and denies
> certificate/envelope confusion and authenticated vote-source forgery.
> Independent-process chaos, rotation/revocation, production telemetry, and
> Multi-AZ evidence remain open.

> **M7 canonical-runtime overlay (2026-08-19).** Adapter format `v2` first added a
> typed `runtime_commit` operation. `NativeEngine` now exposes a validated
> no-write plan, allowing canonical mutations, audit/outbox work, the Raft
> applied cursor, response, and idempotency record to publish in one
> authoritative VyrmKV WAL frame. Reopen, duplicate, stale-cursor denial, and
> same-frame differentials are green. A real three-voter run reopens identical
> canonical runtime truth through `NativeEngine` on every voter. Runtime-bearing
> snapshots are now transferred by adapter v4's physical ownership split and
> the opt-in authenticated transport. Independent-process chaos, credential
> lifecycle, and Multi-AZ evidence remain open.

## 1 · Grounding result

State recomputed from source on 2026-08-10 and differenced against the
specification, in the sense of `SPEC.md` §8.3. Evidence is a file path or its
absence, not recollection.

| Spec | Requirement | State | Evidence |
|------|-------------|-------|----------|
| §3 | `append_batch`, `assert` | Implemented | `store.rs` |
| §3 | `as_of`, `current`, `history` | Implemented | `temporal.rs`, `store.rs` |
| §3 | `observe` | Implemented | `store.rs` |
| §3 | `promote`, `gate` | **Missing** | no `vyrm-gate` |
| §4 | In-process linkage | Implemented | `vyrm-core` is a library crate |
| §4.1 | napi-rs adapter | **Missing** | no `vyrm-node` |
| §5 | `vyrm-core`, `vyrm-store` | Implemented | `crates/` |
| §5 | `vyrm-cli` | Implemented | `crates/vyrm-cli` |
| §5 | `vyrm-graph` | Implemented | `crates/vyrm-graph`: attunement, tree-sitter extraction, routing, freshness, grounding |
| §5 | `vyrm-gate`, `vyrm-node`, `vyrmd` | **Missing** | absent |
| §5 | Modularity criterion | Verified | `cargo tree`: core depends on `serde` alone |
| §6 | Claim model, retirement | Implemented | `claim.rs` |
| §6.1 | Two-timeline key encoding | Implemented | `key.rs`, `tests/bitemporal.rs` |
| §6.2 | Resolution, half-open intervals | Implemented | `temporal.rs` |
| §7 | `producer` mandatory | Implemented | `claim.rs` |
| §7 | Access records | Implemented | `store.rs` |
| §7 | Removal candidacy by query | Implemented | `gc.rs`, `tests/removal.rs` |
| §7.1 | Durability classes | Implemented | `keyspaces.rs`; `sequence_index` replaces `events` |
| §8.1 | Group commit, bounded interval | Implemented | `writer.rs` |
| §8.1.1 | Producer patterns | Measured | `examples/sparse_latency.rs` |
| §8.2 | Incremental rebuild | Unblocked, not implemented | `claims_in_range` exists |
| §8.3 | Grounding | **Blocked** | no projection to ground |
| §8.4 | Differentials | Unblocked, not implemented | `claims_in_range` exists |
| §9 | Tier fields present and inert | Implemented | `claim.rs` |
| §9.1 | Gate evaluation | **Missing**, policy open | §9.1 records the predicate set as unsettled |
| §10 | Recall, recall set | **Missing** | absent |
| §11 | Corrections 1–5 | Implemented | `store.rs`, `tests/` |
| §11 | Correction 6, bounded accept queue | Not applicable yet | no daemon exists to bound |
| §12 | Durability, resolution, isolation, throughput, modularity | Verified | 77 tests, `examples/` |
| §12 | Removal candidacy | Verified | `tests/removal.rs` (8 tests) |
| §13 | Manual triggers, recorded | Implemented | `invocation.rs`, `vyrm-cli`, `tests/operator_surface.rs` |
| §13.1 | Effectiveness ledger | **Missing** | absent |
| §13.2 | Content-addressed objects | **Missing** | absent |
| §14 | Clyffy consumer | **Missing** | absent |

Implemented and verified: 118 tests, clippy clean at `-D warnings`. The former
`durability.rs::an_unflushed_index_is_as_empty_as_the_claims_it_indexes` flake
was resolved on 2026-08-19. The group-commit worker could observe a non-empty
queue before entering its timed wait and commit without a full batch, elapsed
delay, or explicit flush. The worker now carries an explicit flush-request bit
and waits for one of the documented triggers; a direct long-delay test and
repeated SIGKILL durability runs cover the boundary.

## 2 · Findings from this audit

### F1 · The `events` keyspace is allocated for an undefined term — RESOLVED (Step 1)

`store.rs` opens `events` and §7.1 assigns it `SyncAll`, but nothing writes to it
and §1.2 does not define **event**. The field carries `#[allow(dead_code)]`.

This is inherited from the prior runtime, where events were the primary record.
In vyrm the claim is the record. The keyspace is either the sequence index
required by F2, or it is vestigial.

**Resolved as D-3.** The keyspace became the sequence index required by F2 and
was renamed `sequence_index`. A keyspace named for an undefined term would have
reintroduced the vocabulary drift §1.2 exists to prevent.

### F2 · No sequence-ordered scan exists — RESOLVED (Step 1)

Claims are keyed by subject, predicate, and both timestamps. There is no path
from a sequence range to the claims in it, so "claims between two watermarks" is
unanswerable.

§8.2 (rebuild applies claims in `(watermark, current]`) and §8.4 (differential
between two watermarks) are both specified against a scan that does not exist.
§9 change sets inherit the block, since a change set is a differential.

This is the single highest-fan-out gap: three specified capabilities depend on it
and none can begin without it.

### F3 · The core hypothesis is untested and does not depend on the blocked work

The proposition that structured recall costs materially fewer tokens than
unstructured context (§13.1) is the reason this system exists. Nothing validates
it yet.

It depends on claims, which exist. It does **not** depend on projections,
grounding, differentials, or gates. Sequencing by architectural layer would place
the thesis test last; sequencing by risk places it early.

## 3 · Dependencies

```text
            ┌── gc (§7, §12) ─────────────────────────── independent
            │
claims ─────┼── sequence index (F2) ──┬── differentials (§8.4) ──┐
  ✅        │                          ├── rebuild (§8.2) ── grounding (§8.3)
            │                          └──────────────────────────┴── change sets (§9)
            │                                                          │
            │                                              gate policy ┘  ← OPEN, yours
            │
            └── operator surface (§13) ── recall (§10) ── effectiveness ledger (§13.1)
                                              │
                                    content-addressed objects (§13.2)

  napi adapter (§4.1) ── Clyffy (§14)     ← needs a stable core and store API
```

## 4 · Sequence

Ordered by risk retired per unit of work, not by architectural layer.

### Step 1 · Sequence index — COMPLETE

Resolves F1 and F2. A keyspace mapping `{sequence:020}` to a claim key, written
in the same transaction as the claim so the mapping cannot diverge from the
watermark.

- **Unblocks** §8.2, §8.4, §9
- **Acceptance met:** `tests/sequence_index.rs` (9 tests) and
  `tests/durability.rs` (5 tests). A range returns exactly the claims appended in
  it; index and watermark agree after `kill -9`; a full scan reproduces every
  stored claim, differenced against `MemoryClaims`.

### Step 2 · Removal candidacy (`gc --dry-run`) — COMPLETE

Completes §7 and the outstanding §12 row. Independent of everything else, and
small.

- **Acceptance met:** `tests/removal.rs` (8 tests). A pair accessed inside the
  interval is never a candidate; one with no access always is; every verdict
  carries its evidence (access count, last access, last reader, claim count).
- **Analysis only.** No removal path exists and none is specified: the fate of a
  retired claim, and the interaction between removal and promotion, are unsettled.
- **F4 fixed en route:** the access-record encoding used `/` as a field
  separator, which identifiers may contain, so subject `a/b` with predicate `c`
  encoded identically to subject `a` with predicate `b/c`. Attribution would have
  retained the wrong pair. Now `\x00`-separated, matching the claim key.

### Step 3 · Operator surface (`vyrm-cli`) — COMPLETE

§13 stage 1 requires explicit operator invocation with every invocation recorded.
No trigger may be automated before its manual record justifies it, so this
precedes all automation.

- Commands shipped: `assert`, `as-of`, `history`, `status`, `gc`, `invocations`
- **Acceptance met:** `tests/operator_surface.rs` (8 tests), driving the compiled
  binary rather than a test-only entry point. Every invocation records trigger,
  arguments, outcome, and duration; failures are recorded as failures; ordinals
  are monotonic across processes; the log is queryable as text and JSON.
- **Deviation from the planned command list.** `recall` belongs to Step 4 and is
  not yet implemented. `flush` was dropped: each CLI invocation is its own
  process and every write commits synchronously through `append_batch`, so the
  command would have done nothing. `invocations` was added, since "the record is
  queryable" requires a way to query it.
- Recording wraps execution in one place in `main`, so a command cannot be added
  that forgets to record itself.

### Step R · Routing projection (`vyrm-graph`) — COMPLETE

The operator-facing routing layer: attune to a repository, maintain a symbol
index incrementally, answer a query with a ranked file list read in full.
Runs beside the claims-layer sequence rather than within it.

**Complete, with evidence:**

- Attunement, incremental refresh, grounding, projection-only routing:
  `tests/freshness.rs` (7 tests). Routing answers from the projection, never
  disk at query time; grounding detects a stale projection and does not repair
  it.
- Parser-based extraction (tree-sitter), replacing the line-based v1:
  official-organization grammars for Rust, TypeScript, TSX, JavaScript,
  Python; the tree-sitter-grammars organization's Svelte grammar locating
  script elements re-parsed as TypeScript. 10 extraction tests including
  multi-line attribution, JSX, string/comment exclusion, and Svelte line
  offsets.
- Ranking: line-budget fill (`Index::route_budget`, first-fit in rank order,
  top file always included) and reference-graph centrality (PageRank over
  file-to-file reference edges, fixed 30 iterations for determinism) as a
  sort tie-breaker. `tests/ranking.rs` (5 tests): definer ties break toward
  the leaned-on file, centrality never routes an unrelated file, a
  declaration still outranks a central heavy caller, budget fill returns the
  top file even over budget and back-fills smaller files past an oversized
  one.

**Measured outcome of the extraction swap (2026-08-10, identical queries to
baseline):** lines-to-read ratio unchanged — 5.61x on the 1,616-file
repository, 1.76x on this one. Distinct definitions fell 6,521 → 5,773 (748
phantom definers removed: locals and commented declarations); full-index cost
rose 334 ms → 4,752 ms, absorbed by incremental refresh (no-op 68 ms). The
gap to the published ~10x is therefore not extraction's; it lies in ranking
and in query classes with no declaration site.

**Measured outcome of the ranking change (2026-08-11, identical queries, the
1,616-file repository unchanged since baseline):** budget fill at 1,000 lines
took the lines-to-read ratio from 5.61x to 13.87x, with the declaration still
ranked first on every query that has one — past the published ~10x, with the
caveat that the budget bounds the denominator by construction; the evidence
that the bound is not bought with wrong answers is the unchanged
declaration-first column and what filled the budget (`Platform`: 3 files,
998 lines, versus 3,983 under the fixed five). Centrality was decomposed
separately: weighted into the score at 30.0 it cost lines on every ratio
(fixed-5 5.61x → 4.20x, budget 14.07x → 13.32x, central files are
systematically larger) and improved declaration-first on no query — a
recorded negative result. It ships as a sort tie-breaker only (final:
fixed-5 5.61x per-row identical to baseline, budget 13.87x). On this
repository: fixed-5 1.79x, budget 2.22x — the corpus itself changed this
session (ranking code and tests added), so those rows are not
baseline-comparable, and the layer remains not worth engaging at 33 files.

- Filename-level entities (`module_entity` in `route.rs`, generalizing the
  Svelte special case): a module file is the definition site of its stem
  (evidence, 2026-08-12: `terminology` exists in the reference repository
  only as two filename stems with eight importers, nothing inside either
  file bears the name); an entry file (`index.ts`, `mod.rs`, `__init__.py`)
  declares its directory; compound extensions declare the first stem
  segment; non-identifier stems declare nothing; a real declaration line is
  never shadowed. `tests/entities.rs` (5 tests).

**Measured outcome of entity synthesis (2026-08-12, identical queries):**
`terminology` flipped to declaration-first in both modes — both
`terminology.ts` files rank 1–2 as definers, importers behind them — so
every query present in the tree now routes declaration-first; the only NO
left is `SubsystemBadge`, which is correct (absent from this clone).
Distinct symbols 5,773 → 6,122 (+349 filename entities). Fixed-5 ratio
moved 5.61x → 5.37x — *down*, because the old `terminology` row was cheap
but wrong (615 lines, no definer); routing the actual definition site costs
465 more lines, and the ratio metric cannot see correctness. Budget fill:
13.87x → 13.88x, unchanged.

- Index persistence through `vyrm-store`: a `projections` keyspace
  (Buffered durability — a projection is derivable, so a crash-lost write
  costs a rebuild, never truth) holding the index whole as one blob via
  `Index::to_bytes`/`from_bytes`. Persistence wiring lives in vyrm-graph
  tests and `examples/route_persisted.rs` with vyrm-store as a
  dev-dependency: the library stays substrate-free until vyrm-node exists
  to own the composition. `tests/persistence.rs` (3 tests): a reloaded
  index answers identically with zero re-extraction, an offline change is
  caught by refresh and survives grounding, absence is recovery not error.

**Measured outcome of persistence (2026-08-12, the 1,616-file repository):**
load+refresh 228 ms versus rebuild 1,675 ms — **7.3x** — with route parity
asserted, not assumed, across all six reference queries; projection blob
4.46 MB; no-op refresh after load 26 ms. Defect found and fixed by the
parity assertion: persisting the centrality map broke parity by one ULP,
because serde_json's default float parse is a fast path that can land one
ULP off the written value. The fix is structural, not the `float_roundtrip`
feature flag: derived state is `#[serde(skip)]` and recomputed on load by
the same deterministic code, so a projection of a projection cannot drift
from its source.

**Step R is complete.** The routing projection attunes, extracts with real
parsers, ranks with a measured design, routes filename-level entities, and
persists across processes. Successor work belongs to later steps: recall
integration (Step 4), projections over claims (Step 5), and the vyrm-node
composition layer that will own persistence wiring in the product.

### Step T · Enableable developer traces — COMPLETE

Observability for the operator and for the effectiveness ledger, off by
default and free when off. Design decision, recorded before implementation:

- The `tracing` crate with `EnvFilter` (`VYRM_TRACE=vyrm_graph=debug,...`),
  the 2026 ecosystem standard: structured spans, zero-cost when no
  subscriber is installed, JSON output available for machine consumption.
- Spans at subsystem boundaries only — `append_batch`, `refresh`, `ground`,
  `route`, projection save/load — carrying the counts the reports already
  compute, not per-file noise.
- Instrumentation overhead is measured as its own change against the Step R
  baselines before it merges; an unmeasured "zero-cost" claim is still a
  claim.

**Landed (2026-08-14):** `tracing` in vyrm-store (`append_batch`,
`record_invocation`, `rebuild_current`, `ground_current` — divergence is a
`warn`), vyrm-graph (`refresh`, `ground`, `route`, `from_bytes`, each event
carrying the counts its report computes), and vyrm-node (`hook.handle` with
the event name, `preflight` with stacks/claims/tokens). The kernel is
untouched: `cargo tree -p vyrm-core` still shows serde alone. The
subscriber installs only in `vyrm-cli` `main`, only when `VYRM_TRACE` is
set, and writes to **stderr always** — stdout is the answer channel (hook
injection, JSON gate decisions) and a binary test asserts diagnostics never
touch it. `VYRM_TRACE_FORMAT=json` emits machine-readable lines (parsed
back as JSON in the test) — the feed the Step V observatory's Rerun stage
will consume. A blanket `VYRM_TRACE=debug` also surfaces Fjall's own spans:
substrate visibility for free.

**Measured overhead (2026-08-14, controlled):** the naive comparison was a
trap and is recorded as such — the morning's 12.2 ms hook baseline had
grown to 17.7 ms by evening *on the same store*, which is store growth (a
day of journaled invocations), not instrumentation; measuring the new
binary against the morning number would have blamed tracing for it. The
controlled A/B (git-stashed pre-tracing binary vs tracing-compiled binary,
identical fresh 40-claim stores, 30 runs each): **12.6 ms vs 12.8 ms** —
+0.2 ms (~1.6%), within run noise, for tracing compiled in but off.
Enabled (`VYRM_TRACE=debug`, stderr fmt subscriber): **+1.3 ms** on the
session-start path. Step R comparators with tracing compiled: projection
load 205–224 ms against a 205–206 ms baseline, refresh 26–31 ms against
26–27 ms — run-to-run spread, no attributable regression. The off-path
claim ("free when off") is now a measurement: 0.2 ms bounds it.
Workspace at 134 tests, clippy clean.

### Step V · Estate topology and the observatory — DESIGNED, NOT STARTED

Operator decision recorded 2026-08-12: **two surfaces, one kernel.** Shippin
is the customer surface (a customer attunes and owns their Clyffy instance;
access into their projects is explicitly scoped); vyrm is the estate — the
operator's full-control memory system. The kernel is never forked: both
surfaces compose the same crates through vyrm-gate/vyrm-node, because a
forked memory layer would demand shadow-parity verification forever.

Isolation lands on seams that already exist: one Fjall database directory
per project/tenant (a vyrm store is a directory — process-level isolation is
free), keyspace-per-scope within a store (adopted at blueprint triage), and
estate-level claims that point at isolated projects without containing them.
Gates decide what crosses.

The observatory (UI) comes later and in two stages, so motion is visible
before any bespoke UI exists:

1. **Traces into Rerun** — Step T's spans logged through the Rerun Rust SDK
   (time-aware ECS, column-chunk storage, built for streaming temporal
   data). Refresh, routing, grounding, and claim flow become scrubbable on a
   timeline in an existing scientific viewer at near-zero UI cost.
2. **Bespoke estate map** — GPU force-directed graph rendering
   (Cosmograph-class WebGPU/WebGL, millions of nodes) over the routing
   projection and claim graph, with transaction-time scrubbing: vyrm is
   bi-temporal, so "watch the estate as it was" is a query the kernel
   already answers (`resolve_as_of`), not a feature the UI must invent.
   Precedent validating the model: Zep/Graphiti ships bi-temporal
   validity-interval knowledge graphs as agent memory at enterprise scale.

**Panel decision (operator, 2026-08-12):** the operator panel is built once,
at a protocol boundary, and becomes Shippin's master panel by embedding —
neither a separate tool forever nor folded into a customer product that does
not exist yet. The panel owns no logic: every capability is first a `vyrmd`
endpoint, which is first a CLI-provable operation, so every pixel displays
something already measured. Reference model: **Anytype** — local-first with
the primary copy on-device, typed objects connected by relations forming a
navigable graph, a graph view native to the daily tool rather than bolted
on, and spaces as the shareable isolation boundary. The mapping is direct:
claims and subjects are the typed objects and relations, a store-per-project
directory is the space, gates are the sharing boundary, and the observatory
is the graph view living inside the panel. Anytype's encrypted self-hosted
sync (any-sync) is the recorded precedent for estate-to-tenant
synchronization when the master/tenant tiers land — a design to study at
that step, not to adopt unexamined. Build order: recall ledger (Step 4) →
traces (Step T) → vyrmd protocol → panel shell (Rust-native) → Shippin
embedding.

### Step P · Preflight and the runtime experience — COMPLETE (v1)

**The gap this step closes (operator, 2026-08-13):** everything landed so
far is a library with a CLI — the agent must *know* to call `vyrm recall`,
*remember* to journal, *volunteer* to wait. The product claim is the
inverse: the agent lands in a repository and the memory layer is already in
the loop. Recall arrives before reasoning starts; journaling happens as a
side effect of work; gates are enforced by the harness, not by the model's
discipline. A memory system the model has to remember to use is not a
memory system.

**Research adopted (2026-08-13, cited in this entry):**

- **Claude Code hooks are the seam, and it is deterministic.** The 2026
  hook lifecycle covers every moment vyrm needs: `SessionStart` (matchers
  `startup`/`resume`/`compact` — context re-injected *after compaction*),
  `UserPromptSubmit` (stdout injected into model context before reasoning,
  30 s budget), `PreToolUse` (exit 2 / `permissionDecision: deny` blocks a
  tool call, `if` conditions like `Bash(cargo *)` narrow per command),
  `PostToolUse` (react to edits), `Stop` (turn ended), `PreCompact`
  (snapshot before summarization). The field's stated decision framework:
  *if it must be enforced, use hooks* — prompts hope, hooks guarantee.
  Direct prior art: claude-mem builds session memory entirely on this
  architecture.
- **The 2026 agent-memory field converged on two lessons vyrm already
  embodies plus one it must adopt.** (1) Letta: the *runtime* places memory
  into context deterministically — do not hope the model reads a file.
  (2) Zep/Graphiti: temporal validity intervals distinguishing current from
  superseded facts — vyrm's bi-temporal kernel is this, stronger. (3) New:
  **async memory writes are the default** — a write that blocks the
  response pipeline is latency the user feels. vyrm's durability classes
  map exactly: recall (the read) is the only synchronous path; journaling
  rides Buffered and never blocks the turn.
- **Portability:** AGENTS.md is the vendor-neutral discovery convention
  (Codex, Cursor, Copilot, Gemini CLI, Zed, et al.); MCP is the
  vendor-neutral tool surface, with rmcp 3.x as the official Rust SDK
  (stdio transport; all logging to stderr — stdout belongs to the
  protocol). Hooks are the Claude Code fast path; MCP via `vyrmd` is the
  same operations for every other harness.

**Design — one dispatch binary, harness wiring as data:**

- `vyrm preflight` — the moment of attunement. Detects the stack from
  marker files (`Cargo.toml` → cargo profile; `bun.lock`/`package.json` →
  bun profile; extensible), opens or creates the store-per-project
  directory, loads the persisted projection and refreshes it (228 ms load +
  26 ms unchanged-tree check, both measured in Step R — inside the < 1 s
  hook latency guidance), then emits a budgeted recall of the project's
  current claims as injected context. Wired to `SessionStart`; the
  `compact` matcher makes memory survive compaction *mechanically* — the
  claim this system was pitched on, enforced by the harness.
- `vyrm hook <event>` — single entrypoint reading the harness JSON on
  stdin, dispatching by event. Mapping: `UserPromptSubmit` → subject
  extraction + budgeted recall injection; `PostToolUse (Edit|Write)` →
  freshness signal + observed claims; `PostToolUse (Bash, if cargo
  test/bun test …)` → the *application journal*: test and build outcomes
  recorded as claims with validity intervals — a failure is a claim that
  stays in force until the run that retires it; `Stop` → invocation
  journal for the turn; `PreCompact` → snapshot claims for anything only
  present in conversation state; `PreToolUse` → the wait gate:
  `route_fresh` semantics as `permissionDecision` — refresh-and-allow when
  cheap, deny-with-reason when the projection is quarantined. "The AI
  knows to wait" stops being prose and becomes an exit code.
- `vyrm init --harness claude-code` — writes the hook wiring
  (`.claude/settings.json`) and the AGENTS.md block. Turnkey means the
  preflight installs itself; profiles are TOML data, never code forks.
- Stack profiles own: build/test/run commands, the extractor set that
  matters, and the journaling rules for application runs. `bun:` and
  `cargo:` are the first two because they are the operator's stacks.
- `vyrmd` (per Step V) grows an MCP server face (rmcp, stdio) exposing
  recall/observe/route/ledger to non-Claude harnesses. The hook path and
  the MCP path call the same operations; neither owns logic.

**Acceptance (measured, two-sided):** end-to-end hook latency including
process spawn measured against the < 1 s guidance — if spawn + open + recall
blows the budget, that is a *result* and it forces the daemon forward in the
build order rather than being tuned away. Injection cost measured in tokens
per session against the Step 4 budget discipline. A scripted session
transcript proving: recall present before first reasoning; a stale-tree
route blocked and then unblocked by refresh; a failing `cargo test` run
producing a claim and the passing re-run retiring it. Outcome auto-judging
at `Stop` is **not** silently heuristic — a candidate judgement (e.g. the
agent edited a file a recalled claim named) is recorded as a candidate,
promotion to `accepted` is D-4. Registry acceptance: an adapter whose
verification claim has expired surfaces the warning at preflight (proven at
a fixed `as_of` past the interval — the kernel never reads a clock, so the
test doesn't either), and a harness with a closed interval refuses
`vyrm init` with the retirement stated.

**Harness registry and drift alarm (operator requirement, 2026-08-13):**
the integration layer is tailored per harness, and it audits itself,
because this space kills its own members: Gemini CLI — a harness any
registry written in spring 2026 would have named — was shut down mid-2026.
A hand-written adapter list without an expiry is wrong within a quarter.

- **Three orthogonal axes, never conflated.** *Harness* (the agent runtime:
  claude-code, codex-cli, opencode, grok-cli, kimi-cli, zcode/GLM …) ×
  *provider* (the model backend: anthropic, openai, xai/grok,
  moonshot/kimi, z.ai/glm …) × *billing mode* (subscription/rolling-quota
  vs per-usage tokens). The combos are real and current — Z.ai ships an
  Anthropic-compatible endpoint, so *Claude Code harness + GLM provider +
  $10/mo subscription* is one configuration and *Claude Code + Anthropic +
  Max plan* is another; Grok is per-usage API pricing ($3/$15 per Mtok).
  A registry that models "Claude Code" as one thing cannot support either
  cleanly.
- **The registry is data**: one TOML per harness adapter declaring its
  integration surface (hooks lifecycle? MCP client? MCP server? context
  file convention — CLAUDE.md / AGENTS.md / config.toml), which wiring
  `vyrm init --harness <name>` writes, and which capabilities degrade when
  absent (no hooks → MCP-only → recall is on-demand, not injected; the
  degradation is stated, not silent).
- **Verification is a bi-temporal claim with an expiry — the noise is
  `resolve_as_of`, not a scheduler.** Each adapter carries
  `harness/<name> integration_verified = <harness version, evidence>` with
  `valid_to = verified_at + 21 days`. Preflight resolves the claim as-of
  now on every session start: expired → a warning surfaces in the injected
  context and the panel ("codex-cli adapter unverified for 34 days —
  re-audit"). `vyrm harness audit <name>` re-verifies against the vendor's
  current release and writes the fresh claim; a harness that dies gets its
  interval closed and stays in the registry as history. Gemini is the
  registry's first bi-temporal fact: a row with a closed validity
  interval, recorded 2026-08-13.
- **Initial rows (verified 2026-08-13):** claude-code (hooks + MCP,
  first-class, the fast path); codex-cli (AGENTS.md + MCP client *and*
  server, `~/.codex/config.toml`); opencode (the 2026 open-source
  breakout, MCP + Anthropic protocol); grok-cli (per-usage only);
  kimi-cli (Kimi K3 coding plan); zcode/GLM (Anthropic-compatible
  endpoint, cheapest subscription backend); gemini-cli (**retired
  mid-2026**, closed interval). MCP went table-stakes across all
  survivors, which is why vyrmd's MCP face is the portability layer and
  hooks are the Claude Code optimization on top.
- **The ledger prices both billing modes.** `Effectiveness.provider`
  gains the billing mode, because the same measured 9.58x means two
  different things: under per-usage, tokens are dollars — the reduction is
  a *cost* figure; under subscription rolling quotas, tokens are headroom —
  the reduction is *turns before the rate limit*. Shippin's subscription
  and per-usage customer configurations both read their economics straight
  from the ledger instead of from marketing arithmetic.

**Build-order revision (ratified by operator "execute", 2026-08-14):** the
runtime experience jumps ahead of the panel. Step 5 (grounding over claims)
→ **Step P** → Step T traces (spans begin at hook dispatch, so the trace
layer lands where the runtime enters) → vyrmd protocol + MCP → panel shell
→ Shippin embedding.

**Landed (2026-08-14, `crates/vyrm-node/` + CLI surface):** vyrm-node is the
runtime layer — the composition Step V reserved the crate name for. Scope
decision, stated not silent: **v1 is the store-side loop** (claims memory,
registry, journaling, gate); the vyrm-graph routing composition (routing
preflight attunement, `route_fresh` as a gate) joins at the traces/vyrmd
step, its figures already measured standalone in Step R.

- **Preflight** (`vyrm preflight`, wired to session-start): detects the
  stack from markers (cargo; bun, whose lockfile outranks `package.json`;
  node), surfaces estate health (a quarantine warns here and gates below)
  and the adapter's drift alarm, then emits a budgeted recall of every
  claim in force, rendered for injection. Subjects come from the
  authoritative claims keyspace (`Store::subjects`), never the projection —
  a quarantined projection cannot silence recall.
- **Hook dispatch** (`vyrm hook <event>`): session-start → preflight
  injection; user-prompt-submit → whole-word subject match against the
  prompt, recall injected only when something matched, *nothing at all*
  otherwise (a stray newline is not an answer); pre-tool-use → the wait
  gate: a quarantined projection denies Edit/Write/NotebookEdit/Bash via
  `permissionDecision: deny` with the reason and the way out, reads pass —
  waiting applies to mutation, not to looking; post-tool-use (Bash) → the
  application journal: a run matching a stack profile's prefixes becomes a
  claim (`cargo-test status = failing (exit 101): …`) and the re-run
  supersedes it — retirement by supersession, exactly like every claim.
  Stop asserts nothing (D-4 stays open, not sneaked in); an unreported
  exit code journals as "outcome unreported by harness", stated not
  guessed. Unknown input shapes degrade to no-op, never to a session-
  breaking error.
- **Hook invocations record `Trigger::Event`** — the promotion the enum
  was built for ("a change of value rather than a change of schema"), on
  the operator's explicit directive, with the recording invariant intact:
  automation that cannot forget to record itself.
- **Registry + drift alarm**: seven embedded TOML rows (claude-code with
  hooks as the fast path; codex-cli; opencode; grok-cli per-usage-only;
  kimi-cli; zcode; gemini-cli as the closed interval). `vyrm harness
  audit` writes the 21-day verification claim; `vyrm harness status` reads
  every row's state; preflight surfaces expiry in the injected context.
  Proven at fixed instants: current at TTL−1 ms, expired at TTL (half-open,
  like every interval in this system), "unverified for 13 day(s)" in the
  injected context at day 34.
- **`vyrm init --harness <name>`**: writes the marker-delimited context
  block idempotently (second init replaces, never stacks), writes
  `.claude/settings.json` hook wiring for claude-code (SessionStart
  matcher `startup|resume|compact` — memory survives compaction
  mechanically) unless the file exists, in which case the wiring is
  printed for manual merge rather than clobbered. A retired harness
  refuses with the retirement stated. Hookless harnesses get their
  degradation stated in the report.

**Attunement census and stack-profile expansion (2026-08-14):** the
operator directed that the profile set be grounded in their actual estate,
so the priority list is a measurement, not a guess. Census of all 258
repositories under github.com/EonsofStupid (`gh repo list --json
name,primaryLanguage,isArchived,pushedAt`, roots and `package.json`
manifests fetched for the 115 repos active in 2026; aggregates recorded
here, raw listing kept out of the repo):

| Stack | Evidence | Priority signal |
|---|---|---|
| Rust / cargo | 52 repos, **50 active in 2026** | most active stack |
| React + Vite | 33 active repos by deps; `vite.config.*` in 51 roots | dominant frontend |
| bun | **39 lockfiles** vs 18 npm, 16 pnpm, 4 yarn | leading JS runtime |
| TanStack Start | 11 active repos (`@tanstack/react-start`) | the operator's meta-framework |
| Python | 16 repos, 13 active; `pyproject.toml` x8, `uv.lock` x4 | uv adoption underway |
| Vite w/o React | 9 active | tool layer stands alone |
| C++ / CMake | 5 repos, 4 active; `CMakeLists.txt` x3 | present |
| Go | 3 repos, all active | present |
| Next.js | 2 active | **no profile — stated omission** |

Toolchain vocabularies verified against the 2026 state before encoding:
Python consolidated on **uv + ruff** with `pyproject.toml` as single source
(`uv run pytest`, `uv sync`, `ruff check`); **TanStack Start is a Vite
plugin** since the vinxi migration, with Vite 8 stable 2026-03 — its runs
are `vitest`/`vite build`, which is why vite is a profile of its own
riding on bun or node; C++ standardized on CMake presets (`cmake --build`,
`cmake --workflow --preset`, `ctest`); Go remains the single `go` tool
plus `gotestsum`.

Landed in `stack.rs`: profiles gained multi-marker evidence lists
(python: pyproject/uv.lock/requirements/setup.py; cpp:
CMakeLists/meson.build; vite: vite.config.*/vitest.config.ts), the four
new stacks (python, go, cpp, vite), **word-boundary prefix matching**
(`make` must not claim `makepkg` — caught by test), and the **framework
facet**: `package.json` dependencies resolve to
tanstack-start / next / react-vite / react, with the meta-framework
outranking its parts, and corrupt manifests degrading to nothing rather
than erroring. Preflight now announces the full shape — verified live:
`stack=bun+vite+tanstack-start` on a TanStack-shaped project, with a
failing `vitest run` journaled as `vitest status = failing (exit 1)`.
Two-sided note: framework detection reads only the root `package.json`,
so monorepo workspaces attune as their root until the routing projection
joins the preflight (Step T/vyrmd); and the census counted primary
language + root manifests, not per-directory reality inside monorepos —
both recorded as limits of the measurement, not facts about the estate.

**Measured outcome (2026-08-14, release binary, ext4, 40-claim/12-subject
store, mean of 20 runs each, *end-to-end including process spawn, store
open, and the invocation record's fsync*):** session-start (preflight +
injection) **12.2 ms**; user-prompt-submit with matched recall **12.4 ms**;
pre-tool-use gate **12.7 ms**; post-tool-use journal **13.5 ms**. All ~75x
under the harness's 1 s hook guidance — the daemon is not forced by
latency at this store size, and the spawn-per-hook architecture holds for
v1. Injection cost for the 14-claims-in-force estate: ~245 estimated
tokens (1,682 bytes). Two-sided notes: each hook dispatch carries one
Authoritative fsync for its invocation record (~0.4 ms of the 12), which
is the recording guarantee priced in; latency was measured at one store
size and rises with subjects (recall is per-subject seeks) — remeasure
before calling it flat; and the scripted-session acceptance ran through
the compiled binary (recall present before first reasoning, failing run
journaled and retired by the passing re-run, gate denied under quarantine
and reopened by reset, init idempotent and refusing the dead harness).
Workspace at 131 tests, clippy clean.

### Step S · The storage estate — port landed; tiers designed

**Operator directive (2026-08-14):** the ability to fold in storage —
bbolt for the Go portion (LFG: shippin is the canonical Rust authority,
GoClyffy the shadow-parity implementation), Moka for the per-user
instance, Dragonfly as the enterprise shared tier — and a straight answer
on Fjall, not a repeated slogan.

**The Fjall position, answered with numbers and tripwires, not
handwaving.** What has been *measured* so far locates every win and every
bottleneck **above** the engine: the 0.431 ms that dominates a write is
the device's fsync — physics no engine rewrite recovers — and vyrm's
durability classes are the answer to it; the 9.58x token reduction came
from the claim model and recall; the 13.9x routing from the projection;
the one-seek-per-subject recall from the **key encoding** (vyrm's design,
which any engine executes). Today's footprint measurement says the
per-store cost is not a mismatch either: **~180 KiB on disk per store**
(the naive `du -sb` read 67 MB — Fjall's journal is *sparse-preallocated*
at 64 MiB apparent; apparent-vs-allocated is recorded here as a
measurement trap) and ~11 ms open — 258 stores ≈ 45 MiB, trivial. So §2's
rule stands *because* it keeps vyrm honest: the prior runtime's
"built-from-scratch AI database" was Fjall verbatim under branding, and
vyrm will not repeat that claim without the work behind it. **This is a
current position, not dogma.** The recorded tripwires that would justify
vyrm-native engine work, each a measurement: (1) per-instance block-cache
ceiling — Fjall reserves 32 MiB cache *capacity* per open store, and a
vyrmd holding hundreds of stores must measure actual residency and either
share or shrink it; (2) compaction pauses appearing in hook-latency tails
(p99, not means) as stores grow; (3) write amplification measured on
claim-sized values against the append-only, retirement-never-delete
workload — an LSM designed for general workloads pays for deletes vyrm
never issues; (4) a temporal-iteration pattern the key encoding cannot
serve in one seek. Any of these crossing from projected to measured is
the §2 mismatch, and **the port landed below makes the attempt safe**:
a vyrm-native engine would implement eight primitives and be proven by
the same differential and golden vectors as every other backend — an
experiment, not a rewrite.

**Landed (2026-08-14): the storage port.**

- **`vyrm_store::Engine`** — the fold-in seam. Eight required primitives
  (append_batch, sequence, claims_in_range, subjects, observe,
  projection get/put-with-durability, plus the `ClaimSource` reads);
  everything else is **provided by the trait**: assert, current-state
  projection, rebuild with its watermark atomicity, grounding with
  quarantine, reset. An engine implements the primitives and inherits the
  semantic layer — which is the whole argument in one type signature:
  vyrm is the layer, the engine is a port.
- **`MemoryEngine`** — the reference engine (MemoryClaims + primitives).
  The differential (`tests/engine.rs`, standing rule 3): Fjall and the
  reference are indistinguishable through the port — same recall sets
  and content digests, same grounding stamp digest, same quarantine
  behaviour on an induced divergence.
- **vyrm-node is generic over the port** (`tests/port.rs`): preflight and
  prompt-recall run unchanged over `MemoryEngine`, proving the runtime
  consumes `Engine`, not Fjall.
- **Golden vectors** (`vyrm-core/fixtures/golden-vectors.json` +
  `tests/golden.rs`): key encodings incl. the inverted-timestamp
  newest-first ordering, prefixes and exclusive ends, canonical claim
  JSON, and the recall digest — regenerated from the kernel on every
  test run, so a wire-format change cannot land silently. **This file is
  the Go/bbolt engine's conformance target**: the GoClyffy implementation
  reproduces these bytes, implements the eight primitives over bbolt
  (etcd-io fork, actively maintained, serializable ACID transactions,
  single-writer MVCC — the same write discipline Fjall's
  single-writer-tx gives the Rust side), and inherits parity.

**Tiers, designed and recorded (build with vyrmd/Shippin, not before):**
- **T0 · embedded durable (system of record):** Fjall (Rust), bbolt (Go).
  One directory per project; the port above.
- **T1 · per-instance hot tier:** Moka (in-process, does one job well) in
  **vyrmd** — recall sets keyed by (store, query digest, as_of), 
  invalidated by sequence watermark; projections cached across sessions.
  Meaningless in one-shot CLI processes, which is why it waits for the
  daemon; its acceptance is a measured hit-rate and latency delta, or it
  comes back out.
- **T2 · shared enterprise tier:** Dragonfly (BSL 1.1, Redis-protocol,
  vertically scaled) for the Shippin estate: cross-instance recall
  cache, session presence, estate pub/sub. Cache and coordination
  **only** — claims never live in T2; the system of record stays T0 with
  its fsync, and a cache tier that could disagree with the log would be
  the divergence §8.3 exists to catch.

Two-sided notes: the trait fixes `Error = vyrm_store::Error`, which is
pragmatic and slightly wrong for a pure port (a Go engine obviously
doesn't share the type; the *contract* is the differential + vectors, the
Rust trait is one binding of it); invocation recording and gc are not yet
port methods (CLI main still binds to `Store` concretely) — folded in
when vyrmd needs a second engine end-to-end. Workspace at 137 tests,
clippy clean.

### Step 4 · Recall and the effectiveness ledger — COMPLETE

Retires F3, the central risk. Recall v1 resolves current claims for a subject
set and returns a recall set with a token estimate — semantic content with
provenance, never a rendered prompt string (§10).

The ledger records `tokens_emitted` against `baseline_tokens` from a controlled
comparison with unstructured context on the same query (§13.1).

- **Acceptance:** a reproducible A/B over a fixed corpus and query set, reporting
  both token counts and the outcome distribution. **A measured reduction is the
  deliverable. A reduction that does not appear is equally a result and is
  recorded as such.**

**Landed (2026-08-12):**

- `vyrm_core::recall` over a new `ClaimSource::subject_versions` port method
  (one seek per subject via `key::subject_prefix`, never a store scan).
  Deterministic budget fill, first claim always included, truncation visible;
  content digest per §13.2. Six kernel tests; adapter conformance proven by a
  recall differential against `MemoryClaims` across instants and budgets
  (standing rule 3).
- The §13.1 ledger extends the invocation record (one log, as
  `invocation.rs` already prescribed): `Effectiveness` fields on recall
  invocations, `Store::set_recall_outcome` to judge after the fact —
  refusing to judge a non-recall, since that would poison the evidence base.
- Operator surface: `vyrm recall` (recorded with its effectiveness fields),
  `vyrm outcome`, `vyrm ledger` (records plus outcome distribution). Driven
  end-to-end through the compiled binary in tests.

**Measured outcome of the A/B (2026-08-12, `examples/recall_ab.rs`,
fixtures checked in):** corpus = 32 claims extracted from the frozen
2026-08-12 PLAN.md snapshot; baseline arm = every journal section mentioning
a queried subject, stacked whole (mechanical md-stacking); both arms counted
by o200k_base (a proxy tokenizer — no Claude tokenizer is public; the ratio
cancels it). Across six queries: **12,009 baseline tokens vs 1,254 recall
tokens — 9.58x** — range 2.18x (panel/observatory: young subjects, few
matching sections) to 20.13x (persistence), no truncation at the 1,500-token
budget. Six ledger records carry controlled baselines; the outcome
distribution is all-`unknown` until recalls are judged in use, and is
reported as such. Two-sided notes: the kernel's four-bytes-per-token
estimate runs **20.3% under** the real count on this corpus — recorded, not
retuned, because calibrating a constant to the single corpus that measured
it would be overfitting; a second corpus decides. And the baseline arm's
sections overlap heavily across queries (three queries each matched 2,456
tokens), which is faithful to grep-stacking rather than a flattering
construction.

### Step 5 · Projections, rebuild, grounding — COMPLETE

§8.2 and §8.3 on the Step 1 index. Grounding halts on divergence rather than
repairing.

- **Acceptance:** an induced divergence halts and quarantines; a matching
  projection emits `grounded` with a digest; a crash mid-rebuild replays the
  interval rather than skipping it.

**Landed (2026-08-14, `vyrm-store/src/projection.rs`):** the current-state
projection — newest version per (subject, predicate) — as the first §8.2/§8.3
projection over the claim log. The watermark lives in the same serialized
blob as the entries, so §8.2's atomicity requirement holds by construction:
there is no state in which the watermark moved and the entries did not, and
a crash mid-rebuild replays the interval (proven by a test that folds the
interval, "crashes" before the write, and grounds the replayed result
against recomputation rather than against a hand-written expectation).
Grounding recomputes at the projection's **own watermark**, not the current
sequence — it verifies incremental-equals-batch over the same interval,
while staleness beyond the watermark belongs to rebuild; conflating them
would report honest lag as divergence. The operator `vyrm ground` reaches
§8.3's `as_of = now` by rebuilding first. Divergence quarantines with the
one derived-state write that pays for an fsync, because a quarantine a
crash could forget would un-halt a diverged projection silently — proven by
reopening the store with no flush and finding the quarantine held. The only
exit is the explicit `vyrm reset-projection`; rebuild, read, and re-ground
all refuse while quarantined. Operator surface: `vyrm rebuild`, `vyrm
ground`, `vyrm reset-projection`, all invocation-recorded and driven
end-to-end through the compiled binary in tests.

**Measured outcome (2026-08-14, `examples/ground_cost.rs`, ext4):** rebuild
and grounding are linear as §8.3 states — 10,000 claims: rebuild 32 ms,
ground 31 ms; 100,000 claims: rebuild 447 ms, ground 440 ms (≈4.4 µs per
claim, both O(claims)). The incremental case the sequence index exists for:
one claim appended on top of the 100k log rebuilds in **0.80 ms**,
independent of log size. The figures justify §8.3's SHOULD — grounding on a
longer interval than rebuild — with a ratio: at 100k claims a ground costs
~550x an incremental rebuild. Workspace at 123 tests, clippy clean.

### Step 6 · Differentials and change sets

§8.4 as one primitive, consumed unsigned by the analytical path and signed by a
gate.

- **Acceptance:** a differential is content-addressed and reproducible; applying
  it to the `from` watermark reproduces the `to` state exactly.

### Step 7 · Gates and promotion

**Blocked on a decision, not on code.** §9.1 records the predicate set and its
thresholds as unsettled. The evaluation harness, deny-by-default behaviour, and
all-failures-returned reporting can be built against a placeholder policy; the
policy itself cannot be invented.

### Step 8 · napi adapter and Clyffy

§4.1 and §14, once the core and store APIs are stable. Deferred deliberately:
binding an API that is still changing would fix its shape prematurely.

## 5 · Open decisions

| Ref | Decision | Blocks | Owner |
|-----|----------|--------|-------|
| D-1 | Gate predicate set and thresholds (§9.1) | Step 7 | Operator |
| D-2 | Tier naming: `local`/`primary`/`tenant` versus alternatives (§9) | Cosmetic; earlier is cheaper | Operator |
| D-3 | `events` keyspace: sequence index or removal (F1) | — | **Closed:** became `sequence_index` in Step 1 |
| D-4 | Recall outcome auto-judging policy: which runtime evidence (edited a recalled file, reran a recalled command) may promote `unknown` → `accepted` without an operator, if any | Step P effectiveness loop | Operator |

## 6 · Standing acceptance rules

These apply to every step and are not restated per step.

1. A property asserted in an API response MUST have a test that verifies it
   (§12). The prior runtime asserted `sync_all` on every write with nothing
   checking it.
2. A throughput or latency figure MUST be a recorded measurement, on a real block
   device. `/tmp` is tmpfs on this host, where `SyncAll` never reaches a disk.
3. An adapter is correct if and only if it agrees with the grounding reference
   (§8.3). Conformance tests are differentials against `MemoryClaims`, never
   independently written expectations.
4. `vyrm-core` MUST NOT acquire a substrate or transport dependency. Verified by
   `cargo tree` (§5).
5. Terminology follows §1.2. The banned-synonym column is enforceable by grep and
   SHOULD be checked before each commit.
