# Runtime tracing and operator-knowledge boundary

Status: contract, conflict-safe recorder, instance bootstrap, lifecycle, query,
native-storage-read, projection-publication, vector-search, embedding-commit,
provider/tool-envelope, causal-analysis, and data-class export slices
implemented. The portable operator boundary and first live pgvector functional
gate are implemented; production TLS/failure/scale promotion remains pending
alongside storage-write, cluster, OTLP, and retained-regression coverage.
Repository state was verified 2026-08-20.

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

Explicit query execution now uses the same consumable durable-span helper. The
operator captures an immutable scope `ReadStamp` before writing a root
`query.run` start, and `Catalog::capture_at` binds against that stamp after the
live head advances. `KNOWN HEAD` therefore means the head the caller observed,
not a head contaminated by trace events. Child `vyrmql.parse_bind`,
`vyrmmx.plan`, and `vyrmmx.execute` spans link to the root and the exact read;
plan/execution spans also link the physical-plan digest. Finishes record source
family/type, schema revision, selected and rejected paths, budgets, scan/row/
batch/byte counts, truncation, or a bounded error class and digest. Budget
refusal is a denial rather than an apparent execution failure.

The query execution span now contains a `vyrmkv.runtime_read` child. Every
engine reports complete logical scan/result evidence. Native VyrmKV also
captures cumulative manifest, memtable, segment, shared-cache, block-load, and
encoded/decoded-byte counters immediately around the logical execution and
persists only their bounded deltas. Memory and Fjall explicitly label that
evidence `logical_only`; they do not synthesize physical work.

The M5/M6 data plane uses the same contract. A vector search emits
`vector.search → vector.plan → vector.execute`, binds a sealed prepared plan to
the request digest and catalog revision, and revalidates it at execution.
Projection publication records generation plus source/config/artifact digests.
Freshness is derived from the vector-family projection cursor rather than the
global trace cursor, so observation cannot make an otherwise current index
appear stale. Embedding emits `embedding.run → embedding.infer →
embedding.commit`; the commit may rebase over canonical trace-only mutations,
but any intervening data or schema mutation denies it. The vector and its
provenance still publish through one read-bound data transaction.

The explicit surfaces are CLI `vyrm query` and MCP `vyrm_query`. Connectome's
GET Query Lab deliberately remains a read-only lens. Raw query text and
parameter values are returned to the caller but are represented in durable
trace and invocation state only by content digests, counts, and public plan
coordinates.

Connectome's authoritative Temporal stream classifies these durable events into
its reasoning, workflow, routing, search, model, and storage lanes. Its Causal
traces lens now groups persisted events by trace/span identity, validates
lifecycle agreement, exposes missing parents and cycles, distinguishes
complete/incomplete/summary/annotation/invalid spans, and retains each exact
cursor, change digest, and audit digest. The displayed critical candidate is
the longest measured root followed by the longest measured child at each
branch; nested durations are deliberately never summed. The default JSON export
includes only `control`; `operator` and `content` require explicit data-class
selection. This is not yet OTLP translation, sampling policy, or a retained
cross-run regression database.

