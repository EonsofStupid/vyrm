# Vyrm to RRFlow/RRD migration ledger

Status: authoritative migration control document. Pre-release compatibility is
retained only where executable recovery or client evidence requires it.

The target architecture is defined in
[`rrflow-rrd-architecture.md`](rrflow-rrd-architecture.md). This ledger prevents
a blind rename from corrupting persisted data, changing digests, breaking SDKs,
or preserving the old architecture under new labels.

## Migration rules

1. Migrate dependency leaves before composition roots.
2. Keep each slice buildable and independently reversible through Git.
3. Never change a durable identifier without a versioned reader and fixture.
4. Do not add a facade that creates a second catalogue, transaction authority,
   or storage truth.
5. Compatibility code is narrow, named, tested, and assigned a removal gate.
6. Generated SDK output changes only after its authoritative protocol source.
7. Update status only from executable evidence; a new name is not a completed
   capability.
8. Do not begin the next slice while required remote CI for the previous pushed
   slice is red.

## Target name map

### Product and user-facing surfaces

| Legacy | Target | Disposition |
|---|---|---|
| Vyrm product/repository identity | RRFlow | Replace in current product documentation and repository metadata |
| `vyrm` CLI | `rrflow` | Direct rename with command-level conformance; no permanent alias before stable release unless an installed workflow proves it is needed |
| `vyrmd` | RRD composition moves to `rrflow-daemon`; cooperative MCP becomes an RRFlow adapter | Split by responsibility, not by copying persistence |
| VyrmQL | RRFlowQL | Change parser diagnostics, query traces, protocol descriptions, fixtures, and SDK generation together |
| VyrmMX | RRFlow query planner/executor | Remove the public acronym; retain planner/executor modules behind one query contract |
| `.vyrm/instance.toml` | `.rrflow/instance.toml` | Read-old/write-new migration with conflict denial when both differ |
| `.vyrm/store` | `.rrflow/rrd` | Explicit validated move/import; never reinterpret or silently initialize over an old root |
| `.vyrm/workflows.toml` | `.rrflow/workflows.toml` | Parse old then write canonical new; deny ambiguous dual configuration |
| `VYRM_*` environment variables | `RRFLOW_*` | Per-variable deprecation window only if external CI evidence requires it |
| `vyrm_*` MCP tools | `rrflow_*` | Protocol-versioned tool catalogue migration; old catalogue cannot claim enforcement |
| `vyrm:` generated actor/label prefix | `rrflow:` | Change only where it is presentation data; version domain-separated identities |

The existing RRD protocol identity (`"rrd"`), `X-RRD-*` headers, and `RRD.*`
daemon-local files already name the Reason Ready Daemon and are not Vyrm debt.
SDK product/package descriptions should say RRFlow while their service client
may remain an `RrdClient` where that clearly denotes the internal daemon.

### Rust package destination

| Current package | Target package/module | Required migration evidence |
|---|---|---|
| `vyrm-core` | `rrflow-core` | Golden domain types, key ordering, temporal resolution, runtime/event digests |
| `vyrm-kv` | `rrflow-storage-lsm` | WAL/segment/manifest reopen, crash matrix, snapshots, compaction, platform I/O |
| `vyrm-store` | `rrflow-storage` | Memory/native/legacy differential, archive/restore, migrations, atomic runtime commits |
| `vyrm-ql` + `vyrm-mx` | one public `rrflow-query` contract | Parser golden corpus, plan digest, exact read-stamp results, old/new executor differential |
| `vyrm-graph` | `rrflow-graph` | Persisted projection and freshness/routing fixtures |
| `vyrm-vector` | `rrflow-vector` | Artifact readers, exact oracle, ANN recall, filter, compression, lifecycle |
| `vyrm-embed` | `rrflow-inference` | Model/source provenance, deny-network boundary, deterministic fixtures |
| `vyrm-edge` | RRFlow edge deployment profile | Offline reopen/search and resource-envelope tests |
| `vyrm-operator` | RRFlow operator-knowledge module | External snapshot and pgvector binding/provenance fixtures |
| `vyrm-cluster` | `rrflow-cluster` | Command/snapshot compatibility, transport, membership and failure simulation |
| `vyrm-node` | `rrflow-runtime` | Neutral lifecycle, attunement, preflight, authorization and adapter conformance |
| `vyrm-cli` | `rrflow-cli` | Exact CLI behavior, path/config migration and cross-platform argv tests |
| `vyrmd` | RRFlow MCP adapter plus `rrflow-daemon` composition | MCP catalogue migration and proof that cooperative mode is reported honestly |
| `vyrm-eval` | `rrflow-eval` | Retained scenario/result schema readers and reproducibility metadata |
| `rrd-contract` | `rrflow-protocol` | Byte-identical v1 fixture or explicit protocol revision and regenerated SDKs |
| `rrd-server` | `rrflow-daemon` with binary `rrd` | All real-process, auth/session/transaction/restart tests |
| `rrd-client` | `rrflow-client` | Shared black-box daemon conformance |
| `rrd-security` | `rrflow-security` | Policy/audit persistence and deny-by-default tests |
| `rrd-estate` | `rrflow-control-plane` | Reconciliation, leases, backup/deployment jobs and restart tests |
| `rrd-kubernetes` | `rrflow-kubernetes` | CRD compatibility and real-cluster qualification |
| `connectome-ui` | `connectome` | Desktop packaging and authoritative RRD source tests |

