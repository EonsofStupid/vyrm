# RRFlowQL transactions and joins v1

This document freezes the G03-W03 internal engine contract. It does not add a
second transaction coordinator or claim the later G06 network and SDK surfaces.

## Same-stamp equi-joins

The canonical read form is:

```text
FROM <source> JOIN <source> ON <left-field> = <right-field>
AT VALID <time> KNOWN <cursor|HEAD>
[WHERE <predicate>] PROJECT <fields> [LIMIT <rows>]
```

Binding resolves both sources against one captured catalogue and requires at
least one compatible value type for the two join fields. Output fields are
qualified as `left.<field>` and `right.<field>`. Null values never join.

Execution obtains one authenticated read stamp and loads one authoritative log
page through that stamp. Both source snapshots are reduced from that page at
the same valid-time/known-at coordinate before the deterministic equi-join is
evaluated. The join output is identity-ordered and feeds the existing
DataFusion filter, projection, and semantic-limit path. If the intermediate
join would exceed `ExecutionBudget.max_rows`, execution fails closed; it does
not return a partial relation. Accepted output is emitted in deterministic
`max_batch_rows` batches with ordinal and terminal `done` evidence.

Joins and the existing bounded graph traversal are separate logical operators.
This version deliberately excludes outer/cross joins, aliases, aggregates,
subqueries, and a cost-based join order. Those
features must preserve the same one-stamp and fail-closed budget invariants.

## Bounded mutation programs

The canonical program is:

```text
BEGIN;
MUTATE $first;
MUTATE $second;
COMMIT
```

`CANCEL` can replace `COMMIT`. A program contains at most 256 mutation
statements, has a 64 KiB text bound, rejects empty or duplicate bindings, and
canonicalizes to one stable representation. Each binding resolves to the
existing transport-neutral `TransactionMutation`, so schema, claim, record,
relation, event, vector, series, geo, object, and retirement operations keep
their existing validation and wire representation. The combined serialized
binding payload is bounded to 1 MiB.

`RrdEngine::execute_query_transaction` validates and binds the complete program
before it opens a transaction. It then delegates to the engine's existing
begin/commit/abort operations under the request context, scope, lease,
authorization, idempotency, read-stamp CAS, runtime hash chain, and accepted
audit authority. `COMMIT` publishes all bound mutations atomically; `CANCEL`
publishes none. A validation or mutation failure publishes none. Exact retry
returns the same transaction identity and receipt; a changed program or
binding payload under the same idempotency key conflicts through the existing
operation-digest rules. The begin record itself is bound to a digest of the
canonical program and ordered mutations, closing the crash window between
opening a lease and publishing or cancelling it.

The v1 program intentionally uses typed mutation bindings instead of embedding
a second mutation grammar inside RRFlowQL. Direct create/update/delete
expressions, interactive read-your-writes statements, public HTTP/MCP routes,
and generated language SDK methods remain later gates. They must lower to this
same engine transaction authority rather than introduce adapter-owned state.

## Qualification oracle

G03-W03 acceptance requires:

- parser/canonical round trips and strict malformed-program rejection;
- join equality with an independently constructed reference result on memory,
  Fjall compatibility, native RRD, and native reopen;
- a hard intermediate-cardinality error and exact one-row streaming batches;
- transaction commit, cancel, failed mutation, missing binding, exact retry,
  native reopen, and authorization-denial fixtures;
- the existing temporal, graph-traversal, geospatial, and typed-query suites to
  remain green under the shared planner and executor.
