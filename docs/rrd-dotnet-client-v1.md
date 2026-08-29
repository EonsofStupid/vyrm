# RRD .NET client v1

Status: executable asynchronous .NET 10 walking skeleton over every currently
published RRD operation. Generated per-payload models, shared real-server
conformance, released-version testing, and NuGet publication remain open.

`sdks/dotnet` generates a closed `OperationId` enum and endpoint switch from
`rrd-contract`'s deterministic OpenAPI 3.1 document. The shell-free generator
records method, path, authentication, and mutation classification for all 24
routes and supplies exact checked-in drift detection.

The client uses `HttpClient` and `System.Text.Json`, with no third-party runtime
dependency. `CallAsync` covers the complete operation enum; helpers provide
capability negotiation, endpoint/OpenAPI discovery, and session creation. It
accepts caller cancellation, combines per-attempt and absolute deadlines,
disables redirects for its owned transport, restricts cleartext to credential-
free loopback addresses without DNS lookup, validates identifiers/resources,
requires mutation idempotency, caps streamed response bytes, correlates
response identity, and retries only reads or idempotency-bound operations after
transport failure. Closed-field checks validate response envelope, outcome,
and error discrimination before returning a cloned JSON object.

.NET 10 builds the solution with nullable analysis and warnings-as-errors.
`dotnet format --verify-no-changes`, locked restore, three xUnit v3 tests, and
NuGet packing pass. The tests cover transport retry and capability identity,
API-key session creation, bearer query envelopes, resource identity, remote-
cleartext and expired-deadline denial, typed permission errors, cancellation-
aware test execution, and all 33 generated operations.

This is not the F5 exit gate. Generated request/result models, shared real-
server fixtures, supported-version matrices, examples/API reference, and NuGet
release qualification remain open.
