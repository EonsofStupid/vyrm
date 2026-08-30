# RRD time travel and forward rollback

Status: G02-W05 executable candidate. Record/relation rollback is implemented;
the wider multi-model rollback matrix remains explicit future work.

## Two independent timelines

RRFlowQL requires both coordinates:

- `AT VALID <millis>` selects modeled valid time.
- `KNOWN <cursor|HEAD>` selects the immutable transaction-log prefix known to
  the reader.

A literal known cursor bounds schema selection, source watermarks, claims,
index eligibility, and authoritative log replay. A materialized index newer
than the requested cursor is rejected and the planner uses the exact
authoritative prefix. Same-valid-time claim corrections resolve newest-first
within that prefix. Memory, Fjall compatibility, native RRD LSM, and native
reopen run the same contract.

## Forward-only rollback

`rrflow_data_rollback` accepts a stable idempotency key, target valid time,
target known cursor, later effective time, reason, and optional transaction
timeout. The engine requires the target cursor to be strictly older than the
transaction's original read cursor and the effective time to follow the target
valid time.

The planner reconstructs canonical record/relation snapshots at the current
and target coordinates, computes their exact differential, and emits an
ordered compensation transaction:

- target records/relations missing or changed at the current state receive new
  versions at the effective time;
- current records/relations absent from the target receive new closing
  versions ending at the effective time;
- a `historical-rollback` evidence claim binds the original read cursor,
  target coordinates, target-state digest, reason digest, and compensation
  counts.

The normal `DataTransaction` authority commits those mutations together. Its
read-stamp CAS, operation digest, idempotency state, projection outbox, runtime
change chain, and accepted-operation `AuditEnvelope` all remain in force.
Exact retry rebuilds the plan from the transaction's original read cursor and
returns the stored receipt; it cannot append a second rollback.

No historical change is edited or deleted. A query using a cursor before the
rollback still observes the pre-rollback state, while a head query at or after
the effective time observes the compensation.

## Deliberate boundary

This first rollback surface changes only canonical record and relation state.
It excludes event-derived synthetic graph rows so reasoning, lifecycle, event,
series, object, schema, vector, geo, and security-audit history remain
append-only. Those families require family-specific restoration/retirement
contracts and qualification before the capability can claim complete
multi-model rollback. The product catalogue exposes the implemented engine,
HTTP runtime-tool, MCP, and Connectome paths and labels CLI unavailable.
