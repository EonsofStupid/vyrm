# Instance topology

| Layer | Meaning |
|---|---|
| Product | vyrm/connectome: the reusable runtime pattern and shared kernel |
| Instance | One deployed, isolated runtime molded to a platform or an explicit umbrella |
| Member | A project admitted to an umbrella instance |
| Upstream | A capability source such as SurrealDB or Qdrant; never an automatic merge target |

## Deployment rule

A major platform gets a dedicated instance. Its claims, reasoning ledger,
routing projection, policy evidence, and runtime configuration remain local to
that platform.

The deployable Raft node config now carries a required `project_scope`. Its
typed supervisor `runtime_commit` command denies any commit for another scope,
and snapshot artifact derivation uses the same scope before authenticated
transfer. This makes the per-project boundary executable at the storage-node
edge rather than relying only on directory convention.

A set of genuinely related small projects may use one umbrella instance. The
umbrella has an explicit member list; filesystem proximity is not membership.
An unlisted project is denied rather than silently included.

There is no estate-wide default instance. New work starts by creating an
instance for the target platform and molding adapters, policy, retrieval, and
projections to that platform while retaining the shared runtime invariants.

## Invariants shared by every instance

1. Recall is injected before reasoning.
2. Mutation requires a valid typed reasoning transition and fresh evidence.
3. Authoritative records are append-only; projections are rebuildable and
   grounding can quarantine them.
4. Instance identity and project membership are checked before runtime state is
   read or changed.
5. Dedicated state is never rebound to another platform implicitly.
6. Umbrella membership is explicit and scoped; adding a member is an operator
   decision.
7. Platform-specific extensions sit above stable storage and lifecycle ports.
8. External project knowledge such as pgvector is bound to an explicit member,
   model space, adapter configuration, and source revision. It never becomes an
   implicit cross-instance read or a claimed cross-database ACID transaction.

## Operator knowledge

An instance may project or query operator-authored project knowledge through a
capability adapter. pgvector is the first planned compatibility target for
existing Postgres estates. Vyrm remains authoritative for reasoning, policy,
audit, trace, graph, projection freshness, and adapter decisions; Postgres owns
its rows and transaction history. An idempotent outbox coordinates writes and a
persisted trace link records the exact project and external source revision.
See [`runtime-tracing-operator-knowledge.md`](runtime-tracing-operator-knowledge.md).

The portable boundary is now executable in `vyrm-operator`: immutable
project/member and source/tenant/model bindings, snapshot/catalog/stable-revision
evidence, exact/HNSW/IVFFlat controls, bounded results, safe pgvector SQL shapes,
and idempotent vector-outbox work. A live PostgreSQL connection and deployment
now have repeatable-read snapshot/catalog capture, project revision controls,
typed upsert/delete work, and exact/HNSW/IVFFlat endpoint differentials behind
an opt-in feature. Certificate-backed TLS endpoint, process-restart, concurrent
failure injection, typed payload filters, and scale evidence remain before
promotion.

## Upstream capability adoption

SurrealDB and Qdrant are postponed inputs to capability work. When resumed,
their implementations may be inspected and adapted for this private
proof-of-concept, but they are not merged wholesale into the product kernel.
Each adopted capability must name:

- the frontier-runtime gap it closes;
- the instance layer that owns it;
- the invariant and differential tests that constrain it;
- whether it is shared kernel behavior or a platform-specific extension.

Surreal-derived work precedes Qdrant-derived work when this sequence is
unpostponed.

## Current implementation boundary

The existing `.vyrm/store` layout and persisted routing root binding implement
dedicated per-checkout isolation. A versioned `.vyrm/instance.toml` now carries
relocatable identity and topology. Model-facing CLI paths and `vyrmd` reject a
missing manifest or foreign store/root pairing.

Missing `.vyrm/store` paths now initialize native `vyrmKV`. Runtime entry points
share `PersistentEngine`: an authenticated native `CURRENT` marker selects
native on reopen, while an existing non-native directory selects the explicit
Fjall compatibility adapter. Store identity is derived from durable bytes, not
from filesystem proximity or a mutable environment default.

Umbrella manifests validate explicit, relative, non-escaping membership, but
runtime execution intentionally remains denied until routing, reasoning, and
policy state have explicit member scoping and cross-member tests.
