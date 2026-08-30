# RRFlow platform canon

**Status:** authoritative terminology and hierarchy for the RRFlow `0.1` alpha
**Effective:** 2026-08-27
**Authority:** this is the only normative platform-term list in the repository

RRFlow is the product. RRD (Reason Ready Daemon) is the single logical engine
and runtime inside an RRFlow instance. Every contract, crate, API, SDK,
configuration file, test, document, and Connectome surface must use the terms
below with the same meaning. Industry research is indexed separately in
[`research/README.md`](research/README.md) and cannot redefine this vocabulary.
The one end-to-end execution and implementation map is
[`../rrflow-rrd-architecture.md`](../rrflow-rrd-architecture.md).

## Two deployment profiles, one product

These are deployment profiles, not separate products, engines, editions, or
version lines. Both run the same RRFlow release, RRD authority, capability
catalogue, transaction semantics, security model, and evidence format.

1. **Project deployment** — one instance binds exactly one project and one
   deployment environment. It is the AI governance, knowledge, and development
   layer for that project whether the environment is development, staging, or
   production.
2. **Estate deployment** — an organization-level control plane manages many
   project instances and the clusters on which they run. It does not combine
   multiple projects into one project instance or create a second data engine.

Executable multi-project instance topology is retired. Multi-project operation
belongs to the estate profile; source/build aggregation remains project
topology, not instance tenancy.

## Canonical flow

```text
RRFlow product
├── organization
│   ├── project ── environment ──> instance ──> exactly one logical RRD authority
│   └── estate
│       ├── manages project instances
│       └── manages clusters
├── instance data catalogue
│   └── namespace
│       └── database
│           ├── table ──> record
│           ├── collection ──> point ──> vector + payload
│           ├── relation
│           └── alias
└── physical placement
    ├── cluster ──> node
    └── shard ──> replica (placed on a node) ──> segment
```

`tenant` is an authorization and data-placement identity applied to this flow;
it is not another container between every pair of resources. A tenant binding
selects permitted namespaces/databases and, where configured, indexed payload
partitions or dedicated shards.

## One standardized term list

