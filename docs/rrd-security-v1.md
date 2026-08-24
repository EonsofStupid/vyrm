# RRD security authority v1

Status: persistent F4 authority, HTTP action enforcement, routed-outcome audit,
and policy-protected audit read implemented. Principal provisioning APIs,
TLS/mTLS, secrets providers, row/field policy, rate limits, atomic audit
completion with application mutation, and external audit archival remain open.

`rrd-security` owns identity, deny-by-default action policy, and the durable
audit vocabulary. It is deliberately separate from Vyrm physical storage,
RRD query execution, RRO provisioning, and Connectome presentation.

## Persistent contract

- A per-instance `SecurityState` contains an exact format/revision and a
  bounded canonical principal map.
- A principal is a user, service, or node identity with a lowercase SHA-256
  credential verifier, validity window, disable flag, and bounded grants.
- A grant combines one closed action enum with an exact canonical resource
  prefix. There are no implicit wildcard strings, inherited administrator
  rights, or allow-on-missing-policy behavior.
- Authentication compares the supplied credential verifier without an early
  byte mismatch. Raw credentials are never serialized into state or audit.
- Authorization fails when security is uninitialized, the principal is
  unknown/disabled/outside its validity window, the credential differs, or no
  exact action/resource-prefix grant matches.

The initial action vocabulary covers session lifecycle, query, transaction
lifecycle, changefeed read/follow, vector search, backup/list/restore, estate
read, audit read, and security administration.

## Audit contract

An `AuditRecord` captures a canonical audit identity, time, optional principal,
closed action, exact resource, request/operation coordinates, allow/deny/fail
decision, authorization/completion phase, HTTP-style status, and request/
response SHA-256 values. Bodies,
credentials, bearer tokens, and arbitrary headers are excluded.

Each record is immutable and idempotent by audit identity. Rebinding that
identity is denied. The record is appended through Vyrm's authenticated
control journal, so restart replay validates the existing journal chain rather
than trusting a detached log file. Bounded reads return only typed
`security.audit` records while retaining their global control-journal sequence.
The public `POST /v1/audit/read` contract is itself protected by `audit_read`.
Its `through_sequence` reports the global journal coordinate scanned, rather
than merely the last matching audit record, so unrelated control activity
cannot stall pagination.

After successful principal authorization, the server appends an `authorized`
record before invoking the operation. A missing completion therefore remains
visible after a process or audit-writer failure. Denied requests receive only a
terminal `completed/denied` record; accepted work receives a terminal
`completed/allowed` or `completed/failed` record after its response is known.
Authorization records are constrained to status 100/allowed and completion
records to final status codes.

## Evidence and remaining gate

Native reopen tests prove exact-scope allow, wrong credential denial,
ungranted-action denial, wrong-instance denial after restart, audit replay,
audit identity collision denial, journal verification, and absence of the raw
credential from serialized journal evidence.

When an instance has initialized security state, the RRD server requires
`X-RRD-Principal` plus `Authorization: ApiKey …` for session creation. It
persists the authenticated principal on the short-lived session. Every existing
authenticated endpoint maps to one closed action and re-evaluates current
policy, so disabling a principal or removing a grant affects existing sessions.
The API key is used only to establish a session; subsequent calls use the
rotatable bearer lease and cannot change its bound principal.

An instance with no initialized authority continues in an explicitly
advertised loopback development mode for compatibility. This never permits
remote bind. The real-socket differential proves missing/bad API-key denial,
authorized query execution, ungranted backup denial without a runtime
mutation, failed query execution, bounded protected audit read, all three
decision classes, public inspection, unknown-route and missing-bearer coverage,
and absence of raw API-key material from both public records and the reopened
authenticated journal.

F4 is still open. Routed envelope operations, public inspection, and unknown
routes now record outcomes when security is enabled, and authorized work is
reserved durably before execution. Atomic completion in the same transaction
as application mutation, oversized-body/handler-failure coverage, rotation/
export to an external archive, and retention remain before calling the audit
comprehensive. Remote listening remains prohibited until TLS/mTLS and the
complete endpoint differential are proven.
