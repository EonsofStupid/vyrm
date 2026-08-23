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
transaction leases and terminal states, a claim-only public mutation type, and
commit receipts. These types remain independent of `vyrm_core`; later
multi-model transaction mutations extend the tagged public enum rather than
serializing private runtime structs.

The frozen JSON fixture is
[`public-contract-v1.json`](../crates/rrd-contract/fixtures/public-contract-v1.json).
Malformed identifiers, duplicate/unsorted capabilities, unsupported protocol
versions, unknown fields, repeated resource kinds, and mutations without an
idempotency key fail closed in tests.

This is not yet an HTTP or gRPC service and is not presented as an SDK. F2 will
bind transports to this contract; F5 will generate and test supported SDKs
against those same bytes.
