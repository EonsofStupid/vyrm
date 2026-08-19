# Vyrm M7 cluster contract and deterministic simulation

Status: first protocol/simulation gate implemented on 2026-08-19. This is not
a production Multi-AZ implementation.

## Outcome

`vyrm-cluster` freezes the cluster semantics that production adapters must
preserve before Vyrm adds networking or a consensus library:

- canonical, epoch-bound shard placement with unique ordered replicas;
- odd voter sets, quorum math, and explicit availability-zone constraints;
- `linearizable`, `bounded_stale`, and `exact_snapshot` read requests;
- quorum-durable writes as the only M7 write mode;
- per-shard stamps containing term, commit index, placement epoch, and state
  digest;
- snapshot vectors that retain their real partial order instead of inventing a
  total cluster cursor;
- route evidence containing requested consistency, selected replicas, health,
  observed stamp, and an allow/deny reason;
- grounded snapshot-plus-contiguous-WAL replica transfer;
- metadata-indexed reshard plans with exact cutover vectors; and
- fail-closed cross-shard writes until durable intents, recovery, idempotency,
  and a verified commit protocol exist.

The reserved metadata shard is shard `0`. A production control plane must put
placement and reshard transitions through the same quorum-durable ordered log;
an in-memory placement map is not linearizable metadata.

## Why this differs from the comparison systems

Current SurrealDB documentation exposes a consistent query layer over several
storage engines and describes SurrealDS as a separate quorum-based distributed
engine. Its public architecture is useful evidence for keeping compute and
storage contracts separate, but it does not provide enough public protocol
detail to substitute for a Vyrm fault model.

Qdrant uses Raft for cluster metadata, while point writes use separately
configurable replication, write consistency, read consistency, and ordering.
That is appropriate for its availability/throughput priorities. Vyrm cannot
adopt weaker point-write defaults for the authoritative reasoning/runtime log:
the initial Vyrm cluster contract requires a durable per-shard quorum before an
acknowledgement and denies a linearizable read when the leader cannot reach
quorum.

The simulator follows two stronger engineering patterns:

- FoundationDB's deterministic single-process simulation and replayable seed;
- TiKV/CockroachDB's per-range consensus/log model, quorum durability, and
  snapshot followed by ordered log catch-up.

Research sources, retrieved 2026-08-19:

- [FoundationDB simulation and testing](https://apple.github.io/foundationdb/testing.html)
- [TiKV Multi-Raft overview](https://tikv.org/deep-dive/)
- [CockroachDB replication layer](https://www.cockroachlabs.com/docs/stable/architecture/replication-layer)
- [SurrealDB architecture](https://surrealdb.com/docs/architecture)
- [SurrealDB multi-node boundary](https://surrealdb.com/docs/running/multi-node)
- [Qdrant horizontal scaling](https://qdrant.tech/documentation/scaling/horizontal-scaling/)
- [Qdrant consistency guarantees](https://qdrant.tech/documentation/scaling/consistency-guarantees/)

These are design inputs, not copied implementations or proof that Vyrm is
faster or more available.

## Deterministic fault model

`SimCluster` is a single-term, single-shard quorum model. Every message has a
monotonic identity and every fault is an explicit serializable event. A seed is
retained as evidence even though this first gate does not make random choices.
The same schedule yields byte-equivalent `SimEvidence`.

Covered events:

| Event | Modeled behavior |
|---|---|
| Partition/heal | Bidirectional link denial/restoration; blocked messages remain pending |
| Delay | A message's logical delivery tick moves forward |
| Duplicate | A new message identity carries identical content; append/ack remains idempotent |
| Reorder | The caller chooses delivery order; a follower refuses to skip a log index |
| Crash/restart | Volatile availability changes while the durable log survives |
| Clock skew | Recorded per node but never used to decide log order or commit |
| Disk loss | Durable log and applied cursor are removed; restart requires transfer |

The safety verifier rejects:

- a commit cursor beyond a replica's durable log;
- different term/content identities at the same log index; and
- loss of every durable copy of an acknowledged entry while disk failures stay
  within the placement's declared tolerance.

The model-check tests enumerate both possible first-follower quorum paths
crossed with every single disk loss in a three-voter/three-zone placement. They
also enumerate leader-minority partitions and require no acknowledgement.

## What is not yet claimed

This gate does not contain production RPC, authentication, encryption,
membership discovery, leader election, term changes, joint-consensus
reconfiguration, durable simulator storage, live snapshot transfer, admission
control, or multi-shard atomic commit. It therefore does not establish a
Multi-AZ product claim.

The next M7 slice must add a real consensus adapter behind this contract and
extend the model across elections, term changes, membership transitions,
snapshot installation, WAL catch-up, and metadata-shard reshard cutover. Only
then can process/network chaos and independent-node disk evidence begin.
