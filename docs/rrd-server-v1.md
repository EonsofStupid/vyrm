# RRD server v1 implementation contract

Status: F2 alpha process implemented. The async loopback HTTP boundary and
persistent coordinator are shipped; the broader F2 administration,
cancellation, metrics, read-your-writes, and mutating-query exit gate remains
open.

The first RRD server is a new process boundary, not an HTTP wrapper around the
CLI and not an extension of `vyrmd`'s MCP protocol. `vyrmd` remains the AI-tool
adapter. Both consume `rrd-contract`; neither owns persistence semantics.

## Initial deployment boundary

Before F4 identity and authorization land, the server may bind only to an
explicit loopback address. Non-loopback startup fails closed. F2 session tokens
are unguessable transport leases, not user authentication, and the capability
document must advertise that limitation. TLS, principals, roles, scopes,
field/row policy, and enterprise audit are F4 gates before remote exposure.

Start the local process with:

```text
cargo run -p rrd-server -- --db PATH --instance INSTANCE --bind 127.0.0.1:9477
```

The database-local `RRD.SERVER.SECRET` is generated from OS entropy, requires
owner-only permissions on Unix, and makes lease-token derivation stable across
restart without storing bearer tokens. Supplying `--token-key-file PATH`
places that secret elsewhere. `X-RRD-Session` carries the session identifier;
`Authorization: Bearer TOKEN` carries its transport lease.

## HTTP resources

- `GET /v1/health/live` proves only that the process can answer.
- `GET /v1/health/ready` opens the configured instance, verifies its format,
  reads claim/runtime watermarks, and reports maintenance/cutover denial.
- `GET /v1/capabilities` returns the frozen `ServiceCapabilities` envelope.
- `POST /v1/sessions` creates a bounded lease with idle and absolute expiry,
  maximum concurrent transactions, and a server-generated secret token.
- `POST /v1/sessions/{session}/renew` rotates the token and never extends past
  absolute expiry.
- `POST /v1/query` authenticates the session, requires the exact
  `instance:<server-instance>` scope, and executes bounded VyrmQL through the
  VyrmMX binder, planner, and executor. The typed response includes the
  canonical query, read manifest, cursor, schema revision, selected and
  rejected plan candidates, execution evidence, and rows. This endpoint is a
  read-only F6 walking skeleton; mutating VyrmQL and live subscriptions remain
  open.
- `POST /v1/estates/{estate}/read` returns the typed public `EstateSnapshot`
  for a resource path containing that estate and this server instance. It
  requires a live session token, exposes no raw idempotency keys, and performs
  no estate mutation. Session leases are still transport credentials rather
  than F4 authorization.
- `DELETE /v1/sessions/{session}` closes the session and aborts its open
  transactions idempotently.
- `POST /v1/transactions` captures an Engine read stamp and creates one
  server-side transaction lease bound to exactly one instance and either the
  legacy `claims` scope or typed `data` scope.
- `POST /v1/transactions/{transaction}/preview` validates and returns the
  ordered prospective mutations, read cursor, and canonical operation digest.
  Full multi-model read-your-writes projection remains an F6 gate.
- `POST /v1/transactions/{transaction}/commit` requires a mutation
  idempotency key, exact operation digest, and deadline; it commits through
  the matching authoritative Engine transaction. The `claims` scope retains
  `Engine::append_batch_idempotent`. The `data` scope lowers the public typed
  schema, claim, record, relation, event, vector, series, geo, and pre-staged
  object-reference vocabulary into one atomic `RuntimeCommit` with exact
  cursor conflict detection.
- `DELETE /v1/transactions/{transaction}` aborts idempotently.

Every response uses `ResponseEnvelope`; every failure uses the stable
`ErrorCode`. Payload schemas must be added to `rrd-contract` before handler
code, and public payloads must not serialize private `vyrm_core` types.
Mutating envelopes require a client idempotency key. Operation digests use
`rrd_contract::transaction_operation_sha256`, which hashes the stable typed
JSON mutation order rather than arbitrary incoming object-key order; its
golden vector is checked into the public-contract tests.

## Persistent idempotency

