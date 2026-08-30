# RRFlowQL multi-model read foundation

Status: executable stamped reads for records, relations, events, claims,
time-series samples, geospatial values, and bounded recursive graph traversal,
with typed scalar comparison predicates. Unified indexes, configurable BM25,
and durable push subscriptions are present; richer mutating statements remain
open.

RRFlowQL sources now include:

- `record:<kind>` for bitemporal records;
- `relation:<kind>` for directed typed relations;
- `event:<kind>` for immutable cursor-addressed events;
- `claim` or `claim:<predicate>` for resolved bitemporal claims;
- `series:<kind>` where `<kind>` is the referenced series-record kind;
- `geo:<kind>` where `<kind>` is the geospatial value's reference kind.
- `traverse:<relation> START <kind>:<id> DIRECTION <OUTGOING|INCOMING|BOTH>
  DEPTH <1..32>` for bounded recursive graph expansion.

All six families use explicit `AT VALID` and `KNOWN` coordinates, bind against
the captured schema/read stamp, receive a content-addressed plan, enforce scan/
row/output/batch budgets, and execute against the authoritative runtime log.
Series rows expose sample identity, series identity, observation time, and the
typed scalar value. Geo rows expose identity, subject, field, validity window,
geometry kind, and point or bounding-box coordinates represented as canonical
decimal values. Custom properties remain visible under `PROJECT *`, but only
the frozen built-in fields are currently bindable for series/geo filtering and
explicit projection.

The differential fixture commits schema, records, relation, event, claim,
series sample, and geo point together, then proves identical stamped series/geo
results on Memory, Fjall compatibility, and native RRD LSM. It also proves
domain-time exclusion before a sample observation or geo validity start. A
secured real RRD process accepts the same transaction and returns the exact
typed rows through `/v1/query` before and after persistence replay.

This is the first F6 breadth slice, not completion of the query surface.
Traversal validates the start and relation types against the captured schema,
uses only the bitemporal graph snapshot visible at the requested coordinates,
orders relations canonically, returns the first deterministic shortest path to
each reached node, and suppresses cycles with a visited set. Direction and a
hard maximum depth are mandatory; every row exposes depth, reached node, edge,
endpoints, and the complete node path. Three-engine and secured RRD tests prove
outgoing/incoming semantics and cycle termination.

Filters preserve a typed comparison operator through parsing, binding, plan
digests, and execution. The grammar supports `=`, `!=`, `<`, `<=`, `>`, and
`>=`. Equality/inequality work for every type accepted by the selected field;
ordering is currently fail-closed to same-type integer, unsigned integer, and
string values. Decimal and mixed-type ordering are rejected at binding rather
than compared lexically or coerced. The exact event-cursor lookup is selected
only for `cursor = <unsigned>`; other cursor comparisons keep the authoritative
scan.

The initial index catalogue/lifecycle and planner rejection evidence are
documented in [`rrflowql-index-catalogue-v1.md`](rrflowql-index-catalogue-v1.md).
Filtered materialized views, geo selection, planner analysis, BM25, and durable
push live queries now use the same stamped execution authority. Richer mutating
RRFlowQL remains an explicit gap. Exact-cursor semantic delta polling is
documented in [`rrflowql-live-query-v1.md`](rrflowql-live-query-v1.md), and its
WebSocket delivery contract is documented in
[`rrd-live-subscriptions-v1.md`](rrd-live-subscriptions-v1.md).
