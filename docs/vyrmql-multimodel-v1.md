# VyrmQL multi-model read foundation

Status: executable stamped reads for records, relations, events, claims,
time-series samples, geospatial values, and bounded recursive graph traversal.
General operators, indexes, full text, mutating statements, and push
subscriptions remain open.

VyrmQL sources now include:

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
results on Memory, Fjall compatibility, and native VyrmKV. It also proves
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

Numeric/spatial operators, index catalogues and lifecycle, planner statistics,
full text, mutating VyrmQL, streaming responses, and push live subscriptions
remain explicit gaps.
