# Runtime tracing and operator-knowledge boundary

Status: contract, conflict-safe recorder, instance bootstrap, and lifecycle
slice implemented; wider execution coverage and the pgvector adapter remain
pending. Research and repository state were verified 2026-08-20.

## Why this is a kernel feature

Vyrm needs three observability layers, each with a different truth claim:

1. Rust `tracing` spans are low-overhead process diagnostics. They may vanish
   and are not product evidence.
2. Runtime trace events are bounded, typed, persisted, hash-chained micro-events
   that Connectome can freeze, replay, compare, and use for optimization.
3. Audit envelopes explain accepted or denied externally visible operations.
   They are compliance evidence, not a substitute for the detailed trace.

The second layer is now frozen in `vyrm-core`. `RuntimeTraceEvent` supplies
validated `start`, `annotation`, and `finish` methods with W3C-width trace/span
identities, parentage, domain, outcome, data class, bounded attributes, and
typed causal links. `into_runtime_event` maps it into the existing
`runtime_trace` event schema. It therefore receives the same global cursor,
scope, commit identity, mutation digest, hash chain, snapshot semantics, and
replay path as all other runtime truth. A three-engine differential proves the
serialized trace mutation is identical through memory, Fjall compatibility,
and native VyrmKV. A checked-in v1 JSON vector freezes the portable trace and
stored-runtime-event shapes.

`vyrm-node` now owns the conflict-safe persistence bridge. It reads the exact
scope stamp, installs or repairs the canonical trace schema in the same commit
as the first event, and retries only observed cursor conflicts. `vyrm init`
records a bounded `instance.init` annotation after wiring the checkout.
Lifecycle hooks—including the shared `vyrm_lifecycle` MCP path—commit a start
before dispatch and a separate finish afterward. Denials are explicit outcomes;
input is represented by digest and byte count rather than persisted raw content.
If the process dies between the commits, native reopen exposes the unmatched
start as an incomplete span.

Connectome's existing authoritative Temporal stream classifies these durable
events into its reasoning, workflow, routing, search, model, and storage lanes.
This is initial lane-level visibility, not yet the causal-tree, critical-path,
fan-out, or sampled-versus-complete interface described below.

