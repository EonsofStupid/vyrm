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
- atomic typed runtime commits spanning claims, graph records, relations, and
  lifecycle events;
- one hash-chained global runtime cursor with resumable, scope-filtered replay;
- optimistic concurrency that rejects stale writers instead of losing updates;
- hash-chained, typed reasoning runs;
- one-attempt/one-observation mutation authorization;
- freshness barriers and deny-by-default policy differentials;
- parser-backed routing to complete source files;
- rebuildable projections that quarantine on divergence;
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

Connectome provides seven lenses:

| Lens | Purpose |
|---|---|
| Prompt flights | Launch, replay, freeze, inspect, and compare prompt experiments |
| Overview | Runtime health, freshness, grounding, and active work |
| Graph | Selection-centered claim, evidence, run, flight, and source topology |
| Runs | Typed reasoning timelines |
| Claims | Current bi-temporal state and provenance |
| Routes | Ranked complete-file source routes with justification |
| Activity | Lifecycle and operator invocation evidence |

The local API also exposes the authoritative persistence layer:

| Endpoint | Purpose |
|---|---|
| `GET /api/changes?after=N&limit=N` | Resume the verified runtime changefeed |
| `GET /api/runtime/graph?valid_at=T&cursor=N` | Freeze the typed graph at valid time and transaction cursor |
| `GET /api/runtime/diff?from=A&to=B&valid_at=T` | Inspect exact structural change between cursors |
| `POST /api/demos/prompt-strength` | Persist a deterministic weak/strong trace pair for temporal playback |

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

The **Load weak ↔ strong demo** control creates an explicitly synthetic pair
through the real runtime recorder. It contrasts an unbounded “Make this better”
request with a constrained, measurable prompt. Both traces are durable graph
events: freeze any burst, scrub or rewind the timeline, inspect typed payloads,
and compare context, tool fanout, latency, and acceptance side by side. The
demonstration validates the instrument; it is not benchmark evidence.

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
| `vyrm-core` | Claim, reasoning, typed runtime graph, traversal, and differential contracts; serde-only boundary |
| `vyrm-store` | Storage port plus transitional Fjall adapter; atomic runtime commits, hash-chained changefeed, durability classes, sequences, projections |
| `vyrm-graph` | Parsing, incremental freshness, grounding, source routing |
| `vyrm-node` | Runtime lifecycle, instance binding, policy, append-only reasoning composition |
| `vyrm-cli` | Operator surface |
| `vyrmd` | stdio MCP surface for hookless runtimes |
| `vyrm-eval` | Paired frontier-runtime evaluation evidence |
| `connectome-ui` | Prompt-flight recorder and runtime workbench |

Vyrm owns the storage contract and is moving toward a Vyrm-native engine. Fjall
is the current transitional compatibility adapter behind
`vyrm_store::Engine`, not the permanent architecture. The port and in-memory
reference differential preserve semantics while the native engine replaces it;
the replacement must retain correctness and demonstrate equal or better
throughput, latency, durability, recovery, and memory behavior.

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
