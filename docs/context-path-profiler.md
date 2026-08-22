# Context path profiler concept

Status: captured product boundary for a separate public repository after the
native Vyrm baseline is reproduced remotely. It may ship as an RRFlow module
and surface through Connectome, but it is not part of the Vyrm storage kernel.

## Product gap

Existing products already visualize model, tool, and retrieval spans:
[LangSmith](https://docs.langchain.com/oss/python/langchain/observability),
[Phoenix](https://arize.com/docs/phoenix/tracing/llm-traces), and
[Sourcegraph Cody](https://sourcegraph.com/docs/cody/core-concepts/context) are
useful references. Claude Code exposes context-window composition and
compaction through its
[`/context` tooling](https://code.claude.com/docs/en/context-window), while
[Cursor](https://docs.cursor.com/en/guides/working-with-context) documents its
rules, memories, and context sources.

The proposed tool is not another generic trace tree. Its differentiated job is
to explain, in plain language, how a prompt traversed a project's context
architecture and whether that path helped the verified outcome:

```text
prompt -> harness instructions -> routed .md files -> searches and reads
       -> injected/evicted context -> tool actions -> verification -> outcome
```

It should expose repeated searches, missed canonical documents, conflicting
instructions, stale routes, duplicate context, expensive hops, compaction loss,
and successful paths that ought to become the single source of truth.

## Canonical event contract

Every adapter emits provider-neutral, append-only events with a run ID, span
and parent IDs, monotonic ordinal, wall/monotonic time, actor, project identity,
content digest, provenance, token/byte estimates, latency, and policy labels.
Typed event families cover:

- prompt accepted and reasoning profile requested;
- instruction discovered, selected, rejected, or superseded;
- file/search/context read with route and relevance evidence;
- context injected, cached, evicted, compacted, or reconstructed;
- model, tool, and sub-agent attempt plus observation;
- decision, verification, regression, and final outcome;
- proposed documentation consolidation, approval, application, and rollback.

Raw provider events remain immutable. Vyrm stores the temporal log and derived
graph/time-series projections; it does not pretend a projection is canonical.
OpenTelemetry/OpenInference ingestion and native Claude, Codex, local-model,
Automaton, and later LFG adapters all lower into this contract.

## Connectome experience

The default view is a one-run-at-a-time animated path graph that a non-expert
can understand. It supports pause, freeze, step, rewind, fast-forward, and
micro-event expansion without changing canonical data. A weak-prompt and
strong-prompt demo use the same project and outcome rubric so their paths can
be overlaid rather than presented as decorative animation.

Operator views add:

- a synchronized event timeline, graph, context-window meter, and token/latency
  attribution;
- SSOT conflict and duplicate-document heat maps;
- prompt-to-file route coverage, repeated-search loops, and dead-end paths;
- before/after run comparison with verified outcome and regression evidence;
- protected, active, stale, redundant, and deletion-candidate context states;
- direct expansion from an aggregate edge into the exact immutable events.

The UI never invents hidden chain-of-thought. It visualizes observable harness,
context, tool, storage, timing, and verification events plus explicitly labeled
inferences.

## Evidence-gated pruning

The suggested default is a 90-day review cadence. A 50–80% reduction is a
configurable optimization hypothesis, not a deletion quota. Canonical
decisions, security policy, active routes, legal/audit retention, and material
needed by successful replays are protected by default.

Pruning follows a reversible transaction:

1. inventory and classify context with provenance and last-use evidence;
2. propose merges, rewrites, routing changes, archive moves, and removals;
3. replay representative weak/strong prompts against the candidate snapshot;
4. deny when success, latency, tokens, or policy behavior regress;
5. require configured human approval, commit atomically, and retain rollback;
6. grow from the consolidated SSOT rather than recreating parallel notes.

Clyffy can later enforce this lifecycle, while LFG compiles the selected
context into the just-in-time model input. Vyrm provides durable memory and
time travel; neither component is allowed to silently rewrite operator truth.

## Delivery gates

1. Freeze the event schema and ingest one Claude/Codex-compatible trace.
2. Prove deterministic replay and time travel over Vyrm.
3. Ship the weak/strong prompt comparison and path/SSOT diagnostics locally.
4. Validate metric attribution against raw traces and known documentation
   faults in multiple repositories.
5. Add evidence-gated consolidation with approval and rollback.
6. Integrate LFG/Automaton/Clyffy only after the standalone profiler is useful.
7. Publish after privacy, secret-redaction, export, and benchmark gates pass.

## Naming caution

The capability is worth building, but the RRFlow name has search and product
collisions: an existing [RR Flow](https://rrflow.com.br/) network platform and
multiple `rflow` AI/research brands already exist. A low-cost defensive domain
may be reasonable, but domain purchase should follow trademark and exact-name
checks rather than being treated as product validation.