When runners are explicitly armed, each prompt flight starts one durable
`provider.invoke` boundary. Observable provider envelopes annotate that span by
kind, ordinal, and digest; observable tool envelopes become zero-duration
causal children because their upstream streams do not consistently expose
paired tool timing. Prompt text, model output, commands, and hidden reasoning
do not enter the trace. A trace start/finish failure marks the flight failed
rather than presenting an unobserved success. Full read and projection link
serialization now retains schema/catalog/head and config/state fields, while
keeping the original read-cursor alias for v1 consumers.

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
vyrm query                   # dynamic typed query plus durable causal evidence
```

`init`, runtime/workbench, and the traced `query` surface are implemented today.
The `run`, `trace`, and adapter commands above remain target operator surfaces,
not shipped CLI commands. The underlying typed operator-knowledge Rust port is
now implemented in `vyrm-operator`.

## pgvector is an operator-knowledge adapter

pgvector is useful when a project already keeps operator-authored documents,
code-derived knowledge, notes, incidents, or application rows in Postgres. It
does not replace Vyrm's authoritative reasoning, policy, cursor, audit, graph,
or native search state.

The executable adapter contract is:

1. Bind one explicit Vyrm scope to one Postgres database/schema/table or
   partition and immutable adapter-config digest.
2. Execute the search in one external snapshot and bind its snapshot digest,
   database/relation/catalog identity, and optional stable project revision.
   WAL LSN is supporting evidence, not a substitute for snapshot visibility.
3. Bind every vector to exact embedding provenance/model space and every query
   to project/tenant filters. Seal the minimum required Vyrm source cursor;
   deny a stale projection or one newer than the query's captured read stamp.
4. Return result identities, distances, source revision, chosen exact/HNSW/
   IVFFlat path, scan controls, and observed latency—not unbounded row payloads.
5. Commit or reference an `OperatorKnowledge` trace link and projection stamp.
6. Deny, fall back, or label stale when the source revision/config/model binding
   no longer matches policy.
7. Synchronize Vyrm-originated work through a content-addressed idempotent
   outbox identity. A retry after an ambiguous finish returns the same external
   revision without applying the payload again. Never claim a single ACID
   transaction spans VyrmKV and Postgres.

`vyrm-operator` now freezes these portable shapes in checked-in JSON, provides
a deterministic exact-search adapter over Vyrm's vector oracle, validates both
sides of every adapter call—including the applied scan controls and projection
freshness—and declares vector-kind plus path-specific metric capabilities so an
unsupported cross-product fails closed. It builds quoted/parameterized pgvector
query shapes for exact, HNSW, and IVFFlat operation. `vyrm-node` adds paired
durable search/execute and sync/apply spans with `OperatorKnowledge` and projection
links. The reference writer proves idempotent replay. This is an executable
port and conformance oracle.

The opt-in `pgvector-postgres` feature now adds a real synchronous endpoint.
Its serializable control migration fixes source identity and project revision;
search captures `pg_current_snapshot()`, supporting WAL LSN, extension version,
relation OID, ordered column/index definitions, JSON `EXPLAIN`, stable revision,
and results inside one read-only repeatable-read transaction. Typed upsert and
delete payloads apply under a project advisory lock in one transaction with the
revision increment and persisted idempotency receipt. Connection secrets and
root certificates stay outside serialized deployment state. The production
constructor requires `sslmode=require` and certificate/hostname validation.

The CI endpoint uses a digest-pinned PostgreSQL 18/pgvector 0.8.6 service and
proves ordered exact/HNSW/IVFFlat parity, tenant/source/future-cursor exclusion,
stale-revision denial, update/delete, replay, and reconnect. It deliberately
uses a disposable non-TLS loopback client, so a certificate-backed endpoint
handshake, typed payload-expression filters, process restart, concurrent
serialization recovery, and retained performance evidence remain promotion
requirements.

The official [pgvector v0.8.6 repository](https://github.com/pgvector/pgvector/tree/v0.8.6)
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
   incomplete spans across native reopen — complete. Explicit query parse/bind,
   planning, and execution emit a parent/child tree linked to the pre-trace read
   stamp and plan digest, including error and budget-denial finishes — complete.
   Query storage reads, vector plan/execution, vector projection publication,
   and embedding inference/atomic commit now emit observer-safe causal spans —
   complete at the local M3–M6 boundary. Provider roots and observable
   model/tool-envelope coverage are complete for Connectome prompt flights.
   Vector artifact publication now emits the same projection span around an
   authoritative `vyrmDS` transition: staged exact/compact/HNSW bytes plus a
   typed catalog record and verified object reference commit atomically before
   serving changes, and restart reconstruction fails closed. Cluster emission,
   storage-write coverage outside this projection/embedding path, and
   cross-node catalog replication remain open.
4. Connectome renders causal trace trees, critical path, fan-out, cache/IO/token
   mass, stale/fallback decisions, and sampled-versus-complete status. Causal
   lifecycle reconstruction, integrity diagnostics, measured critical
   candidate, exact event drill-down, and responsive visualization are
   complete. Cross-run fan-out/cache/IO/token mass and explicit sampling
   completeness remain open.
5. JSON export preserves Vyrm identities and is deny-by-default for non-control
   data classes — complete. A formal OTLP translation and round-trip fixture
   remain open.
6. Portable operator contract, exact oracle, project/model/revision denial,
   safe SQL planning, traced execution, and outbox retry — complete. First live
   pgvector functional gate—repeatable-read/catalog capture, ordered exact/ANN,
   model/tenant/cursor isolation, stable revision, update/delete, reconnect, and
   retry—is complete and CI-enforced. Typed payload filters, process restart,
   concurrent failure recovery, authenticated endpoint TLS, and performance
   evidence remain open.
7. HelixDB/Vyrm fixtures publish correctness, task outcome, recall, throughput,
   p50/p95/p99, RSS/disk, startup/recovery, trace completeness, and raw trials.
