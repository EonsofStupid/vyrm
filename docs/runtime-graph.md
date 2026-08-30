# Temporal runtime graph

## Boundary

The product architecture is:

```text
RRFlow
├─ RRO orchestration: Automaton → LFG
├─ RRD data runtime
│  └─ RRFlow native LSM persistence engine
└─ Connectome Panel: operator client and visualizer
```

This graph records operational causality. It does not perform BM25, embedding,
vector, or semantic retrieval and does not claim access to hidden model
chain-of-thought.

## Authoritative contract

`rrd-core::RuntimeCommit` is the atomic unit. A commit declares:

- a validated `ScopeId`;
- actor and caller-supplied time;
- the exact runtime cursor the writer observed;
- one or more schema, claim, record, relation, or lifecycle-event mutations.

The compatibility store compares `expected_cursor` inside one atomic write
transaction. A mismatch
returns `RuntimeConflict`; it never retries against newer state implicitly.
Every accepted mutation receives a contiguous global cursor and a digest linked
to the preceding runtime change. Claims embedded in the commit advance the
existing claim sequence in the same transaction.

Typed records and relations are immutable versions with half-open valid-time
windows. The enclosing runtime change supplies transaction order and
provenance. Relation endpoints and event subjects must exist in the same scope,
either from an earlier commit or as records in the same commit.

## Persisted unified catalogue and fail-closed writes

Every scope that writes typed records, relations, or events has an authoritative
`RuntimeSchemaRegistry`. Installing or advancing that registry is itself a
hash-chained runtime mutation, so schema and data can migrate in one atomic
commit. The first revision is `1`; every later migration must advance exactly
one revision. Concurrent or skipped revisions fail without advancing either
the runtime cursor or registry.

The same registry now names its namespace/database and owns one table map for
document, relational, graph-node/relation, key-value, vector, event,
time-series, geo, object, reasoning, and lifecycle identities. A tagged
`RuntimeLogicalModel` prevents one table kind from silently changing mutation
families. A tagged `RuntimeSchemaMode` makes strict and schemaless tables
distinct states: schemaless entries cannot also carry a strict specialized
schema, and strict record/relation/event entries must carry one.

Persisted pre-G03 registries remain readable. Their record/relation/event maps
derive strict catalogue entries without changing historical commit identity.
The first registry revision containing any explicit table switches that scope
to the unified contract: every non-claim mutation must resolve to a table whose
logical model matches the mutation family. Namespace/database coordinates and
explicit tables participate in commit identity and public contract round trips.

The registry governs allowed object types, required and optional property value
types, additional-property policy, event subject requirements, legal relation
endpoint combinations, temporally overlapping record uniqueness, relation-pair
uniqueness, and maximum incoming/outgoing cardinality. Governed writes deny by
default when the registry is absent or a mutation violates it. The production
store and `MemoryEngine` reference implementation execute the same validation
contract.

Reasoning and prompt-flight writers install or merge their strict types during
the next write to a legacy scope. This deliberately validates the migrating
commit and all future mutations without pretending old untyped history was
already governed. Claims remain protected by their statically typed,
bi-temporal claim contract rather than duplicating that contract in this
registry.

The exact contract and migration boundary are in
[`rrd-unified-catalogue.md`](rrd-unified-catalogue.md).

## Unified CRUD, retirement, and read stamp

One `RuntimeCommit` can create or update document, key-value, record,
relation/native-edge, event, vector, time-series, geo, and object values. A
typed retirement mutation closes any of those identities at an explicit valid
time while retaining the append-only change history. Event retirement targets
the immutable event cursor; other families target their typed runtime
reference. Reasoning claims continue to use their dedicated bi-temporal
correction contract.

`RuntimeDataSnapshot` reconstructs every model family at one valid-time instant
and one authenticated cursor under one catalogue revision and read manifest.
The public `RrdEngine` data snapshot returns the same write vocabulary, which
makes create/read/update/retire and reopen comparison lossless, including named
vector collection addresses. Retirement validation and the final backend
cursor compare-and-swap make a mixed-model batch all-or-nothing.

## Replay and graph views

`Engine::runtime_changes_since(after, limit, scope)` returns a bounded page with:

- `requested_after` — the caller's cursor;
- `through_cursor` — the global position actually examined;
- `head_cursor` — the head observed by the read snapshot;
- matching verified changes.

Consumers always resume at `through_cursor`, including when a scope filter
matches nothing. This prevents a sparse scoped consumer from stalling behind
other scopes' traffic.

`RuntimeGraphSnapshot::from_changes` resolves record and relation versions by
both valid time and known transaction cursor. Lifecycle events become stable
event nodes keyed by their cursor; a subject-bearing event produces a
deterministic `emitted` edge. Snapshots support outgoing/incoming traversal,
bounded typed breadth-first traversal, and exact structural differentials.

Connectome exposes read-only development endpoints:

```text
GET /api/changes?after=0&limit=256
GET /api/runtime/schema
GET /api/runtime/retention
GET /api/runtime/query?ql=<percent-encoded-RRFlowQL>
GET /api/runtime/graph?valid_at=<millis>&cursor=<cursor>
GET /api/runtime/diff?from=<cursor>&to=<cursor>&valid_at=<millis>
```

## Migrated lifecycle state

Reasoning runs persist an updated typed `reasoning_run` record and immutable
`reasoning_event` for each validated transition. Prompt flights persist typed
`prompt_flight` revisions and immutable `prompt_flight_event` micro-events.
Their former v1 projection blobs are migration inputs only: if present without
typed events, the next mutation moves the complete history atomically.

## Deliberate next work

The following are not represented as complete:

1. A content-addressed large-object store for trace bodies, file revisions,
   prompt packets, and verification artifacts, with backreferences and explicit
   retirement policy.
2. Incremental grounded materialized graph lenses carrying source watermark,
   digest, and quarantine state; current graph-at-cursor reconstruction replays
   the bounded authoritative feed.
3. Estate-to-instance scope propagation and capability-based remote authorization.
4. Retention checkpoints and archival for high-volume event histories.

Fjall is the transitional compatibility adapter, not the destination. The
RRFlow-native engine now implements the same contracts, proven against both
Fjall and `MemoryEngine`. Fjall remains useful as a live compatibility path and
as the performance/correctness threshold the native engine must meet or beat.