`rrd-maintenance` is concurrent unfinished work and is outside the active
migration until that work is integrated or deliberately removed. The migration
must not absorb or rewrite it opportunistically.

## Durable identifier inventory

The following known classes cannot be changed mechanically. The inventory must
be regenerated before every storage-affecting slice.

| Class | Current examples | Target handling |
|---|---|---|
| Native keyspace tag | `VYRSK002` | Keep v2 reader; new version uses a reviewed RRFlow/RRD tag only with cross-version reopen |
| Native sequence value | `VYRNSI01` | Same rule as keyspace tag |
| Store migration archive | `VYRMIG01` | Preserve reader; new archive version identifies RRFlow and round-trips through old fixture |
| Logical archive | `RRDLAR01` | Already aligned with RRD; retain unless archive schema changes |
| Vector artifacts | `VYRVEC01`, `VYRHNS01`, `VYRDMAP1`, `VYRTQ001` | Dispatch by magic/version; add new writers only after old readers and rebuild/import policy exist |
| Digest domains | `vyrm-*`, `vyrm.*`, and `vyrm/...` byte prefixes | Never replace in place; changing a domain creates a new versioned identity and requires correspondence fixtures |
| Raft keys/media types | `vyrm/raft/v4/*`, `application/vnd.vyrm.*` | Versioned snapshot/state migration before cluster rename |
| Object-store keys/media | Vyrm-prefixed object identities and content types | Inventory capability by capability; retain read compatibility until all referenced manifests migrate |
| Projection IDs | `routing-index-v1`, `current`, vector catalogue IDs | Rename only with rebuild marker or dual-reader plus exact source-cursor proof |
| Runtime trace identities | Vyrm-prefixed trace/span domain separators | Preserve old trace identity on replay; emit a new event/schema version for RRFlow identities |
| Benchmark/evaluation schemas | `vyrm.*-evidence.v1` | Historical evidence remains immutable; new runs use new schema names and never rewrite old artifacts |

Known path and configuration debt includes `.vyrm/instance.toml`,
`.vyrm/store`, `.vyrm/workflows.toml`, Claude instruction markers, package-runner
commands, `VYRM_TRACE`, `VYRM_TRACE_FORMAT`, `VYRM_UPDATE_GOLDENS`, and
pgvector test/deployment variables and schemas. Each variable receives an
explicit row before its owning slice changes.

## Dependency order

The present dependency graph starts with `vyrm-core`, fans through storage,
query, graph, vector and cluster, then converges in `vyrm-node`, `rrd-server`,
the CLI, SDK tests, and Connectome. The migration order is therefore:

```text
0. authority documents and generated inventory
1. rrflow-core
2. rrflow-storage-lsm
3. rrflow-storage
4. rrflow-query foundation and reference-executor boundary
5. graph, vector, inference and operator-knowledge physical modules
6. rrflow-runtime provider-neutral lifecycle
7. rrflow-security and rrflow-protocol ownership cleanup
8. rrflow-daemon composition root and `rrd` binary
9. rrflow-client, generated SDKs and rrflow CLI
10. control plane, cluster, Kubernetes and Connectome
11. paths, configuration and remaining public-surface cutover
12. compatibility-reader removal and stable-release naming gate
```

The Arrow/DataFusion work begins at step 4 after the storage snapshot/read-stamp
port is available under its target identity. It must not be implemented inside
the HTTP server or Connectome.

## Slice ledger

