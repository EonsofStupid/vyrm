# RRD security authority v1

Status: G05-W03 identity/authorization and G05-W04 comprehensive structured
audit foundations implemented. The durable authority, inherited roles,
API-key and short-lived JWT sessions, exact third-party identity bindings,
tenant/row/field query policy, TLS 1.3 mTLS, rotation/revocation, independent
hash-chained audit, protected read, and canonical JSON Lines export are
executable. An external OIDC/JWK verifier adapter, certificate hot reload,
secret-provider integration, rate limits, atomic completion in the same data
transaction, retention policy, and external sink delivery remain later
production hardening.

`rrd-security` owns identity, deny-by-default action policy, and the durable
audit vocabulary. It is deliberately separate from RRFlow physical storage,
RRD query execution, RRO provisioning, and Connectome presentation.

## Persistent contract

- A per-instance `SecurityState` contains an exact format/revision and bounded
  canonical maps for principals, roles, third-party identity bindings, and JWT
  issuers. Ongoing administration replaces the complete authority with one
  compare-and-swap revision; exact replay is a no-op and stale or substituted
  state is denied.
- A principal is a user, service, or node identity with a lowercase SHA-256
  credential verifier, credential revision, validity window, disable flag,
  direct grants, and role memberships.
- A grant combines one closed action enum with an exact canonical resource
  prefix and an optional typed data policy. Roles form a bounded acyclic
  inheritance graph; there are no implicit wildcard strings, administrator
  rights, or allow-on-missing-policy behavior. Equally specific grants with
  different data policies fail closed.
- Authentication compares the supplied credential verifier without an early
  byte mismatch. Raw credentials are never serialized into state or audit.
- Authorization fails when security is uninitialized, the principal is
  unknown/disabled/outside its validity window, the credential differs, or no
  exact action/resource-prefix grant matches.

## Credentials and external identity

RRD issues bounded HS256 JWT credentials only after the principal authenticates
and has `session_create`. Header and claims bind the exact issuer, audience,
key identity, principal, token identity, issued/not-before/expiry window,
security revision, and principal credential revision. Only the signing-key
SHA-256 is persisted. Verification reloads the current authority, so principal
disablement, issuer disablement/key rotation, and credential rotation reject an
otherwise valid older token. The server accepts these credentials only when an
owner-only mounted key file matches the configured issuer.

External identity is an explicit post-verification boundary. An adapter must
first cryptographically verify the third-party assertion; RRD then resolves an
exact persisted issuer/subject/audience tuple to one current principal and
applies `session_create`. The authority does not trust unsigned identity
headers and does not claim to be an OIDC signature/JWK implementation.

Every authenticated session persists the principal credential revision used to
create it. The shared engine invocation boundary and domain authorization both
compare that revision with current policy, so rotation invalidates existing
API-key, JWT, and externally bound leases across restart.

## Data policy

A grant may carry one tenant equality predicate, bounded row equality
predicates, and an allowed-field set. RRFlowQL query execution compiles the
current authorization for the requested resource, injects the tenant/row
filters and field projection before binding and planning, and stamps the result
with the policy revision and authorization digest. A requested forbidden field
returns `permission_denied`. Operations without a policy injector deny a
constrained grant instead of silently widening it.

The closed action vocabulary covers session lifecycle, query and index work,
transaction lifecycle, changefeed/subscription work, vector collections and
points, backup/list/restore, estate read/administration, audit read/export,
diagnostics, security administration, and governed RRFlow runtime actions.

## Audit contract

An `AuditRecord` captures a canonical audit identity, time, optional principal,
closed action, exact resource, request/operation coordinates, allow/deny/fail
decision, authorization/completion phase, HTTP-style status, request/response
SHA-256 values, the previous audit digest, and its own semantic digest. Bodies,
credentials, bearer tokens, signing keys, filesystem paths, and arbitrary
headers are excluded.

Each record is immutable and idempotent by audit identity. Rebinding that
identity is denied. Audit owns an independent genesis-to-head hash chain rather
than inheriting trust from a filtered view of the broader control journal. The
record and compare-and-swap audit head publish in one bounded atomic control
batch; concurrent writers retry the head conflict and cannot create forks.
Restart reads validate every record digest and the requested chain segment.
Bounded reads retain the global control-journal sequence, anchor digest, and
head digest. `through_sequence` is the global coordinate scanned, so unrelated
control activity cannot stall pagination.

`POST /v1/audit/read` is protected by `audit_read`. The separately authorized
`POST /v1/audit/export` returns canonical newline-delimited JSON with exact
record count, chain anchor/head, media type, and content SHA-256. Contract and
client validation parse every line, recompute every record digest, verify the
lineage, and reject framing, count, content, or chain substitution.

After successful principal authorization, the server appends an `authorized`
record before invoking the operation. A missing completion therefore remains
visible after a process or audit-writer failure. Denied requests receive only a
terminal `completed/denied` record; accepted work receives a terminal
`completed/allowed` or `completed/failed` record after its response is known.
Authorization records are constrained to status 100/allowed and completion
records to final status codes.

Security initialization/replacement publishes its state transition and
`security_admin` completion audit in the same control batch. Authorized estate
administration, reconciliation, backup reconciliation, retention pruning, and
restore recovery emit `estate_admin` authorization and completion records.
Session-backed embedded calls outside an active invocation cannot disappear:
they emit a durable authorization reservation or terminal denial. A missing
completion therefore remains explicit evidence of a crashed or nonconforming
caller instead of looking like an operation that never occurred.

## Evidence and remaining gate

Memory, Fjall-compatibility, and native tests prove atomic multi-record control
publication and rollback. Native reopen and concurrent-writer tests prove
exact-scope allow, wrong credential denial, wrong-instance denial, audit replay,
identity collision denial, linear no-fork chaining, journal verification, and
absence of raw credentials from serialized evidence.

When an instance has initialized security state, session creation accepts
`X-RRD-Principal` plus `Authorization: ApiKey …`, or an RRD-issued
`Authorization: Bearer …` JWT without a caller-selected principal header. It
persists the authenticated principal and credential revision on the short-lived
session. Every authenticated endpoint maps to one closed action and
re-evaluates current policy. Session bearer credentials cannot change their
bound principal.

An instance with no initialized authority continues in an explicitly
advertised loopback development mode for compatibility. This never permits
remote bind. The real-socket differentials prove missing/bad API-key denial,
JWT exchange, allowed and forbidden-field query decisions, denied-audit
persistence, exact session replay, credential rotation, rejection of an old
JWT and its existing lease, ungranted backup denial without a runtime mutation,
failed query execution, bounded protected audit read and export, all three
decision classes, oversized-body rejection, handler-join failure attribution,
chain continuity, export digest verification, and absence of raw
API-key/JWT/signing-key material from reopened storage. Public inspection and
unknown routes are attributed too. Remote cleartext remains prohibited;
non-loopback service requires initialized application security plus
client-authenticated TLS 1.3.
