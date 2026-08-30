# Platform industry research index

**Status:** inventory only
**Reviewed:** 2026-08-27
**Canonical root:** `docs/platform/research/`

This directory is the single entry point for industry research used to shape
the RRFlow platform. This index lists source systems and repository research;
it is not a second terminology list or an implementation plan. Canonical RRFlow
terms are defined only in [`../README.md`](../README.md).

## File naming

Use lowercase kebab-case and one of these forms:

- `<subject>-capability-inventory.md` for a source-system inventory;
- `<subject>-architecture-notes.md` for bounded technical research;
- `rrd-<topic>-differential.md` for evidence-backed comparisons;
- `rrd-<topic>-decision.md` for an accepted terminology or architecture
  decision;
- `rrd-<topic>-implementation-plan.md` only after the inventory and decision
  exist.

`README.md` is the authoritative index. New platform-industry research must be
linked here. Existing flat documents remain at their current paths until a
separate link-preserving migration is reviewed.

## External reference systems

| Subject | Terms to inventory | Primary sources | Repository note |
|---|---|---|---|
| Qdrant | collection, point, vector, payload, segment, shard, replica, alias, tenant, strict mode | [overview](https://qdrant.tech/documentation/overview/), [collections](https://qdrant.tech/documentation/manage-data/collections/), [distributed deployment](https://qdrant.tech/documentation/scaling/distributed_deployment/), [multitenancy](https://qdrant.tech/documentation/manage-data/multitenancy/) | [`qdrant-capability-inventory.md`](../../qdrant-capability-inventory.md) |
| HelixDB | labeled property graph, node, edge, label, property, index, query, workspace, project, cluster | [introduction](https://docs.helix-db.com/database/helix-db/start-here/introduction), [data model](https://docs.helix-db.com/database/helix-db/core-concepts/data-model), [run modes](https://docs.helix-db.com/database/helix-db/start-here/run-modes), [repository](https://github.com/HelixDB/helix-db) | No peer capability inventory exists yet. |
| SurrealDB | namespace, database, table, record, relation, organization, project, instance, cluster | [architecture](https://surrealdb.com/docs/architecture), [namespace/database hierarchy](https://surrealdb.com/docs/learn/schema-management/multi-tenancy/namespace-and-database-architecture), [repository](https://github.com/surrealdb/surrealdb) | [`surrealdb-capability-inventory.md`](../../surrealdb-capability-inventory.md); [`rrflow-surrealdb-differential.md`](../../rrflow-surrealdb-differential.md) |

The reviewed SurrealDB material does not establish **estate** as its canonical
data-isolation term. Its documented logical hierarchy is namespace → database
→ table/record; its managed and deployment vocabulary is separate. Any RRD use
of `estate` therefore needs an RRD-owned definition rather than attribution to
SurrealDB.

## Canonical RRFlow vocabulary

The research above informed the one standardized list in
[`docs/platform/README.md`](../README.md). Research notes may compare foreign
terms, but they must link to that list instead of restating or redefining RRD
terminology.

## Existing RRD research and architecture notes

The following documents currently participate in this naming discussion:

- [`rrd-data-services-architecture-research.md`](../../rrd-data-services-architecture-research.md)
- [`estate-control-v1.md`](../../estate-control-v1.md)
- [`local-estate-authorization-v1.md`](../../local-estate-authorization-v1.md)
- [`instance-topology.md`](../../instance-topology.md)
- [`rrd-engine-foundation-cheat-sheet.md`](../../rrd-engine-foundation-cheat-sheet.md)
- [`rrd-engine-capability-coverage.md`](../../rrd-engine-capability-coverage.md)
- [`full-stack-gap-ledger.md`](../../full-stack-gap-ledger.md)

## Remaining research and migration gaps

- Add a HelixDB capability inventory at
  `docs/platform/research/helixdb-capability-inventory.md`.
- Complete the code audit against the canonical platform list across contracts,
  engine, storage, services, SDKs, tests, and documentation.
- Classify and migrate each conflicting use without retaining aliases or a
  second glossary.
- Author the full researched implementation plan only after the terminology
  and migration gaps have been incorporated into the enforced RRFlow board.
