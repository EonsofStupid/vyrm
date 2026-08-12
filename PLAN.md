# vyrm — Work Plan

| Field | Value |
|-------|-------|
| Status | Draft. Grounding result re-run 2026-08-10 after Step 1. |
| Governs | Sequencing and acceptance of work against `SPEC.md` |
| Does not govern | Contracts, terminology, or requirements. Those are `SPEC.md` only. |

This document sequences work. It does not restate requirements. Where the two
disagree, `SPEC.md` is authoritative and this document is wrong.

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

Implemented and verified: 107 tests, clippy clean at `-D warnings`. Known
flake, observed once on 2026-08-11: `durability.rs::
an_unflushed_index_is_as_empty_as_the_claims_it_indexes` found 37 recovered
claims where it expects 0 — the test presumes an unflushed write is absent
after a clean reopen, but a clean close can land journal bytes via the page
cache, so the premise is not guaranteed. To be tightened when `vyrm-store` is
next touched (Step R item: index persistence).

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

### Step T · Enableable developer traces — DESIGNED, NOT STARTED

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

### Step 4 · Recall and the effectiveness ledger

Retires F3, the central risk. Recall v1 resolves current claims for a subject
set and returns a recall set with a token estimate — semantic content with
provenance, never a rendered prompt string (§10).

The ledger records `tokens_emitted` against `baseline_tokens` from a controlled
comparison with unstructured context on the same query (§13.1).

- **Acceptance:** a reproducible A/B over a fixed corpus and query set, reporting
  both token counts and the outcome distribution. **A measured reduction is the
  deliverable. A reduction that does not appear is equally a result and is
  recorded as such.**

### Step 5 · Projections, rebuild, grounding

§8.2 and §8.3 on the Step 1 index. Grounding halts on divergence rather than
repairing.

- **Acceptance:** an induced divergence halts and quarantines; a matching
  projection emits `grounded` with a digest; a crash mid-rebuild replays the
  interval rather than skipping it.

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
