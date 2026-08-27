# RRD local estate authorization v1

Status: F3 local-only alpha boundary. It authorizes explicit local estate
creation, desired-state mutation, and quiesced backup scheduling; it does not
authorize a remote listener or replace F4 identity, RBAC/ABAC, credential
rotation, or comprehensive audit.

## Policy and key boundary

`LocalOperatorPolicy` is strict JSON, capped at one MiB and versioned with
`format: 1`. It binds:

- one canonical operator identity;
- the SHA-256 of one exact 32-byte operator key;
- an inclusive `not_before_unix_ms` and exclusive `expires_at_unix_ms` window;
- at most 1,024 exact canonical estate IDs; and
- an explicit set of `create`, `set_desired`, and/or `schedule_backup`
  permissions per estate.

There is no wildcard estate or action. Unknown JSON fields, empty grants,
non-canonical IDs, malformed digests, invalid windows, wrong key bytes, wrong
estate, wrong action, and expired/not-yet-valid requests fail before storage is
opened. On Unix, both policy and key files must deny all group/world permission
bits. Windows relies on the file ACL and exact key digest in this alpha slice;
ACL-owner inspection remains an F4 hardening item.

## Local admin process

`rrd-estate-admin` has three explicit actions:

```text
rrd-estate-admin create ...
rrd-estate-admin set-desired ...
rrd-estate-admin schedule-backup ...
```

All actions require database, an explicit estate-control authority instance,
policy, key, estate, timestamp, request ID, and operation ID arguments. The
authority instance identifies the running RRD engine and must not be replaced by
the managed estate ID. Desired-state mutation additionally requires instance,
idempotency key, phase, deployment, version, and configuration SHA-256. Backup
scheduling requires instance, idempotency key, and canonical label; the estate
authority rejects it unless desired and observed state are stopped at the same
generation with no process ID. No shell string, secret value, implicit current
account, wildcard target, or remote session credential enters the estate
document.

The executable is an outward adapter in `rrflow-cli`. Authorization, repository
construction, local authority opening, and mutation execution are one typed
`RrdEngine` operation; `rrd-estate` owns state-machine contracts but no longer
owns or opens an admin executable.

Authorization supplies the journal actor; callers cannot override it. Accepted
mutations return the frozen `rrd-contract::EstateMutationResult`, or the
separate `EstateBackupMutationResult`. Both contain the public estate
projection and an `idempotent_replay` flag; the backup result also carries its
strict job projection.

## Replay and recovery

Desired-state and backup scheduling replay use the aggregate's durable
idempotency bindings. Estate creation replay matches the exact estate control
key, actor, request ID,
operation ID, and timestamp in the authenticated control journal. Its lookup is
bounded to 65,536 journal entries; work outside that alpha window fails rather
than performing an ambiguous second create.

The black-box test denies an unauthorized estate before its database exists,
creates an authorized estate, replays the exact create, sets desired state,
replays it, reaches a quiesced observation, schedules and replays a backup, then
reopens the native database and verifies the exact operator identity on the
accepted backup schedule journal entry.

## Deliberate limits

- No HTTP mutation route is enabled.
- No policy/key generator or credential rotation exists yet.
- No Windows ACL-owner verification exists yet.
- No organization/account hierarchy, role inheritance, ABAC conditions,
  approval workflow, revocation list, or remote authentication exists yet.
- F4 must reuse the public mutation result and estate state-machine semantics;
  it must not reinterpret this local key as a user/session credential.
