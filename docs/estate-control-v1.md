# RRD estate control contract v1

Status: F3 alpha contract. This document freezes the first persistent estate
authority; it does not claim that deployment drivers or remote administration
are complete.

## Ownership

RRD persists authoritative estate state. `rrd-estate` owns the transport-free
state machine and reconciliation vocabulary. RRO will schedule and compose
reconciliation. Connectome consumes a read-only projection and must not invent
or overwrite authoritative state.

An estate is stored as one bounded compare-and-swap document at
`server/state/estate/{estate_id}/document`. Every accepted replacement and its
request/operation identity are committed atomically to Vyrm's authenticated
control journal. The v1 aggregate is intentionally bounded to 1,024 instances,
4,096 operations, and 4,096 idempotency bindings; sharding the catalogue is a
future persisted-format migration, not an implicit layout change.

## Invariants

1. The document format and revision are explicit. Revisions increase by one.
2. Desired generations are monotonic per instance. Desired state names a
   deployment reference, target version, and configuration digest, never a
   shell command or secret value.
3. Observed state is evidence, not desired state. It records the generation a
   driver actually observed and cannot advance beyond the desired generation.
4. Every desired mutation binds an idempotency key to one canonical request
   digest and operation ID. Reusing the key for different work is denied.
   A newer desired generation terminally marks older unfinished operations as
   `superseded`; a stale target is never applied after a newer one.
5. A reconciliation operation advances through pending, leased, prepared,
   applied, and a terminal state. A prepared record is durable before an
   external driver is invoked. Driver calls carry the stable operation ID.
6. One unexpired lease owns an operation. Lease epochs strictly increase so a
   stale worker cannot publish a receipt after another worker takes over.
7. Receipts are append-only within an operation and bind the boundary, lease
   epoch, evidence digest, and timestamp.
8. Activity classification is derived from persisted last-meaningful-runtime
   evidence and explicit estate thresholds. Wall-clock absence is `unknown`,
   never guessed as active.
9. Secret material, bearer tokens, arbitrary command strings, and UI-only
   health labels are forbidden from this document.

## Initial state vocabulary

Desired phases are `running`, `stopped`, and `absent`. Observed phases are
`unknown`, `provisioning`, `starting`, `running`, `stopping`, `stopped`,
`deleting`, `absent`, and `failed`. Operations are `provision`, `start`, `stop`,
`restart`, `upgrade`, and `delete`.

Activity thresholds are ordered `active <= idle <= stale <= neglected` through
three durations: `idle_after_ms`, `stale_after_ms`, and
`neglected_after_ms`. Connectome may decorate these facts but must retain the
evidence timestamp, evaluation timestamp, and threshold policy.

## Recovery boundary

The reconciler advances at most one durable boundary per call. It writes
`prepared`, invokes a typed driver method with the operation ID (never
shell-string execution), then writes an `applied` receipt. Replaying a prepared
operation invokes the driver with the same stable ID. A driver fixture that
persists its own receipts proves convergence when an effect commits but its
first acknowledgement is lost. The native engine is dropped and reopened
between lease, prepared, retry, applied, observed and completed steps.

This is state-machine recovery evidence, not yet production process-driver
qualification. The next driver slice must bind trusted executable paths and
argument vectors without a shell, preserve per-instance process identity across
controller restart, and run actual process-kill tests at the same boundaries.

## Read projections

`rrd-contract::EstateSnapshot` is the stable outward projection; the internal
aggregate is not serialized as an accidental SDK. It retains desired/observed
generations, activity evidence, leases, operation receipts and errors but
exposes only the number of idempotency bindings, not their client keys.

RRD serves it through the session-authenticated
`POST /v1/estates/{estate}/read` endpoint. Connectome projects the same type in
`/api/estate` and the full workbench snapshot. When no authoritative document
exists, Connectome labels its manifest-derived row `synthetic` and never
presents it as reconciled control-plane state. Both Connectome paths are
read-only and tests prove they do not advance the estate revision.
