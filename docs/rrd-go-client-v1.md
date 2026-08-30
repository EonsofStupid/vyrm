# RRD Go client v1

Status: executable synchronous/context-aware Go walking skeleton over every
currently published RRD operation. Generated per-payload models, shared
real-server conformance, released-version testing, and module publication
remain open.

`sdks/go` generates a closed `OperationID` constant set and endpoint map from
`rrd-contract`'s deterministic OpenAPI 3.1 document. The generator invokes the
Rust exporter directly without a shell, extracts method, path, authentication,
and mutation classification for all 34 operations, formats output with Go's
standard formatter, and supports an exact drift check.

The client intentionally has no third-party runtime dependencies. Its generic
`Call` covers the complete generated catalogue, while helpers provide
capability negotiation, endpoint/OpenAPI discovery, and session creation. It
accepts `context.Context` for caller cancellation, combines per-attempt and
absolute deadlines, disables redirects, restricts cleartext to credential-free
loopback URLs, validates identifiers and resource paths, requires idempotency
for mutations, bounds response reads, correlates response identities, and
retries only reads or idempotency-bound operations after transport failure.
Strict JSON decoding validates the typed success/error envelope before payload
return, and credentials are excluded from errors.

The current gate runs generation drift, `gofmt`, `go vet`, ordinary tests, and
the race detector. Tests cover retry and capability identity, API-key session
creation, bearer query construction, resource envelopes, remote-cleartext and
expired-deadline denial, typed permission errors, and all 34 generated routes.

This is not the F5 exit gate. Complete generated request/result types, shared
real-server fixtures, supported-version matrices, examples/API reference, and
module release qualification remain open.
