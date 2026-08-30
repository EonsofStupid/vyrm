# Instance topology

The canonical platform terms and hierarchy are defined only in
[`platform/README.md`](platform/README.md). This document applies that
terminology to deployment and does not define a second glossary.

## Locked deployment rule

One RRFlow instance is bound to exactly:

- one project;
- one environment;
- one logical RRD authority.

An instance can run embedded, as a single server node, or through a cluster.
Those execution shapes do not change its logical ownership. An estate manages
multiple project instances and their lifecycle; a cluster supplies physical
nodes, consensus, placement, replication, and availability. Neither estate nor
cluster is a logical database or a multi-project instance.

`workspace` describes discovered build topology inside a project. It is not a
deployment, authorization, catalogue, or data-isolation resource. `namespace`
owns logical catalogue isolation inside RRD.

## Format-1 manifest contract

`.rrflow/instance.toml` remains a relocatable identity manifest. Format 1 has a
single valid shape:

```toml
format = 1
id = "project-instance"
mode = "dedicated"
members = ["."]
```

`mode` and `members` remain serialized only because the persisted
`ProjectAuthorityBinding` digest already covers those fields. They are frozen
V1 compatibility fields, not extension points. The runtime rejects any other
mode or member list and refuses to bind a nested or neighboring project.

The environment identity is a required part of the canonical instance model,
but it is not added silently to manifest V1: doing so would invalidate existing
authority digests. A successor manifest and project-authority format must add
the environment through an explicit, fail-closed migration with reopen and
rollback fixtures.

## Runtime invariants

1. Instance identity and the exact project root are verified before runtime
   state is read or changed.
2. A database authority is never rebound to another project or filesystem root
   as a startup side effect.
3. Recall is injected before reasoning; mutation requires a valid typed
   reasoning transition, current authorization, and fresh evidence.
4. Authoritative records are append-only; rebuildable projections carry
   freshness evidence and can be quarantined by grounding.
5. Platform-specific extensions sit above stable engine and lifecycle ports.
6. External knowledge adapters bind an exact project, environment, tenant,
   model space, source revision, and authorization context. They do not imply
   cross-instance reads or cross-database ACID.

## Provisioning and routing

Estate and cluster controllers provision the same project-instance contract
before starting `rrd-server`. The embedded profile applies the identical
contract in the project checkout. All service faces must resolve to the same
`RrdEngine` authority rather than independently opening storage.

The intended routing chain is:

```text
organization
  -> estate
    -> project + environment
      -> instance
        -> logical RRD authority
          -> namespace -> database -> tenant
            -> table / collection / relation / alias
```

Physical placement remains a separate chain:

```text
estate -> cluster -> node -> shard -> replica -> segment
```

The complete query, transaction, vector, security, and implementation flow is
owned by [`rrflow-rrd-architecture.md`](rrflow-rrd-architecture.md).
