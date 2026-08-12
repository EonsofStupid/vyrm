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
| `vyrm-cli` | Operator surface | the above |

Future members, added as each layer lands: `vyrm-gate` (promotion gates),
`vyrm-node` (embedding adapter; will own the composition of graph and store
that tests and examples wire today), `vyrmd` (daemon). Dependency arrows
point inward only.

## Measured, not asserted

Every figure below is reproduced by an example in this repository, on a
1,616-file reference repository, with the queries recorded in `PLAN.md`.
Negative results are recorded with the same weight as positive ones.

- **13.9× fewer lines to read** than a text scan under the 1,000-line budget
  fill, with the declaration site ranked first on every query that has one
  (`examples/route_vs_scan.rs`).
- **7.3× faster process start** from the persisted projection than from
  rebuild — 228 ms load-and-refresh against 1,675 ms parse — with route
  parity between built and loaded asserted, not assumed
  (`examples/route_persisted.rs`).
- **26 ms** to confirm an unchanged tree; refresh cost is proportional to
  what changed, not to repository size.
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