| Term | Kind and owner | Exact RRFlow meaning | Codebase direction |
|---|---|---|---|
| `RRFlow` | Product | The complete AI governance, knowledge, development, data, runtime, and operator system. | Keep as the only product brand. |
| `RRD` | Engine/runtime | Reason Ready Daemon: the one logical engine authority within an instance. Specialized storage, graph, query, vector, and inference crates are internal operators. | `rrd-engine` becomes the sole composition and authority entry point. |
| `organization` | Control identity | The administrative owner of projects, estates, principals, policy, and billing/operations metadata. | Already a public `ResourceKind`; authoritative organization state is not implemented. |
| `estate` | Control-plane resource | A managed fleet and lifecycle aggregate of instances and clusters owned by one organization. It owns desired state, reconciliation, fleet backup policy, and operational inventory. | Keep `rrd-estate`; never use estate as a database, tenant, namespace, or cluster synonym. |
| `project` | Governed workload identity | One software/product codebase governed by RRFlow. | Each project deployment instance binds exactly one project; no umbrella membership. |
| `environment` | Instance attribute | The deployment context of a project instance, such as development, staging, or production. | Add an explicit typed attribute; never encode environment by overloading namespace, tenant, or instance mode. |
| `instance` | Deployment and authority | One deployed RRFlow runtime bound to exactly one project and one environment, backed by exactly one logical RRD authority. It may be embedded, single-node, or served by a cluster. | Preserve `.rrflow/instance.toml`; retire `Dedicated`/`Umbrella` as public topology. |
| `workspace` | Discovered build fact only | A Cargo, pnpm, npm, uv, Go, Gradle, Maven, .NET, or similar workspace found while attuning a project. | Never add `WorkspaceId`, `ResourceKind::Workspace`, `/workspaces`, tenancy, or data ownership. |
| `namespace` | Logical catalogue and isolation resource | The highest logical data/security container inside an instance. It contains databases and supplies an administration/policy scope. | Public `ResourceKind` exists; catalogue persistence remains incomplete. Kubernetes and language namespaces are unrelated. |
| `database` | Logical data resource | A schema/catalogue and transaction container inside one namespace. It owns tables, collections, relations, indexes, aliases, triggers, and functions. | Public `ResourceKind` exists; persistence must enter the single `rrd-engine` catalogue. It is not an instance or filesystem directory. |
| `tenant` | Security and placement identity | A customer/principal data-isolation identity explicitly bound to permitted namespace/database resources and an indexed partition or shard strategy. | Do not create one collection per tenant by default; tenant predicates must be index/shard routed. |
| `table` | Logical catalogue resource | A typed or explicitly schemaless set of records in one database. | One catalogue owns tables and their schemas. |
| `collection` | Vector-specialized table | A table with named vector schema, metrics, index/quantization policy, and point operations. It is not a second catalogue or database. | Fold the existing vector collection catalogue into the unified engine catalogue. |
| `record` | Logical data identity | One versioned entity in a table, with typed fields and one canonical identity. | Remains the general multi-model entity. |
| `point` | Vector-specialized record | A record addressed through a collection. It contains named vectors and payload fields under the same transaction/read stamp. | Do not persist point and record as competing truths. |
| `vector` | Point/record value | A named dense, sparse, or multi-vector value with dimensions, metric, provenance, and optional quantization/index artifacts. | Authoritative full-precision value and derived artifacts share one lifecycle. |
| `payload` | Point fields | The non-vector typed fields exposed with a point for filtering and result projection. | Payload is a point/record view, not a detached document store. |
| `relation` | Logical data identity | A typed, directed relationship between canonical records; a point participates through its record identity. | Relations and graph traversal remain inside the database transaction/catalogue. |
| `alias` | Catalogue indirection | An atomically switched alternate name for a table or collection within the same database and resource kind. | No cross-database, cross-kind, or filesystem aliases. |
| `strict mode` | Typed enforcement policy | A scoped policy combining declared-schema enforcement, indexed-filter requirements, query/write bounds, timeout/rate limits, and storage caps. | Implement as `StrictModePolicy`, not an ambiguous boolean and not RBAC. |
| `cluster` | Physical service topology | A group of RRD nodes participating in consensus, placement, replication, availability, and compute service. | Keep `rrd-cluster`; a cluster can host instances but does not own their logical identity. |
| `node` | Physical runtime member | One process/host identity participating in a cluster. | Keep distinct from instance and replica. |
| `shard` | Physical horizontal partition | A partition of database-owned data with an explicit routing and consistency identity. | Bind shards to logical catalogue resources through RRD; never expose a second data authority. |
| `replica` | Physical copy | One placed copy of a shard on a cluster node, with role, health, and consistency state. | Keep replica placement in `rrd-cluster`. |
| `segment` | Physical storage unit | A bounded mutable or immutable storage/index unit within a shard replica. | Internal implementation detail shared by storage/vector lifecycle vocabulary; not a tenant or public logical container. |

## Rejected resource synonyms

- `domain` is not a public resource. Do not add `Domain`, `DomainId`,
  `domain_id`, or `/domains`. Use `namespace` for logical isolation and the
  precise model name for application data.
- `boundary` remains ordinary architecture language for an authority,
  transaction, trust, or component edge. It is never a resource kind or ID.
- `workspace` never means namespace, tenant, project, instance, or estate. It
  is permitted only when preserving the vocabulary of a discovered build tool.
- `umbrella` is not a deployment profile. Estate control replaces multi-project
  umbrella instances.
- `scope` is a generic parameter/value describing where an operation applies;
  it must resolve to canonical resource identities and must not become another
  container type.

## Canonical resource paths

Resource paths are explicit; current directory, session state, labels, and
filesystem proximity never supply missing identity.