This follows the useful part of the
[OpenTelemetry trace model and database semantic conventions](https://opentelemetry.io/docs/specs/semconv/db/database-spans/)
without making OpenTelemetry a kernel dependency. Export is an adapter; the
Vyrm causal contract remains stable if an exporter changes.

## Causal coordinates

A durable trace may link to these exact coordinates:

| Link | What it prevents |
|---|---|
| Reasoning run | Detached tool/model activity that cannot be tied to a goal and decision |
| Runtime cursor | A visual event that cannot be located in committed order |
| Read stamp | Query or policy results with unknown schema/catalog/hash state |
| Snapshot | Replays whose retained physical/logical state is ambiguous |
| Plan digest | Latency or correctness claims against an unknown execution plan |
| Projection stamp | Search results from an unknown or stale generation |
| Workflow manifest | Package events whose authorization/configuration changed silently |
| Provider invocation | Token/latency evidence that cannot be reconciled with a provider envelope |
| Operator knowledge | External pgvector/project results with no project or source revision |

Start and finish are separate immutable events. Incomplete spans therefore stay
visible after crash instead of being rewritten into apparent success.

## Required execution coverage

The contract is not yet a claim of complete tracing. Instrumentation is promoted
subsystem by subsystem and must carry the following minimum evidence:

| Boundary | Required durable fields |
|---|---|
| Lifecycle/preflight/gate | reasoning run, workflow manifest, read stamp, decision, denial code |
| `vyrmQL` parse/bind | query digest, contract version, referenced types, error class |
| `vyrmMX` plan | plan digest, alternatives considered/rejected, budget, selected access paths |
| VyrmKV read/write | cursor or read sequence, memtable/segment path, bytes/blocks, durability, cache deltas |
| Projection | source cursor, generation/config/artifact digests, lag, fallback/quarantine |
| Vector/embedding | model-space digest, filter selectivity, candidates, rerank count, recall mode, accelerator/fallback |
| Provider/tool | provider invocation, observable token/tool envelope, outcome; never hidden chain-of-thought |
| Cluster | shard/term/leader, snapshot or log coordinate, transport decision, retry class |
| External adapter | system, project, source revision, latency, result count, freshness/fallback decision |

High-volume physical events require sampling or aggregation policy. Logical
mutation and denial evidence must not be sampled. Credentials, authorization
headers, raw embeddings, and secret values never belong in trace attributes.
Content-class prompt/document/tool values require explicit instance retention
policy; control-class digests/counts/timings are the default.

## Per-project deployment

One major project receives one Vyrm instance. The instance manifest is the
authority for project ID, root, storage identity, adapters, trace retention,
embedding model spaces, and permitted operator-knowledge sources. An umbrella
instance still requires an explicit member on every trace and adapter request;
filesystem proximity is never membership.

The target operator methods are:

```text
vyrm init                    # materialize one instance contract
vyrm run                     # start the local daemon/workbench for that instance
vyrm trace status|tail       # inspect durable coverage, lag, drops, retention
vyrm adapter inspect         # show capabilities and exact external revision
vyrm adapter verify          # differential/freshness/tenant-isolation checks
vyrm query|explain           # one dynamic typed query path over the same planner
```

Only the existing `init`, runtime/workbench, and query/explain foundations are
implemented today. The trace methods above describe the next operator surface,
not shipped commands.

## pgvector is an operator-knowledge adapter

pgvector is useful when a project already keeps operator-authored documents,
code-derived knowledge, notes, incidents, or application rows in Postgres. It
does not replace Vyrm's authoritative reasoning, policy, cursor, audit, graph,
or native search state.

The adapter contract is:

1. Bind one explicit Vyrm scope to one Postgres database/schema/table or
   partition and immutable adapter-config digest.
2. Capture a source revision (for example a Postgres LSN plus catalog/index
   identity) before searching.
3. Bind every vector to exact embedding provenance/model space and every query
   to project/tenant filters.
4. Return result identities, distances, source revision, chosen exact/HNSW/
   IVFFlat path, scan controls, and observed latency—not unbounded row payloads.
5. Commit or reference an `OperatorKnowledge` trace link and projection stamp.
6. Deny, fall back, or label stale when the source revision/config/model binding
   no longer matches policy.
7. Synchronize Vyrm-originated work through an idempotent outbox. Never claim a
   single ACID transaction spans VyrmKV and Postgres.

The official [pgvector repository](https://github.com/pgvector/pgvector)
documents that exact search is the default; HNSW and IVFFlat trade recall for
speed; approximate filtering is applied after index scanning; iterative scans
can recover filtered result count within explicit tuple/probe/memory bounds;
and separate tables or partitions improve tenant isolation. Those choices must
be exposed as plan and trace evidence rather than hidden behind one “semantic
search” method.

## HelixDB comparison and inspiration

The current upstream audit was pinned to HelixDB commit
`475e805cd864be7ff81c09ee6ba9a18ccc4d918b`. Its public repository currently
shows a project-oriented CLI, a JSON dynamic-query AST and multi-language SDKs,
graph/vector/text access, built-in embedding and MCP surfaces, HNSW lifecycle
configuration, planner experiments/guardrails, and query/index telemetry. See
the [HelixDB repository](https://github.com/HelixDB/helix-db).

These are the parts Vyrm should compete with directly:

- one obvious `init → run → query → inspect` project experience;
- a single typed dynamic query AST shared by SDKs;
- planner quality budgets and recorded rejected alternatives;
- first-class graph/vector/text index lifecycle and readiness;
- embedded/local operation with a production service path;
- observable query and index behavior.

Vyrm's distinct hypothesis is not “another graph-vector wrapper.” It is that a
database dedicated to frontier-AI operation can make evidence freshness,
reasoning lifecycle, temporal truth, tool authorization, provider observation,
projection generation, and replay part of the same enforceable runtime. That
hypothesis still needs direct HelixDB benchmark fixtures; it is not a current
superiority claim.

## Promotion gates

1. Trace contract, conflict-safe recorder, schema repair, and three-engine
   persistence differential — complete.
2. Trace schema installed by every newly initialized project instance —
   complete. A dedicated operator migration command for untouched existing
   instances remains open; their first recorded trace repairs the schema
   atomically.
3. Lifecycle emits paired start/finish evidence, records denials, and preserves
   incomplete spans across native reopen — complete. Query, plan, storage,
   projection, vector, embedding, provider, and cluster emission remain open.
4. Connectome renders causal trace trees, critical path, fan-out, cache/IO/token
   mass, stale/fallback decisions, and sampled-versus-complete status. Basic
   durable-event lane classification is complete; the richer analysis is open.
5. OTLP/JSON export round-trips without changing Vyrm identity or leaking
   content outside policy.
6. pgvector adapter passes project-isolation, exact-oracle, filtered ANN,
   model-space, stale-revision, delete/update, restart, and outbox retry tests.
7. HelixDB/Vyrm fixtures publish correctness, task outcome, recall, throughput,
   p50/p95/p99, RSS/disk, startup/recovery, trace completeness, and raw trials.
