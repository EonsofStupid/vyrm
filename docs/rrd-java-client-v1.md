# RRD Java client v1

Status: executable synchronous Java 21 walking skeleton over every currently
published RRD operation. Async/caller cancellation, generated per-payload
models, shared real-server conformance, released-version testing, and Maven
Central publication remain open.

`sdks/java` generates a closed `OperationId` enum from `rrd-contract`'s
deterministic OpenAPI 3.1 document. The shell-free generator records method,
path, authentication, and mutation classification for all 21 routes and
supports checked-in drift detection.

The client uses Java's standard HTTP client and Jackson 3.2 for bounded JSON
transport. Generic `call` covers the entire operation enum; helpers provide
capability negotiation, endpoint/OpenAPI discovery, and session creation. It
disables redirects, restricts cleartext to credential-free loopback addresses
without resolving arbitrary hostnames, validates identifiers/resources,
requires mutation idempotency, applies per-attempt and absolute deadlines,
caps response bytes, correlates response identity, and retries only reads or
idempotency-bound operations after I/O failure. The response parser rejects
extra envelope/outcome/error fields and invalid success/error discrimination.

Maven compiles on Java 21 with all lint warnings promoted to errors. JUnit 6
real-loopback tests cover dropped-connection retry, capability identity,
API-key session creation, bearer query envelopes, resource identity,
remote-cleartext and expired-deadline denial, typed permission errors, and all
21 generated operations. The generator drift check and packaged JAR also pass.

This is not the F5 exit gate. Async transport/caller cancellation, generated
request/result models, dependency locking/verification, shared real-server
fixtures, supported-version matrices, and publication remain open.
