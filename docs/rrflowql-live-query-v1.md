# RRFlowQL semantic live-query foundation

Status: deterministic resumable polling exists inside RRD query executor and through the
authenticated `POST /v1/query/live/poll` RRD route. The operation has its own
deny-by-default security action and is present in all six generated SDK route
catalogues. The request may now wait up to five seconds for the authoritative
cursor to advance, with deadline preflight and explicit `timed_out`/`waited_ms`
evidence. Typed ergonomic SDK methods, streaming transport, backpressure, and a
retained subscription registry remain open.

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
out without cursor invention when no new commit arrives. This is bounded
resumable long-polling, not yet a streaming or push-subscription claim.