An in-memory key map is insufficient. The server requires one authoritative
keyspace binding `(instance, session, idempotency_key)` to operation SHA-256 and
the accepted response identity in the same commit as the mutation. Replaying
the same key/digest returns the original result; the same key with a different
digest returns `conflict`; no accepted mutation may be repeated after server
restart. Session creation and transaction begin derive stable identities from
the server secret plus the client key. Renew, close, abort, and commit retain
their accepted key/digest bindings in session state. Runtime commit content
identity remains a second independent guard.

## Authoritative lifecycle journal

Session and transaction state is not process memory and the journal is not a
best-effort log. Every accepted lifecycle transition uses Engine compare-and-
swap to update its materialized record and append a monotonically sequenced,
SHA-256-chained journal entry in the same storage transaction. Each entry
contains event time, actor/action, request and operation identities, the prior
state digest, and the complete replacement state required to replay it.

Persisted session state and journal entries contain only the session-token
SHA-256; raw bearer tokens are derived for responses and never journaled.
Session expiry atomically marks all open transactions expired. Begin, commit
prepare, commit completion, abort, renewal, close, preview, transaction expiry,
and session expiry have explicit lifecycle events. An idempotent response
replay does not invent a second lifecycle transition. Successful activity
advances idle expiry but never crosses absolute expiry. A compare-and-swap
conflict fails closed rather than overwriting a concurrent lifecycle event.

Claim and data acceptance use a recoverable three-transition protocol until
the broader F6 transaction engine owns it as one generalized transaction: first
`transaction.commit_prepared` durably binds the only permitted key/digest,
then the authoritative claim or runtime commit stores its receipt atomically
with data, then `transaction.committed` records the terminal state. A data
intent also freezes runtime time and content digest; recovery resolves that
digest through the authoritative runtime commit catalogue. A stop at either
gap resumes only the prepared identity and cannot accept a different retry
key or duplicate data. Recovery after process restart is exercised. This is
deliberately not presented as one cross-keyspace commit.

## Time and resource invariants

- The process reads its clock once per request and passes that value inward.
- An already-expired request is denied before storage I/O.
- A deadline expiring before commit publication returns `deadline_exceeded`
  only if no commit was accepted; otherwise the durable accepted result wins.
- Session/transaction counts, body bytes, mutation count, and lease duration
  have configured hard bounds. Result-byte and execution-time caps remain
  open for generalized work. Query input, parameter count/bytes, scanned
  changes, returned rows, batch rows, and encoded output bytes are bounded by
  the public query contract.
- Expiry cleanup is idempotent and never deletes canonical data.
- Disconnect does not imply abort or commit; the transaction remains governed
  by its lease and idempotency identity.

## Black-box exit matrix

The current real-socket matrix proves liveness/readiness on reopen, capability
negotiation, malformed/oversized-body denial, wrong-instance denial, elapsed
deadline denial before mutation, token rotation and replay, idle expiry,
transaction quota, claim-scope preview, explicit abort and close, same-process
and post-restart commit replay, idempotency collision, disconnect/retry,
concurrent same-operation convergence, graceful shutdown, and refusal by both
the library and real binary to bind remotely before F4.

The matrix also proves that an unauthenticated estate read is denied, URL and
resource-estate identities must agree, and the response is the public snapshot
rather than the persisted authority document.

The real-socket matrix proves the same authentication and scope denial for the
query endpoint and verifies an exact persisted-record query, typed row output,
planner candidates, validation evidence, and cursor/schema coordinates.

It also commits all nine runtime mutation families through one `data`
transaction, verifies the single eleven-change cursor interval and one-claim
receipt, restarts the server, replays the same runtime commit identity, and
confirms that the authoritative scoped log contains exactly eleven changes.

F2 is not closed by that matrix. Remaining black-box gates are cancellation of
long-running query work, deadline races during generalized commit, prospective
multi-model read-your-writes, lost-ack process interruption for the data scope,
mutating VyrmQL, live subscriptions,
CRUD/schema/vector/snapshot administration, generalized result/time limits,
durable query-span export, metrics export, and released-version negotiation
clients.
