# RRD deployment modes v1

RRD is one logical engine with multiple composition and transport faces. A
deployment mode may change ownership, durability, transport, and resource
limits; it may not introduce a second transaction coordinator, query model,
authorization model, or ordering authority.

The public `DeploymentMode` vocabulary is closed for protocol v1:

| Mode | Composition boundary | Durability and ownership | Qualification in G05-W01 |
|---|---|---|---|
| `memory` | In-process `RrdEngine` over memory storage and objects | Process lifetime; durability-only operator actions fail explicitly | Full session, transaction, schema, record, and RRFlowQL corpus |
| `embedded` | In-process `RrdEngine` over one native root | One process owns the root writer lock; reopen preserves the runtime cursor | Same logical corpus plus second-writer denial and reopen |
| `local_daemon` | Standalone `rrd-server` on loopback | The daemon exclusively owns the native root | Real child process, readiness, Rust client query, graceful shutdown, and reopen |
| `edge` | Offline mmap index opened by `rrflow-edge` | Read-only artifact bounded by its source cursor and checksum | Same corpus's deterministic retrieval result without network access |
| `remote` | RRD protocol over TLS 1.3 mutual authentication | Server owns persistence; the client owns no local storage authority | Rust client query over the mTLS service face and explicit remote capability |
| `distributed` | Future cluster placement of the same engine contract | Consensus, replication, failover, and placement evidence required | Not qualified by G05-W01; remains G08/G10 work |

Browser/WASM and mobile packaging are not protocol modes in v1 and are not
claimed by this gate. They may consume the edge or remote contract only after
their own platform resource and security qualification.

## Shared logical corpus

[`rrd-deployment-conformance-v1.json`](../fixtures/rrd-deployment-conformance-v1.json)
is the single checked-in corpus. Its strict format is represented by
`DeploymentConformanceCorpus` in `rrd-contract`; unknown fields, unsupported
format versions, empty documents, duplicate identifiers, invalid bounds, and
an expected identifier absent from the corpus fail closed.

Memory and embedded compositions commit its schema and documents through the
normal session and data-transaction authority and execute its exact RRFlowQL
query. The local daemon and remote mTLS tests seed and query those same bytes
through `rrd-client`. The edge test builds and mmap-opens the offline artifact
from the same documents and evaluates the same retrieval expectation. Edge
therefore proves bounded retrieval compatibility, not mutation, session, or
durability equivalence that its offline face does not offer.

## Cohesion invariants

- `RrdEngine::memory` and `RrdEngine::open` share the same sessions,
  transactions, query, policy, audit, and runtime-commit paths. Only the
  storage and immutable-object ports vary.
- The global runtime cursor and engine control journal remain the logical
  ordering authorities. HTTP, WebSocket, SDK, daemon, and edge adapters do not
  create another engine.
- A native root has one writer. The standalone-process test proves lock denial
  while the daemon runs and clean acquisition only after bounded shutdown.
- Memory mode never fabricates durability. Backup, restore, physical-root
  inspection, and project-binding operations return an explicit non-durable
  error before preparing operator state.
- Service capabilities report the deployed face (`local_daemon` or `remote`),
  while in-process engines report `memory` or `embedded`.
- Distributed placement remains unavailable until its consensus and
  independent-host evidence passes. The enum value is vocabulary, not a claim
  that the mode is operational.

## Executable evidence

- `crates/rrd-engine/src/engine/tests/deployment_conformance.rs`
- `crates/rrd-server/tests/http_process.rs`
- `crates/rrd-client/tests/real_server.rs`
- `crates/rrflow-edge/tests/offline.rs`
- `crates/rrd-contract/tests/public_contract.rs`

