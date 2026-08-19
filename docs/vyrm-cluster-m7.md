# Vyrm M7 cluster contract and deterministic simulation

Status: protocol/simulation gate plus the first real-consensus adapter slice
implemented on 2026-08-19. This is not a production Multi-AZ implementation.

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

## Real-consensus adapter slice

The optional `openraft-adapter` feature pins OpenRaft `0.9.25` and keeps Tokio,
OpenRaft, and native storage out of the default protocol/simulator build. The
pin is deliberate: `0.9.25` is the current stable line and includes upstream
commit-safety and membership-divergence corrections, while the `0.10` line is
still alpha. The reviewed upstream tag resolves to commit
`8815cdba2826f74e848acef361ad03f93bb1c3f8`.

Selection matrix:

| Candidate | Decision | Reason |
|---|---|---|
| OpenRaft 0.9.25 | Adopt behind a feature | Application-neutral log, state-machine, snapshot, and network ports; storage conformance suite; stable release line |
| OpenRaft 0.10 alpha | Reject for this gate | Alpha API/behavior is not the right persistence-format dependency |
| TiKV `raft-rs` | Defer, retain as a design reference | Strong production lineage, but lower-level integration and readiness driving would add more Vyrm-owned consensus plumbing before this contract is proved |

The Vyrm adapter format is now `v3`. It supplies:

- a canonical command/response/node `RaftTypeConfig` with typed `probe` and
  `runtime_commit` operations;
- a store permanently bound to one shard and two explicit physical domains:
  canonical state at the instance root and node-local Raft state under
  `raft-local-v3`;
- authoritative VyrmKV batches for node-local votes, committed/purged pointers,
  and ordered logs, without placing those records in transferable state;
- monotonic vote persistence and append-batch hole rejection;
- full-command idempotency, payload integrity, shard binding, placement-epoch
  binding, and optional expected-commit-index comparison;
- native `RuntimeCommit` planning without publication, allowing canonical
  runtime mutations, audit/outbox work, the Raft applied cursor, response, and
  idempotency state to share one authoritative VyrmKV WAL frame;
- digest-chained application state and metadata-checked physical snapshot
  installation;
- snapshot-bundle v1 export/install carrying the applied cursor, membership,
  request identities, schema, audit/outbox, and every canonical runtime record;
- a content-addressed local snapshot object plus a small local VyrmKV reference,
  preventing recursive bundles and avoiding the 8 MiB value ceiling;
- the complete upstream OpenRaft storage conformance suite; and
- a real four-node in-process engine test that elects a leader, commits
  commands, purges snapshotted logs, catches up a new learner through snapshot
  transfer, isolates the leader, elects on the majority side, commits after
  failover, and completes joint-to-uniform membership replacement.

A real canonical `RuntimeCommit` run waits for three voters to apply the same
truth, builds an authenticated physical snapshot, purges the leader through
that snapshot, and adds a fourth node. The fresh learner receives the snapshot,
reopens through `NativeEngine`, and exposes the same commit identity and cursor
as all voters. Storage differentials additionally prove same-frame
Raft/runtime publication, duplicate replay without a second runtime mutation,
durable expected-cursor denial, corrupt-byte and forged-metadata refusal before
state publication, idempotent reinstallation, stale refusal, restart recovery,
and preservation of the target node's local vote.

Snapshot data is exactly VyrmKV physical snapshot-bundle v1: a flush-bounded,
SHA-256-authenticated manifest and immutable-segment closure installed through
one new local manifest publication. Before installation the adapter reads the
state config and state-machine record directly from the validated closure,
checks shard/domain ownership, binds OpenRaft metadata and snapshot id, and
refuses any local-Raft config in the bundle. Source manifest ancestry is never
adopted. Snapshot cache bytes live in the node-local content-addressed object
tier; only their verified reference is stored beside local Raft history.

This is stronger evidence than the single-term simulator, but the two tests
serve different purposes. The simulator gives replayable schedules for explicit
fault events; the in-process test exercises the real consensus engine and
durable adapter. The latter uses a controlled wall-clock lease wait and is not
represented as deterministic virtual-time model checking.

Research sources, retrieved 2026-08-19:

- [OpenRaft releases](https://github.com/databendlabs/openraft/releases)
- [OpenRaft storage implementation guide](https://docs.rs/openraft/latest/openraft/docs/getting_started/index.html)
- [OpenRaft `RaftLogStorage` contract](https://docs.rs/openraft/latest/openraft/storage/trait.RaftLogStorage.html)
- [TiKV raft-rs](https://github.com/tikv/raft-rs)

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

This gate still does not contain production RPC, node identity/authentication,
TLS, membership discovery, admission control, multi-shard atomic commit, or
metadata-shard reshard cutover. Application state currently proves ordered
identity/CAS/digest semantics and now atomically dispatches canonical
`RuntimeCommit` transactions into native VyrmKV and transfers that runtime state
in Raft snapshots. Snapshot construction and receipt currently use
`Cursor<Vec<u8>>`; OpenRaft can chunk those bytes on the wire, but Vyrm does not
yet claim file-backed or bounded-memory snapshot streaming. Its synchronous
mutex, prefix scans, JSON protocol state, and unbounded request-id map are
correctness-first test implementations, not production throughput or footprint
claims. Commands are limited to 1 MiB and physical snapshot envelopes to 1 GiB
until compact and streaming codecs land.

The next M7 slice must define an explicit placement-epoch transition command,
bound idempotency state, add authenticated production transport, and run
process/network/disk chaos on independently restarted nodes. File-backed,
bounded-memory snapshot creation/receipt is also required before high-volume
cluster claims. Only that evidence can advance a Multi-AZ claim.