```text
organization/{organization}/estate/{estate}
organization/{organization}/project/{project}/instance/{instance}
instance/{instance}/namespace/{namespace}/database/{database}
.../table/{table}/record/{record}
.../collection/{collection}/point/{point}
.../relation/{relation}
estate/{estate}/cluster/{cluster}/node/{node}
.../shard/{shard}/replica/{replica}/segment/{segment}
```

An alias retains the target path's database and kind. A tenant is carried in
the authenticated authorization context and resolves to allowed logical paths
and placement constraints; it cannot be inferred from a payload supplied by a
client.

## RBAC and privilege performance invariant

Security cannot literally have zero CPU cost. The locked requirement is that
it adds no policy interpretation, storage lookup, allocation, or authorization
branch inside record, vector-candidate, graph-edge, or segment inner loops.

- Authentication occurs when a session is established.
- Roles and grants compile into an immutable `AuthorizationContext` keyed by
  instance, principal, tenant binding, and security revision.
- Warm action/resource admission is an in-memory indexed lookup performed once
  per command or query plan. A security-revision change invalidates it.
- Tenant, row, and field privileges compile into the query plan. Tenant
  isolation uses catalogue indexes or shard routing; it may not degrade into a
  hidden full scan or post-filter.
- An authorized mutation carries a typed, revision-bound capability into the
  transaction coordinator; physical operators do not re-run RBAC.
- Audit identity and completion remain unavoidable and transactionally bound,
  with batching permitted only when durability and denial evidence are
  preserved.

Qualification must prove no warm-path security-state reads, no per-result
authorization callbacks, identical scan/candidate counts to the equivalent
explicit indexed predicate, and no more than 2% throughput or p95 latency
regression against the same guarded operation with admission pre-authorized.
Cold authentication, policy compilation, and revision invalidation are
measured separately and cannot be hidden in that comparison.

The current security implementation does not meet this invariant: it reloads
`SecurityState` and linearly scans direct principal grants on each authorization,
and it has no role compilation. This is an explicit G05 security implementation
gap, while G01 must first preserve one security authority.

## Code-backed migration facts

- [`PLATFORM_TERMS`](../../crates/rrd-contract/src/platform.rs) is the
  machine-readable mirror of this exact table and maps every public resource
  term to [`ResourceKind`](../../crates/rrd-contract/src/lib.rs). Contract tests
  fail on term/order drift and rejected public resource synonyms. Resource-path
  validation still checks uniqueness rather than enforcing every hierarchy
  shown above.
- [`runtime/instance.rs`](../../crates/rrd-engine/src/runtime/instance.rs)
  accepts only the frozen format-1 `Dedicated` plus `members = ["."]` shape.
  Those fields remain serialized solely to preserve existing authority bytes;
  explicit environment identity requires a successor-format migration.
- [`rrd-vector/collection.rs`](../../crates/rrd-vector/src/collection.rs)
  implements a separate vector collection catalogue that must become an
  internal operator of the unified RRD catalogue.
- [`rrd-security`](../../crates/rrd-security/src/lib.rs) owns persistent policy
  truth but still uses per-principal direct grants and the warm-path behavior
  described above.
- [`rrd-estate`](../../crates/rrd-estate/src/lib.rs) and
  [`rrd-cluster`](../../crates/rrd-cluster/src/contract.rs) already model the
  distinct control-plane and physical-topology concepts; they must remain
  distinct while routing through `rrd-engine`.
- Logical `namespace` and `database` kinds now exist in the RRD public
  contract, but their catalogue persistence, transaction behavior, and public
  API operations are not implemented. Current Kubernetes,
  programming-language, and keyspace uses of the word namespace are not that
  feature.

No code surface may claim conformance merely because its name appears here.
Each migration remains incomplete until contract, engine, persistence, public
surface, restart, authorization, and performance evidence pass its RRFlow work
item.
