# RRD local estate authorization v1

Status: F3 local-only alpha boundary. It authorizes explicit local estate
creation and desired-state mutation; it does not authorize a remote listener or
replace F4 identity, RBAC/ABAC, credential rotation, or comprehensive audit.

## Policy and key boundary

`LocalOperatorPolicy` is strict JSON, capped at one MiB and versioned with
`format: 1`. It binds:

- one canonical operator identity;
- the SHA-256 of one exact 32-byte operator key;
- an inclusive `not_before_unix_ms` and exclusive `expires_at_unix_ms` window;
- at most 1,024 exact canonical estate IDs; and
- an explicit set of `create` and/or `set_desired` permissions per estate.

There is no wildcard estate or action. Unknown JSON fields, empty grants,
non-canonical IDs, malformed digests, invalid windows, wrong key bytes, wrong
estate, wrong action, and expired/not-yet-valid requests fail before storage is
opened. On Unix, both policy and key files must deny all group/world permission
bits. Windows relies on the file ACL and exact key digest in this alpha slice;
ACL-owner inspection remains an F4 hardening item.

## Local admin process

`rrd-estate-admin` has two explicit actions:

```text
rrd-estate-admin create ...
rrd-estate-admin set-desired ...
```

Both require database, policy, key, estate, timestamp, request ID, and operation
ID arguments. Desired-state mutation additionally requires instance,
idempotency key, phase, deployment, version, and configuration SHA-256. No shell
string, secret value, implicit current account, wildcard target, or remote
session credential enters the estate document.

Authorization supplies the journal actor; callers cannot override it. Accepted
mutations return the frozen `rrd-contract::EstateMutationResult`, containing the
public `EstateSnapshot` and an `idempotent_replay` flag.

## Replay and recovery

Desired-state replay uses the aggregate's durable idempotency binding. Estate
creation replay matches the exact estate control key, actor, request ID,
operation ID, and timestamp in the authenticated control journal. Its lookup is
bounded to 65,536 journal entries; work outside that alpha window fails rather
than performing an ambiguous second create.

The black-box test denies an unauthorized estate before its database exists,
creates an authorized estate, replays the exact create, sets desired state,
replays it, reopens the native database, and verifies revision, instance state,
and the two hash-chained journal actions under the exact policy operator.

## Deliberate limits

- No HTTP mutation route is enabled.
- No policy/key generator or credential rotation exists yet.
- No Windows ACL-owner verification exists yet.
- No organization/account hierarchy, role inheritance, ABAC conditions,
  approval workflow, revocation list, or remote authentication exists yet.
- F4 must reuse the public mutation result and estate state-machine semantics;
  it must not reinterpret this local key as a user/session credential.
