# RRD Rust client v1

Status: asynchronous local-daemon plus experimental remote-mTLS client walking
skeleton implemented against the entire currently published RRD operation catalogue.
Distributed qualification, generated API reference, and a released-version
matrix remain open.

`rrd-client` is the first supported-client boundary. It depends on
`rrd-contract` and HTTP transport crates, never on RRFlow storage, RRFlowQL/RRD query executor,
RRD server, estate, or security internals. Server/security dependencies are
dev-only black-box fixtures.

## Transport and protocol invariants

- Plain HTTP construction accepts only an explicit loopback socket. Remote
  construction requires an HTTPS origin and caller-supplied Rustls client
  identity/trust configuration; the server still requires mTLS and application
  authorization.
- Capability and endpoint-catalogue calls validate protocol identity/version,
  capability ordering, and exact instance identity.
- Every envelope is constructed from public resource and correlation types and
  validated before network I/O.
- Every response must agree across HTTP status and typed outcome, use protocol
  v1, and echo request/operation identity for envelope calls.
- Response accumulation is capped at four MiB while frames are read; a declared
  timeout and optional absolute request deadline bound each attempt.
- Dropping the async future cancels client-side waiting. Deadline expiration is
  detected before an attempt.
- Reads and idempotency-bound mutations retry only transport loss/timeout, at
  most the configured 1–8 attempts. API responses are never blindly replayed.
- API-key and bearer values exist only in request headers and are not included
  in typed errors.

## Current typed surface

The client covers capability, endpoint, and OpenAPI negotiation; session create/renew/
close; transaction begin/preview/commit/abort; RRFlowQL query; vector search;
changefeed read/follow; durable subscription open/connect/receive/ACK/heartbeat/
close over `ws://` or mTLS `wss://`; backup create/list/restore; estate read;
and audit read.
`RequestOptions` makes correlation, deadline, and mutation idempotency explicit.
API errors retain stable `ErrorCode`, message, retryability and HTTP status.

## Black-box evidence

The hermetic test starts a secured real RRD server, places a TCP fault proxy in
front of it, drops the first connection, and proves capability negotiation
recovers within the configured attempt bound. It then proves endpoint-catalogue
decoding, typed wrong-key error mapping, principal session creation, exact query
results, client-side expired-deadline denial, transaction begin/preview/abort,
retained changefeed decoding, durable WebSocket push/reconnect replay, protected
audit decoding, and cleartext remote endpoint denial. A second live fixture
proves valid mTLS HTTP and WebSocket negotiation, exact remote-listen capability
reporting, missing-client-certificate denial, and wrong-server-name denial.
The loopback client requires the `local_daemon` capability and the mTLS client
requires `remote`; both commit and query the exact checked-in deployment corpus
described in [`rrd-deployment-modes-v1.md`](rrd-deployment-modes-v1.md).

This is not the F5 exit gate. Commit and vector mutations, renewal/closure,
backup/restore, estate projection, response-limit
faults, server-version mismatches, and released package compatibility need
additional black-box rows. TypeScript, Python, Go, Java and .NET clients must
then pass the same semantic fixture.
