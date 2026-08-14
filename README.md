# vyrm

An AI-native memory system: bi-temporal persistence and recall for coding
agents, built as a semantic layer over a proven log-structured merge-tree
substrate.

vyrm is the umbrella. Every optimization in this repository — parser-grade
extraction, ranked routing, budget fill, incremental freshness, projection
persistence — is a subsystem of one product with one contract: an agent
deployed into a repository attunes without configuration, recalls with fewer
tokens than it would spend searching, and is told to wait whenever the answer
would otherwise be stale.

## What vyrm is, precisely

vyrm is **not a storage engine**. The substrate is [Fjall] 3.1.8 — keyspaces,
a single database-level journal, manual journal persistence — credited by
name and wrapped, not rewritten. This project's own audit of a
"built-from-scratch AI database" found Fjall verbatim under the branding
(`docs/blueprint-triage.md`); vyrm declines that claim by design. `SPEC.md`
§2 forbids engine work without a measured mismatch, and none has been
measured.

The engine is a **port**, not a marriage: `vyrm_store::Engine` is eight
primitives; assert, projections, rebuild, grounding, and quarantine are
provided by the trait, so a backend implements the primitives and inherits
the semantics. Two engines ship (Fjall, and the in-memory reference proven
indistinguishable by differential); `vyrm-core/fixtures/golden-vectors.json`
is the byte-level contract a parity engine in any language tests against —
the Go/bbolt engine first. Engine replacement is gated on measured
mismatches, which are recorded as tripwires in `PLAN.md` Step S — and the
port is what makes that experiment safe to attempt when one trips.

What the substrate cannot provide is what vyrm is:

- **Bi-temporal claims.** Valid time and transaction time, half-open
  intervals; retirement, never deletion. The kernel is `vyrm-core` and
  depends on nothing but serde.
- **Durability classes.** An authoritative write carries exactly one fsync;
  telemetry and derived state are buffered, because durability costs 0.431 ms
  of a 0.562 ms write and truth is the only thing worth charging it to.
- **The routing projection.** Attune to a repository, index it with real
  parsers, answer a query with a ranked list of files to read **in full** —
  fragments are how agents drift; whole files are how they don't.
- **Grounding.** Every incremental structure can be rebuilt from scratch and
  differenced against itself. Divergence quarantines the projection and
  says so; it is never silently repaired.
- **Wait gates.** `route_fresh` refreshes before answering — the barrier an
  agent crosses when the answer must reflect the tree as it is now. Freshness
  is a spoken contract, not an assumption.

## Subsystems

| Crate | Subsystem | Depends on |
|---|---|---|
| `vyrm-core` | Claims kernel: bi-temporal model, keys, validation | serde only |
| `vyrm-store` | Substrate adapter: Fjall, durability classes, sequence index, projections keyspace | vyrm-core |
| `vyrm-graph` | Routing projection: attunement, tree-sitter extraction, ranking, freshness, grounding, persistence | vyrm-core |
| `vyrm-node` | Runtime experience: preflight, harness hook dispatch, stack profiles, harness registry with drift alarm | vyrm-core, vyrm-store |
| `vyrm-cli` | Operator surface | the above |

Future members, added as each layer lands: `vyrm-gate` (promotion gates),
`vyrmd` (daemon and MCP face for hookless harnesses). Dependency arrows
point inward only.

## The runtime experience

A memory system the model has to remember to use is not a memory system.
`vyrm init --harness claude-code` wires the harness's hook lifecycle so the
loop closes without the model's discipline: session start (and every
post-compaction restart) injects a budgeted recall of everything currently
held true; a prompt naming known subjects gets their claims injected before
reasoning; a `cargo test`/`bun test` run's outcome becomes a claim that the
next run supersedes; and a quarantined projection *denies mutating tool
calls* with the reason and the way out — the wait is an enforced decision,
not advice. Harness, provider, and billing mode are separate axes in an
embedded registry whose adapters carry 21-day verification claims:
verification that lapses makes noise in the injected context itself,
because this space retires its members mid-year.

## Measured, not asserted

Every figure below is reproduced by an example in this repository, on a
1,616-file reference repository, with the queries recorded in `PLAN.md`.
Negative results are recorded with the same weight as positive ones.

- **9.6× fewer tokens** than unstructured context in the controlled recall
  A/B — 12,009 tokens of stacked journal sections against 1,254 tokens of
  claims with provenance, same queries, same tokenizer
  (`vyrm-cli/examples/recall_ab.rs`, fixtures checked in).
- **13.9× fewer lines to read** than a text scan under the 1,000-line budget
  fill, with the declaration site ranked first on every query that has one
  (`examples/route_vs_scan.rs`).
- **7.3× faster process start** from the persisted projection than from
  rebuild — 228 ms load-and-refresh against 1,675 ms parse — with route
  parity between built and loaded asserted, not assumed
  (`examples/route_persisted.rs`).
- **26 ms** to confirm an unchanged tree; refresh cost is proportional to
  what changed, not to repository size.
- **12–14 ms per hook dispatch, end-to-end** — process spawn, store open,
  recall, and the invocation record's fsync included — against the
  harness's 1 s budget; the pre-tool-use gate answers in 12.7 ms
  (`PLAN.md` Step P, release binary, mean of 20).
- **4.4 µs per claim** to ground the current-state projection against its
  log, linear as specified; an incremental rebuild of one claim on a 100k
  log is 0.80 ms, independent of log size
  (`vyrm-store/examples/ground_cost.rs`).
- **Traces cost 0.2 ms when off** (`VYRM_TRACE` unset; controlled A/B of
  pre- and post-instrumentation binaries on identical stores) and ~1.3 ms
  enabled — "free when off" is a measurement, not a slogan (`PLAN.md`
  Step T). `VYRM_TRACE_FORMAT=json` feeds machines; stderr only, stdout
  stays the answer channel.
- Recorded negatives: PageRank centrality weighted into the ranking score
  cost lines on every ratio and improved nothing measurable, so it ships as
  a tie-breaker only; the tree-sitter swap left the routing ratio unchanged
  while removing 748 phantom definers. Both live in `PLAN.md` Step R.

## Status

Pre-release. `SPEC.md` is the contract, including its §1.2 controlled
vocabulary — terms like *claim*, *projection*, *grounding*, *attunement* are
used here in their defined senses and no others. `PLAN.md` is the execution
journal: every landed step carries its measured outcome, and open operator
decisions are listed as decisions, not defaults.

[Fjall]: https://github.com/fjall-rs/fjall