| Slice | State | Scope | Exit evidence |
|---|---|---|---|
| N0 | complete | Freeze product architecture, target names, durable-identifier classes and dependency order | Documentation consistency checks; no code claim |
| N1 | pending | Rename/refactor `vyrm-core` to `rrflow-core` without semantic changes | Core golden/property tests and all direct dependants compile |
| N2 | pending | Migrate native KV package to `rrflow-storage-lsm` while retaining legacy on-disk readers | WAL/manifest/segment/snapshot/crash/reopen matrix |
| N3 | pending | Migrate storage facade and make RRD catalogue/transaction/snapshot ports explicit | Memory/native/Fjall compatibility differential plus archive/recovery |
| N4 | pending | Establish `rrflow-query`; preserve current executor as oracle; introduce Arrow schemas and snapshot-bound providers | Exact parser/plan/result differential and bounded RecordBatch streaming |
| N5 | pending | Integrate DataFusion physical planning/execution with RRFlow extension nodes | Pushdown, cancellation, memory/spill budgets, stale-index and restart tests |
| N6 | pending | Migrate graph/vector/inference/operator knowledge behind shared catalogue and read stamp | Cross-model atomicity/freshness and artifact compatibility |
| N7 | pending | Build `rrflow-runtime` neutral lifecycle and bind it to shared RRD authority | Adapter-neutral fixtures, fail-closed mutation, one-shot authorization |
| N8 | pending | Migrate protocol/security/daemon composition and SDK authority | Real-daemon conformance across supported clients |
| N9 | pending | Migrate CLI/config/project paths and Connectome | Read-old/write-new fixtures, conflict denial, desktop/platform matrix |
| N10 | pending | Remove obsolete shims and enforce naming gate | No unapproved Vyrm names outside legacy readers, immutable evidence and history |

### N0 evidence — 2026-08-24

- Commit: `c8df581` (`docs: define cohesive RRFlow RRD migration`).
- Old names changed: current product headings and boundary statements in the
  README, specification, plan, status, Clyffy handoff, instance topology, and
  runtime foundation documents.
- New names introduced: RRFlow product, RRD Reason Ready Daemon, RRFlowQL,
  target RRFlow package map, and dependency-ordered N1–N10 migration slices.
- Public contract impact: documentation only; the RRD v1 protocol and generated
  SDK bytes were not changed.
- Persisted-format impact: none. The ledger records known magic bytes, digest
  domains, paths, environment variables, projection IDs, and artifact formats
  as migration inputs instead of rewriting them.
- Local evidence: `git diff --check` and relative Markdown-link validation
  passed.
- Remote evidence: [push CI 32750794610](https://github.com/EonsofStupid/vyrm/actions/runs/32750794610)
  and [pull-request CI 32750798239](https://github.com/EonsofStupid/vyrm/actions/runs/32750798239)
  passed the full workspace verification job and Linux, macOS, and Windows
  estate/process matrix.
- Remaining documentation debt: the concurrently modified
  `full-stack-gap-ledger.md` retains an older Vyrm-under-RRD diagram. The target
  architecture explicitly supersedes it; reconcile it after the concurrent
  change lands rather than staging unrelated work.
- Next dependency gate: N1, the semantic-neutral `vyrm-core` to `rrflow-core`
  migration.

## First executable slice: N1

N1 is deliberately semantic-neutral. It must:

1. snapshot the current `vyrm-core` public API, golden fixtures, package
   dependants, doctests, and digest-domain inventory;
2. rename the package/library to `rrflow-core` and move its directory without
   changing serialized fields, key bytes, digest domains, or behavior;
3. update every direct Cargo dependency and Rust import in one atomic commit;
4. keep legacy bytes named as legacy in code comments and the durable inventory
   rather than cosmetically relabeling them;
5. run formatting, core tests, every direct-dependant check, workspace tests,
   and Clippy with warnings denied; and
6. push and watch every required platform job before N2 begins.

N1 does not add Arrow/DataFusion, redesign core types, split oversized modules,
or alter the public RRD protocol. Combining those changes would make failures
and compatibility regressions impossible to attribute.

## Per-slice evidence template

Every completed row records:

```text
commit:
old names changed:
new names introduced:
public contract impact:
persisted-format impact:
compatibility reader/shim:
tests run locally:
remote jobs and URLs:
failure/recovery evidence:
remaining debt:
next dependency gate:
```

## Naming release gate

Before the first stable RRFlow release, CI must reject new case-insensitive
`vyrm` occurrences except an allowlist containing only:

- versioned legacy readers and their fixtures;
- immutable historical benchmark/evaluation artifacts;
- migration documentation that names the source format; and
- Git history references that cannot affect a build or user-facing surface.

The allowlist records file, exact reason, owning slice, and removal condition.
An unrestricted repository-wide text exception is not acceptable.
