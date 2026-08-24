# RRD Python client v1

Status: executable synchronous Python walking skeleton over every currently
published RRD operation. Async support, generated per-payload models, shared
real-server conformance, released-version testing, and package publication
remain open.

`sdks/python` derives its closed `OperationId` and endpoint map from
`rrd-contract`'s deterministic OpenAPI 3.1 document. The generator executes the
Rust contract exporter without a shell and records operation ID, method, path,
authentication, and mutation classification for all 21 routes. A drift check
fails when the checked-in projection differs from the contract.

`RrdClient.call()` exposes the entire generated operation catalogue while
ergonomic helpers cover capability negotiation, endpoint/OpenAPI discovery,
and session creation. The client constructs versioned context/resource
envelopes, requires idempotency keys for mutations, applies API-key or bearer
authentication, rejects credential-bearing or non-loopback cleartext URLs,
caps streamed responses, validates response correlation, honours absolute and
per-attempt deadlines, and retries only reads or idempotency-bound calls after
transport failure. Pydantic validates the untrusted response envelope and
typed error discrimination before a payload is returned.

The package is locked with uv and gates source with Ruff, strict mypy, pytest,
generation drift, and wheel/source-distribution builds. Mock-transport tests
cover retry and negotiation, API-key session creation, bearer query execution,
resource and response identity, remote-cleartext denial, deadline denial,
typed permission errors, and complete operation-catalogue generation.

This is not the F5 exit gate. The Python client remains a synchronous generic
payload skeleton until generated request/result models, async transport,
shared real-server fixtures, packaging, and supported-version matrices pass.
