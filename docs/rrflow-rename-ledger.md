# RRFlow/RRD pre-release identity cutover

Status: authoritative V1 naming contract.

RRFlow means **Reason Ready Flow** and is the product. RRD means **Reason Ready
Daemon** and is RRFlow's single engine/runtime authority. The retired
pre-release identity is not a supported product, storage format, protocol, or
compatibility target.

This is an in-place V1 cutover, not a released-version migration. Git history
is the recovery boundary. Runtime compatibility code is not.

## Non-negotiable cutover rules

1. Every active Rust package, crate namespace, binary, module, CLI command,
   protocol label, MCP surface, SDK symbol, configuration path, environment
   variable, persisted marker, digest domain, fixture, benchmark artifact, and
   documentation path uses an RRFlow/RRD name.
2. No alias, forwarding crate, dual reader, deprecated command, old environment
   fallback, read-old/write-new path, or source allowlist preserves the retired
   identity.
3. The current format is V1. Checked-in V1 fixtures are regenerated against the
   canonical RRFlow/RRD bytes; no V2 is introduced merely to correct a
   pre-release name.
4. RRFlow owns product-facing surfaces. RRD owns the engine, daemon, protocol,
   storage/query/index internals, and their persisted identities.
5. Specialized storage, graph, vector, inference, and query crates are physical
   modules behind `rrd-engine`. They may not own a second catalogue,
   transaction truth, security authority, event log, or public product.
6. `rrflow-mcp`, `rrflow-cli`, SDKs, and Connectome consume authoritative RRD
   contracts; they do not duplicate registries or engine behavior.
7. The source gate is case-insensitive and has no exception for active files.
   Historical Git objects and the enclosing checkout directory are outside the
   build/runtime surface.
8. Capability completion is independent of naming completion. A renamed but
   incomplete capability remains explicitly incomplete in the gap ledger.

## Canonical names

| Responsibility | Canonical identity |
|---|---|
| Product and repository | RRFlow / Reason Ready Flow |
| Native engine and daemon | RRD / Reason Ready Daemon |
| Unified composition root | `rrd-engine` / `rrd_engine` |
| Public wire contract | `rrd-contract` / `rrd_contract` |
| Rust client | `rrd-client` / `rrd_client` |
| Server package and daemon | `rrd-server`; binary `rrd` |
| Core domain model | `rrd-core` / `rrd_core` |
| Native WAL/MVCC/LSM | `rrd-lsm` / `rrd_lsm` |
| Persistence port | `rrd-store` / `rrd_store` |
| Query parser/planner/executor | `rrd-query` / `rrd_query`; public language RRFlowQL |
| Graph | `rrd-graph` / `rrd_graph` |
| Vector and TurboQuant | `rrd-vector` / `rrd_vector` |
| Inference | `rrd-inference` / `rrd_inference` |
| Operator knowledge | `rrd-operator-knowledge` / `rrd_operator_knowledge` |
| Security, estate, cluster, Kubernetes | `rrd-security`, `rrd-estate`, `rrd-cluster`, `rrd-kubernetes` |
| Edge profile | `rrflow-edge` |
| User CLI | `rrflow-cli`; binary `rrflow` |
| MCP adapter | `rrflow-mcp`; tools prefixed `rrflow_` |
| Evaluation | `rrflow-eval` |
| Operator/developer client | `connectome-ui`; binary `connectome` |
| Project state | `.rrflow/` |
| Environment prefix | `RRFLOW_` |

The query executor has no separately branded MX layer. Data services have no
separately branded DS layer. Native persistence has no separately branded KV
product. Those are responsibilities inside RRD.

## Canonical internal V1 persisted identities

These bytes identify RRFlow's internal persisted-format V1 compatibility
domain. They are updated in place and frozen by fixtures and reopen/recovery
tests. `V1` here is not the RRFlow or Connectome product release number.

