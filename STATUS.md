# Runtime status — 2026-08-18

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
- A deterministic weak/strong prompt demonstration now persists two flight
  records and sixteen typed micro-events as one atomic runtime burst. The
  workbench renders event-density bursts, typed payload breakdowns, measurable
  cost differentials, event freezing, scrubbing, reverse playback, and up to
  8× fast-forward without presenting synthetic values as benchmark results.
- Prompt comparison is now a stable two-editor A/B lab instead of a polling-
  sensitive run selector. Prompt drafts survive live refresh, local contract
  signals react while typing, edited pairs execute through the real flight
  path, observed traces stay pinned together, and the guided seed is idempotent.

## Verification

CI runs the locked full workspace tests, warning-free clippy, evaluation-evidence
validation, and the `vyrm-core` serde-only dependency boundary. Compiled-binary
tests cover operator commands, hooks, explicit recovery, and both MCP eras.

## Deliberate limits

- The evaluation sample is a harness validation, not statistical significance:
  one trial per cell on synthetic repository fixtures.
- MCP cannot intercept another runtime's private tools. Hookless clients receive
  identical semantics through `vyrm_lifecycle` and must place that call around
  their mutations; server-owned operations remain directly enforceable.
- Record/relation endpoint integrity is enforced, but a persisted schema
  registry for allowed types, endpoint combinations, cardinality, and migration
  versions is still open. Until it lands, type names are validated but not
  centrally registered.
- Runtime scopes are present in every commit and feed query. Current reasoning
  and flight composition uses the physically isolated store's
  `instance:default` scope; umbrella member routing and capability-based remote
  authorization remain deliberately non-executable.
- Fjall remains wired only as the current compatibility substrate. The target
  is a Vyrm-native engine behind the existing `Engine` conformance contract;
  removal is gated by parity plus measured equal-or-better write/read latency,
  throughput, durability, crash recovery, and memory use—not by preserving
  Fjall as an architectural dependency.
- Evidence carries content digests, but general large artifact bytes and their
  revision/backreference lifecycle do not yet have a dedicated object store.
- Prompt-flight acceptance without a marker proves process completion only.
  Model-quality conclusions require non-trivial evaluators and repeated trials;
  the flight UI deliberately does not turn one attractive trace into a claim.

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
