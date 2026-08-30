# RRFlowQL semantic live-query foundation

Status: deterministic resumable polling and durable WebSocket push both reuse
the RRD query executor. `POST /v1/query/live/poll` remains the bounded fallback;
`POST /v1/subscriptions/open` plus the authenticated subscription WebSocket
provide server push, durable ACK state, bounded in-flight delivery, retention
floors, fencing generations, and restart-safe replay. The supported Rust client
exposes typed open/connect/receive/ACK/heartbeat/close operations.

A live query is an ordinary typed RRFlowQL read with explicit `AT VALID` and
`KNOWN HEAD`. The caller separately supplies the last consumed runtime cursor.
RRD query executor captures one immutable head, evaluates the same query at the resume and
head cursors, and differences rows by stable identity. The result contains
deterministically ordered `added`, `updated { before, after }`, and `removed`
rows plus `from_cursor`, `through_cursor`, and `head_cursor`.

The query digest binds the contract version, canonical query, and typed
parameters. A resume cursor beyond head, fixed `KNOWN` clauses, truncated
snapshot execution, duplicate identities, and zero/overflowing budgets fail
closed. A caller advances only to `through_cursor`; repeating that cursor is an
empty idempotent poll until authoritative state advances.

Three-engine differentials prove identical additions, updates, removals, empty
replay, digest identity, and budget denial on memory, Fjall compatibility, and
native RRD LSM. A real-process test proves unauthenticated denial and exact
authenticated delivery. A second real-process fixture proves an in-flight poll
wakes on a committed record update, returns the before/after delta, and times
out without cursor invention when no new commit arrives. The same delta is now
a durable `live_query` WebSocket frame; `rrd-live-subscriptions-v1.md`
specifies its delivery and replay contract.