| Class | Internal V1 identity |
|---|---|
| WAL | `RRDWAL01`; record prefix `RRD1` |
| Batches | `RRDBAT01`, `RRDBAT02` |
| Segments | `RRDSEG01`, `RRDSEG02`, `RRDSEG03` |
| Index | `RRDIX003` |
| Snapshot bundle | `RRDSNP01` |
| Store archive/migration | `RRDMIG01` |
| Native sequence | `RRDNSI01` |
| Native keyspace | `RRDSK002` |
| Vector, HNSW, dense map, TurboQuant | `RRDVEC01`, `RRDHNS01`, `RRDDMAP1`, `RRDTQ001` |
| Digest and media domains | `rrflow-*`, `rrflow.*`, `rrflow/...` |

Any future format change must be motivated by a semantic format change, receive
an explicit version, and add cross-version evidence. This naming cutover does
not manufacture such a change.

## Cohesion boundary after the cutover

```text
RRFlow
├─ rrd-engine: one catalogue, transaction/read stamp, policy, event, query,
│  recovery, graph, vector, inference, and reasoning authority
├─ rrd-server: thin embedded/local/remote transport over rrd-engine
├─ rrflow-cli / rrflow-mcp / SDKs: contract consumers
└─ Connectome: RRD-backed operations and diagnostics client
```

The package rename did not by itself prove this boundary. G01-W02 subsequently
made it executable: the architecture suite rejects a public operator module,
`EmbeddedOperator`, `runtime_store`, outward physical-store openings, and
security/estate ownership of product executables. Remaining G03/G04/G05 work
expands behavior behind this authority; it does not authorize another engine.

## Executable acceptance gates

The cutover is complete only when all of the following pass on the same tree:

1. `cargo metadata --locked` lists only canonical RRFlow/RRD workspace packages.
2. A case-insensitive repository scan finds no retired identity in active
   source, manifests, workflows, SDKs, fixtures, evidence, or documentation.
3. The repository architecture test enforces that scan without an allowlist.
4. Frozen V1 persistence fixtures decode and reopen under the canonical bytes.
5. `cargo fmt --all -- --check` passes.
6. `cargo check --workspace --all-targets --locked` passes.
7. `cargo test --workspace --all-targets --locked` passes.
8. `cargo clippy --workspace --all-targets --locked -- -D warnings` passes.
9. Generated SDK and protocol fixtures agree with the RRD contract.
10. The implementation journal records exact commands, failures, fixes, and
    remaining architectural debt.

Remote platform CI is a separate verification gate after a push. Local success
must never be reported as Linux/macOS/Windows CI success.

## Current execution record

- Workspace package and crate namespaces: renamed to the canonical map above.
- Public language: RRFlowQL.
- Persisted markers and checked-in RRD LSM V1 fixtures: renamed and regenerated.
- Documentation/evidence file paths: renamed.
- Repository-wide no-retired-identity gate: implemented in
  `rrd-engine/tests/workspace_architecture.rs`; it scans every active tracked or
  untracked repository file case-insensitively, has no source allowlist, and
  also rejects workspace dependency rename aliases and package-directory/name
  mismatches.
- Runtime trace read links now serialize only the canonical `commit_cursor`;
  the retained compatibility field has been removed and is denied by test.
- The physical snapshot test proves the checked-in V1 bytes equal the current
  canonical export, installs and reinstalls those fixture bytes, continues the
  write sequence, and reopens the resulting database.
- Focused local evidence: all targets for `rrd-core`, `rrd-lsm`, and
  `rrd-engine` pass together after these corrections.
- Full local verification: G01-W01 is persisted as verified under digest
  `c6a9a19a057baad305e57a73b04680a7e9de5ee661ff0948b07146186af442a2`.
- Remote platform CI: source commit
  `38f1a19bcc9ad76389cceed7eb0988452e694235` passed push run
  `33075846902` and PR run `33075851923`. This documentary ledger change still
  requires clean publication and must not absorb concurrent G01-W02 files.
- Remaining engine cohesion and capability gaps: governed by
  [`full-stack-gap-ledger.md`](full-stack-gap-ledger.md), not concealed here.
