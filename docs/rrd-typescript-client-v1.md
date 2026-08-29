# RRD TypeScript client v1

Status: executable TypeScript/Node/browser-fetch walking skeleton over every
currently published RRD operation. Package publication, browser matrix, real-
server shared conformance, and TLS/distributed endpoints remain open.

`sdks/typescript` is generated from `rrd-contract`'s deterministic OpenAPI 3.1
document. `scripts/generate.ts` executes the contract exporter without a shell,
generates exact `paths` and `operations` types, and derives the runtime endpoint
map from `operationId`, method, path, auth, and mutation extensions. `pnpm
generate:check` fails when either artifact drifts.

The package deliberately uses two boundaries:

- OpenAPI-generated types provide compile-time request payload and successful
  response payload types for all 33 operations.
- ArkType validates the untrusted response envelope, protocol version,
  success/error discrimination, and bounded error shape at runtime. Endpoint
  payload validation is still server-authoritative; generated per-payload
  ArkType validators remain an F5 hardening row.

`RrdClient.call()` is keyed by the closed generated `OperationId`, so one method
covers the full operation catalogue without a handwritten route table.
Ergonomic methods cover capabilities, endpoint catalogue, OpenAPI, and session
creation. It constructs strict resource/context envelopes, requires
idempotency for mutations, validates response identity, caps streamed response
bytes, supports external cancellation and absolute/per-attempt deadlines, and
retries only public/read/idempotency-bound calls after transport failure. API
responses are never blindly retried. Cleartext is restricted to credential-free
loopback URLs until remote TLS is qualified.

Biome 2 is the sole formatter/linter. The pnpm lockfile pins ArkType, Biome,
OpenAPI TypeScript, TypeScript, and the test runner; pnpm's build allowlist
permits only the reviewed `esbuild` install script.

Current tests prove transport retry, ArkType envelope validation, generated
capability typing, API-key session construction, bearer query construction,
request/resource identity, remote-cleartext denial, expired-deadline denial,
and typed permission errors. Shared real-server fixtures and released-version
compatibility remain required before the TypeScript SDK is release-qualified.
