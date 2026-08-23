# RRD server v1 implementation contract

Status: F2 dependency contract. The persistent coordinator is implemented; no
network endpoint is claimed as shipped by this document.

The first RRD server is a new process boundary, not an HTTP wrapper around the
CLI and not an extension of `vyrmd`'s MCP protocol. `vyrmd` remains the AI-tool
adapter. Both consume `rrd-contract`; neither owns persistence semantics.

## Initial deployment boundary

Before F4 identity and authorization land, the server may bind only to an
explicit loopback address. Non-loopback startup fails closed. F2 session tokens
are unguessable transport leases, not user authentication, and the capability
document must advertise that limitation. TLS, principals, roles, scopes,
field/row policy, and enterprise audit are F4 gates before remote exposure.

## HTTP resources

- `GET /v1/health/live` proves only that the process can answer.
- `GET /v1/health/ready` opens the configured instance, verifies its format,
  reads claim/runtime watermarks, and reports maintenance/cutover denial.
- `GET /v1/capabilities` returns the frozen `ServiceCapabilities` envelope.
- `POST /v1/sessions` creates a bounded lease with idle and absolute expiry,
  maximum concurrent transactions, and a server-generated secret token.
- `POST /v1/sessions/{session}/renew` rotates the token and never extends past
  absolute expiry.
- `DELETE /v1/sessions/{session}` expires the session and aborts its open
  transactions idempotently.
- `POST /v1/transactions` captures an Engine read stamp and creates one
  server-side transaction lease bound to exactly one instance and scope.
- `POST /v1/transactions/{transaction}/preview` validates and returns
  read-your-writes state without mutation.
- `POST /v1/transactions/{transaction}/commit` requires a mutation
  idempotency key, exact operation digest, and deadline; it commits through
  the matching authoritative Engine transaction. The first implemented
  mutation surface uses `Engine::append_batch_idempotent`; later multi-model
  mutations must retain the same durable acceptance contract.
- `DELETE /v1/transactions/{transaction}` aborts idempotently.

Every response uses `ResponseEnvelope`; every failure uses the stable
`ErrorCode`. Payload schemas must be added to `rrd-contract` before handler
code, and public payloads must not serialize private `vyrm_core` types.

## Persistent idempotency

An in-memory key map is insufficient. The server requires one authoritative
keyspace binding `(instance, session, idempotency_key)` to operation SHA-256 and
the accepted response identity in the same commit as the mutation. Replaying
the same key/digest returns the original result; the same key with a different
digest returns `conflict`; no accepted mutation may be repeated after server
restart. Runtime commit content identity remains a second independent guard.

## Authoritative lifecycle journal

Session and transaction state is not process memory and the journal is not a
best-effort log. Every accepted lifecycle transition uses Engine compare-and-
swap to update its materialized record and append a monotonically sequenced,
SHA-256-chained journal entry in the same storage transaction. Each entry
contains event time, actor/action, request and operation identities, the prior
state digest, and the complete replacement state required to replay it.

Persisted session state and journal entries contain only the session-token
SHA-256; the raw bearer token is returned once and is never journaled. Session
expiry atomically marks all open transactions expired. Begin, commit, replay,
abort, abort replay, transaction expiry, and session expiry are explicit
events. Successful activity advances idle expiry but never crosses absolute
expiry. A compare-and-swap conflict fails closed rather than overwriting a
concurrent lifecycle event.

Claim acceptance and the session terminal transition are deliberately
recoverable as two durable steps until the broader F6 transaction engine owns
them together. If the process stops after claim acceptance but before
`transaction.committed`, retrying the same key and operation digest returns the
stored claim receipt without duplicating claims, then closes the lifecycle
journal gap. This crash window is an exercised recovery contract, not an
atomicity claim across two commits.

## Time and resource invariants

- The process reads its clock once per request and passes that value inward.
- An already-expired request is denied before storage I/O.
- A deadline expiring before commit publication returns `deadline_exceeded`
  only if no commit was accepted; otherwise the durable accepted result wins.
- Session/transaction counts, body bytes, mutation count, result bytes, and
  execution time have configured hard bounds.
- Expiry cleanup is idempotent and never deletes canonical data.
- Disconnect does not imply abort or commit; the transaction remains governed
  by its lease and idempotency identity.

## Black-box exit matrix

The binary is qualified only when tests use a real loopback socket and prove:
readiness on reopen; capability negotiation; malformed/oversized-body denial;
session idle/absolute expiry and token rotation; per-session transaction quota;
scope isolation; read-your-writes preview; explicit abort; stale-CAS conflict;
deadline before/during commit; same-process and post-restart idempotent replay;
idempotency collision; disconnect/retry; concurrent commits; clean shutdown;
and refusal to bind remotely before F4.
