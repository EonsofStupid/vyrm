# RRD public contract v1

`rrd-contract` is the first F0 boundary. It is transport-neutral and does not
depend on Vyrm storage, query, cluster, node, or Connectome crates.

Version 1 freezes:

- the `rrd` protocol identity and exact protocol-version negotiation;
- canonical URL/path-safe resource identities for organisation, estate,
  project, instance, node, shard, collection, table, record, transaction,
  snapshot, backup, and operation resources;
- explicit hierarchical resource paths with no cwd/session inference;
- request, operation, idempotency, and deadline coordinates;
- a mutation rule requiring an idempotency key;
- content binding of an idempotency key to a lowercase SHA-256 operation
  digest;
- deployment modes and sorted, versioned capability negotiation;
- success/error response framing and stable error codes.

F2 extends version 1 additively with bounded transport-session leases,
transaction leases and terminal states, typed multi-model mutations and commit
receipts, and the first read-only query contract. The mutation vocabulary
covers claims, schema registries, records/documents, graph relations, events,
dense/sparse/multi-dense vectors with embedding provenance, time-series
samples, WGS84 geo values, and pre-staged immutable object references.
`ExecuteQuery` defines strict query, parameter, scan, row, batch, and
encoded-output bounds; its typed result carries the canonical query, read
coordinates, plan evidence, execution counters, and rows. These types remain
independent of `vyrm_core`; the server performs an explicit lowering into the
authoritative runtime rather than serializing private runtime structs.

The vector boundary now includes journaled collection administration. A named
vector definition freezes its stored field, dense/sparse/multi-dense kind,
dimensions, metric, optional embedding-model digest, and requested
pinned/cached/cold placement. Ensure is mutation-idempotent; list exposes the
authoritative catalogue revision and stable collection generation. Search may
use the original field/metric address for compatibility or a collection plus
vector name; the latter resolves the persisted contract and rejects kind or
dimension drift before executing.

The first vector-read contract supports bounded dense, sparse, and
multi-dense/MaxSim queries across cosine, dot, Euclidean, and Manhattan
metrics. Its result carries the authoritative read manifest/cursor, scan count,
plan digest, selected access path and exactness alongside typed scored hits.

The retained changefeed contract is cursor-addressed and page bounded. It
freezes lifecycle coordinates and the runtime digest chain, preserves complete
claim provenance, and reuses the typed public multi-model vocabulary for every
non-claim mutation. `through_cursor`, rather than the final matching change,
is the sole resume coordinate so scoped feeds cannot stall on unrelated global
activity.

The bounded follow contract wraps that exact read coordinate with a maximum
five-second wait. Its result distinguishes a timeout from delivered data and
always returns a normal replay page, keeping disconnect/reconnect behavior
independent of server-local subscription state.

Managed backup/restore is path-closed at the public boundary. A create request
contains only a bounded safe label and event time; a restore request contains a
content-addressed backup digest, canonical restore identity, and event time.
Catalogue responses expose authenticated archive identity, watermarks and an
explicit coverage matrix. No request accepts a source, catalogue, target, or
active-instance filesystem path. The server owns generated locations and a
restore always targets a new root.

The F4 audit vocabulary freezes a closed security-action enum, authorization
and completion phases, allow/deny/fail decisions, and a bounded cursor-addressed
read. Public records contain principal/action/resource and request/operation
coordinates plus request/response SHA-256 values; bodies, credentials, bearer
tokens, and arbitrary headers are excluded. `through_sequence` is the scanned
authenticated control-journal coordinate, so readers advance safely across
unrelated control transitions.

F5 begins from `EndpointCatalogue`, not handwritten per-language route lists.
It currently freezes 26 sorted operation identities with HTTP method/path template,
authentication mode, mutation/idempotency classification, security action, and
public request/response type names. `GET /v1/schema/endpoints` serves the exact
catalogue used by the server. Duplicate operations/routes, GET mutations,
private Rust paths, invalid names, and ordering drift fail contract tests.
The first consumer is `rrd-client`; it imports these public types directly and
has no dependency on storage, query, server, estate, or security implementation
crates in its release dependency graph.

Every public wire type also derives JSON Schema. `openapi_document()` projects
those schemas and the endpoint catalogue into deterministic OpenAPI 3.1, and
`rrd-contract-export` writes the same document to standard output for package
generation. Its canonical pretty-JSON SHA-256 is frozen in the contract tests;
intentional schema drift therefore requires an explicit version review. A
running RRD serves the document from `GET /v1/schema/openapi`.
Self-recursive `QueryValue` is promoted to one canonical OpenAPI component;
other definitions are inlined, so references resolve under ordinary OpenAPI
generators rather than relying on schema-local `$defs` resolution.

The frozen JSON fixture is
[`public-contract-v1.json`](../crates/rrd-contract/fixtures/public-contract-v1.json).
Malformed identifiers, duplicate/unsorted capabilities, unsupported protocol
versions, unknown fields, repeated resource kinds, and mutations without an
idempotency key fail closed in tests.

`rrd-server` binds the current contract to a loopback-only HTTP alpha. It is
not presented as an SDK or remote-authenticated service. F5 will generate and
test supported SDKs against these same bytes, and F4 must land before remote
exposure.
