# RRD security authority v1

Status: persistent F4 authority core implemented; HTTP enforcement, principal
provisioning APIs, TLS/mTLS, secrets providers, row/field policy, rate limits,
and complete endpoint audit integration remain open.

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
decision, HTTP-style status, and request/response SHA-256 values. Bodies,
credentials, bearer tokens, and arbitrary headers are excluded.

Each record is immutable and idempotent by audit identity. Rebinding that
identity is denied. The record is appended through Vyrm's authenticated
control journal, so restart replay validates the existing journal chain rather
than trusting a detached log file. Bounded reads return only typed
`security.audit` records while retaining their global control-journal sequence.

## Evidence and remaining gate

Native reopen tests prove exact-scope allow, wrong credential denial,
ungranted-action denial, wrong-instance denial after restart, audit replay,
audit identity collision denial, journal verification, and absence of the raw
credential from serialized journal evidence.

This authority core does not by itself complete F4. The next slice binds every
RRD HTTP route to one action, authenticates session creation as a principal,
persists the principal on each session, and records allowed, denied, and failed
outcomes. Remote listening remains prohibited until TLS/mTLS and that endpoint
differential are complete.
