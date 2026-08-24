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

The first vector-read contract supports bounded dense, sparse, and
multi-dense/MaxSim queries across cosine, dot, Euclidean, and Manhattan
metrics. Its result carries the authoritative read manifest/cursor, scan count,
plan digest, selected access path and exactness alongside typed scored hits.

The frozen JSON fixture is
[`public-contract-v1.json`](../crates/rrd-contract/fixtures/public-contract-v1.json).
Malformed identifiers, duplicate/unsorted capabilities, unsupported protocol
versions, unknown fields, repeated resource kinds, and mutations without an
idempotency key fail closed in tests.

`rrd-server` binds the current contract to a loopback-only HTTP alpha. It is
not presented as an SDK or remote-authenticated service. F5 will generate and
test supported SDKs against these same bytes, and F4 must land before remote
exposure.
