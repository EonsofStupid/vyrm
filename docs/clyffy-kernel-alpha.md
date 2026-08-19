# Vyrm alpha → Clyffy kernel handoff

Status: execution contract, 2026-08-19. This document separates what must be
proved in Vyrm from what belongs in the later, clean Clyffy product repository.
It is intentionally release-gated rather than calendar-gated.

## Product boundary

The canonical chain remains **Automaton → LFG → Connectome**:

- Automaton brokers provider sessions and subscription-backed AI surfaces.
- LFG parses and composes just-in-time context.
- Connectome records, explains, visualizes, and enforces runtime intelligence.
- Vyrm is Connectome's embeddable kernel: contracts, persistence, query and
  execution, projections, lifecycle policy, and evidence.
- Clyffy will package those capabilities into installable per-platform and
  umbrella deployments. It consumes versioned Vyrm interfaces; it does not
  fork Vyrm internals into a second implementation.

The Clyffy repository should not be created by pooling old repositories. It
starts from a release manifest that pins reviewed versions of Automaton, LFG,
Connectome/Vyrm, provider adapters, schemas, migrations, and benchmark evidence.

## Canonical lifecycle event model

Every enforceable workflow event uses a stable identity:

```text
<domain>:<producer>:<action>[:<target>]
```

The first implemented family is package execution:

```text
package:bun:test
package:pnpm:run:typecheck
package:npm:run:test-unit
package:yarn:run:build
```

Script identity is preserved so unrelated runs cannot supersede one another.
The current hook records these as post-tool evidence. The enforcement sequence
is deliberately staged:

1. A project-owned workflow manifest declares event, command matcher, required
   scope, required projections, freshness bounds, and verification policy.
2. Session start captures an instance-bound `ReadStamp` and injects only the
   allowed context budget.
3. Pre-tool policy resolves the declared event and denies on missing identity,
   stale source/projection generations, absent authorization, or an unresolved
   prior mutation.
4. Post-tool handling commits the observable command/result envelope and audit
   event atomically, then schedules rebuildable projections.
5. Stop/compact gates require the reasoning-run contract to reach a verified
   outcome or preserve an explicit incomplete state for the next session.

No package-manager hook may infer business meaning from a script name. The
project manifest supplies that meaning and is versioned evidence itself.

## Alpha release gates

Vyrm becomes a firm local alpha only when all of these are machine-verifiable:

| Gate | Required evidence |
|---|---|
| Portable contract | Versioned golden JSON for read, transaction, plan, projection, audit, workflow event, and error envelopes |
| Transaction semantics | Reference/compatibility/native differential, conflict matrix, read-your-writes, repeatable paging, crash/restart, lease and retention-pin tests |
| Query/runtime | `vyrmQL` parser corpus and fuzzing; typed-SDK equivalence; deterministic `vyrmMX` plans and budgets |
| Native storage | WAL/MVCC/manifest crash matrix, compaction with pinned snapshots, corruption handling, recovery idempotency, and no acknowledged-write loss |
| Unified data | Atomic record/edge/claim/event/vector/series/geo/blob-reference mutations plus local/S3 object differential |
| Search | Exact dense/sparse/multivector oracle; ANN recall/latency/memory matrix; filtered update/delete/reopen/compaction soak |
| Lifecycle | Claude hooks and MCP/daemon runtimes produce the same decisions and audit fields; package workflow policies deny stale or unverified mutations |
| Workbench | Freeze/rewind/forward renders only persisted events and links every value to cursor, snapshot, manifest, plan, and projection generation |
| Release | Reproducible artifacts, signed update manifest, forward/rollback migration rehearsal, SBOM/provenance, compatibility matrix, and benchmark regression budgets |

Cluster/Multi-AZ is a separate deployment gate. A local alpha must not claim
distributed durability merely because its interfaces reserve shard fields.

## Deployment tiers for the future Clyffy repository

| Tier | Shape | Update behavior |
|---|---|---|
| Developer | One embedded instance per major checkout; explicit umbrella membership for small related projects | Opt-in stable/beta channel, signed manifest, local migration backup and health rollback |
| Workstation | `vyrmd` owns multiple isolated instances and provider adapters | Staged daemon restart, schema compatibility check, per-instance rollback |
| Team | Authenticated service with explicit tenant/shard placement and object storage | Rolling update only after mixed-version simulation and migration fencing |
| Edge | Offline, resource-capped exact/ANN search with no required network | Side-loaded signed bundle and atomic slot switch |

Provider adapters use official local/API authentication and expose capability,
effort, quota, and observability metadata. Clyffy may combine subscription AI
services at the orchestration layer, but it must not pool credentials, pretend
provider-specific effort levels are equivalent, or label inferred hidden
reasoning as observed data.

## Competitive proof, not a blanket claim

“Beats SurrealDB” and “beats Qdrant” are two separate benchmark hypotheses.
They become publishable only after the corresponding Vyrm subsystem exists.

- Against SurrealDB: fixed hardware and durability; transactional mixed
  record/edge/time queries; conflict rate; p50/p95/p99 latency; throughput;
  recovery time; write amplification; RSS/disk; and correctness oracle.
- Against Qdrant: exact oracle first; dense/sparse/multivector and filtered ANN;
  recall@k/NDCG; p50/p95/p99 latency; build/update/delete/reopen/compaction;
  CPU/GPU build cost; RSS/VRAM/disk; and stale-generation behavior.
- Vyrm-specific frontier-runtime value is measured separately: task success,
  stale-action denials, retries/regressions, provider/context/reasoning tokens,
  latency, compaction recovery, and trace completeness.

Results must publish dataset/version, query generation, warmup, concurrency,
hardware/software, durability, configuration, raw samples, confidence interval,
and failed runs. Until then the repository may claim implemented semantics and
measured local results, never universal superiority.

## Immediate execution order

1. **Complete:** exact `vyrmQL`/`vyrmMX` over the frozen M0/M1 port and live
   snapshot/retention-pin inspection in Connectome.
2. **Complete:** atomic hash-chained audit/runtime commits and deny-by-default
   reasoning/lifecycle differentials.
3. **Local gate passed:** native `vyrmKV` behind the same semantic, query,
   crash, storage-full, compaction, and benchmark harness. Reproduce its strict
   Fjall baseline in CI and broader workloads before removing the oracle.
4. Add unified object/vector mutations, then exact and ANN search.
5. Bind declared package workflows to preflight/pre-tool/post-tool policy.
6. Extend truthful temporal visualization across storage, search, and workflow
   events and continue controlled provider evaluations.
7. Cut the Vyrm alpha manifest; only then scaffold the separate Clyffy master
   repository and its signed tier/update system.
