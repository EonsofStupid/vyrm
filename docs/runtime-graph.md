# Temporal runtime graph

## Boundary

The product architecture is:

```text
Automaton → LFG → Connectome
                      └─ Vyrm persistence/runtime substrate
```

This graph records operational causality. It does not perform BM25, embedding,
vector, or semantic retrieval and does not claim access to hidden model
chain-of-thought.

## Authoritative contract

`vyrm-core::RuntimeCommit` is the atomic unit. A commit declares:

- a validated `ScopeId`;
- actor and caller-supplied time;
- the exact runtime cursor the writer observed;
- one or more claim, record, relation, or lifecycle-event mutations.

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

1. A persisted strict schema registry for node types, edge endpoint types,
   property requirements, cardinality, uniqueness, and schema migrations.
2. A content-addressed large-object store for trace bodies, file revisions,
   prompt packets, and verification artifacts, with backreferences and explicit
   retirement policy.
3. Incremental grounded materialized graph lenses carrying source watermark,
   digest, and quarantine state; current graph-at-cursor reconstruction replays
   the bounded authoritative feed.
4. Umbrella-member scope propagation and capability-based remote authorization.
5. Retention checkpoints and archival for high-volume event histories.

Fjall is the transitional compatibility adapter, not the destination. The
Vyrm-native engine will replace it behind the same contracts, which are also
proven against `MemoryEngine`. The Fjall baselines remain useful only as
performance and correctness thresholds the native engine must meet or beat.
