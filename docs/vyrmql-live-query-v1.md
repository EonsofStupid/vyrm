# VyrmQL semantic live-query foundation

Status: deterministic resumable polling exists inside VyrmMX. The authenticated
RRD route, long-poll wakeup, streaming transport, SDK surface, and retained
subscription registry remain open.

A live query is an ordinary typed VyrmQL read with explicit `AT VALID` and
`KNOWN HEAD`. The caller separately supplies the last consumed runtime cursor.
VyrmMX captures one immutable head, evaluates the same query at the resume and
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
native VyrmKV. This is semantic realtime, not yet a network delivery claim.
