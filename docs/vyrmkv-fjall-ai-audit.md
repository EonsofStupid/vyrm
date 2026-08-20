# Fjall → vyrmKV AI-runtime audit

Status: first executable optimization and benchmark profile, 2026-08-20.

## Decision

Fjall is a compatibility oracle and engineering reference, not Vyrm's physical
backend. `vyrm-kv` already owns its WAL, MVCC sequence allocation, memtable,
content-addressed immutable segments, manifest/CURRENT protocol, checkpoints,
snapshot bundles, compaction, block cache, failure injection, and recovery. The
Fjall dependency is pinned to 3.1.8 inside `vyrm-store`; new stores select native
VyrmKV and old Fjall directories move only through the verified migration path.

That does **not** mean the native engine is mature enough to dismiss Fjall.
Fjall is substantially ahead in general-purpose LSM mechanics. Vyrm earns a
separate engine only where its known AI-runtime access patterns produce measured
advantages while every semantic and physical differential remains green.
`vyrmQL` is Vyrm's language; `vyrmKV` is its storage engine. Calling the latter
a new language would blur the architecture.

## Sources inspected

The compatibility source is the exact [Fjall 3.1.8 crate](https://docs.rs/crate/fjall/3.1.8/source/)
and its `lsm-tree` dependency installed by `Cargo.lock`. Current upstream was
also inspected at Fjall commit
[`00a221e`](https://github.com/fjall-rs/fjall/commit/00a221e3f008056c867ffae8f7641ee3d0798524)
(3.1.9 at review time), including the
[README](https://github.com/fjall-rs/fjall),
[3.x change history](https://github.com/fjall-rs/fjall/blob/main/CHANGELOG.md),
and [keyspace policies](https://docs.rs/fjall/3.1.8/fjall/struct.KeyspaceCreateOptions.html).
These are primary project sources. No Fjall source was copied into Vyrm.

## What Fjall gets right

| Mechanism | Fjall strength | Vyrm response |
|---|---|---|
| LSM maintenance | Leveled compaction, background flush/compaction workers, write stalls, and global write-buffer accounting | Treat automatic bounded maintenance and backpressure as a native gap; do not hide it behind the adapter |
| Read amplification | Bloom/filter policies, partitioned filters and indexes, configurable pinning, block sizing, and restart intervals | Add only after negative-hit and fan-out profiles establish the policy per Vyrm data family |
| Physical isolation | A separate physical LSM per keyspace with cross-keyspace journal/transaction semantics | Measure against Vyrm's single ordered space: it is cheaper for one atomic causal commit but can cause family-level compaction/cache interference |
| Large values | Optional KV separation with compaction-time blob rewriting | Keep large immutable objects and vector artifacts in VyrmDS/content-addressed storage; avoid duplicating a blob log inside VyrmKV |
| Concurrency | Serialized and optimistic serializable transaction modes | Single-writer remains correct for the local alpha; OCC belongs to the team-tier contention matrix |
| Operational maturity | Cache/write-buffer/disk/compaction metrics and automatic maintenance | Expose the equivalent native counters before Fjall compatibility retirement |

## AI-specific physical model

Vyrm can specialize because its workload is not an arbitrary `BTreeMap`:

| Data class | Expected access | Native opportunity |
|---|---|---|
| Control truth | Hot point reads and overwrites: cursor, schema, routing generation, leases, idempotent outcomes | Memtable-first reads, tiny pinned control blocks, exact generation-aware admission |
| Causal streams | Append and bounded cursor replay: changes, audit, outbox, reasoning and workflow events | Prefix-compressed sequential blocks, cursor fences, streaming merge without materializing unrelated families |
| Temporal entities | Current point reads plus explicit historical snapshots | Latest-version accelerators that always fall back to MVCC truth; snapshot-pin-aware compaction |
| Search metadata | Batched point fan-out over vector generation/provenance/filter state | Sorted multi-get, negative filters, family-aware cache admission, immutable mmap artifacts outside the KV value path |
| Large artifacts | Content-addressed, immutable, verified, often streamed | VyrmDS local/S3 object references; never amplify them through the WAL/LSM |

## First implemented optimization: hot control truth

`Database::get` now resolves a visible active-memtable version before examining
immutable segments. Because every memtable sequence is newer than every
published segment, that result—including a tombstone—is authoritative for the
requested snapshot. `get_many` resolves all such keys first and sends only the
unresolved subset to segment lookup. Historical snapshots still fall through to
the exact older segment version.

The MVCC regression `hot_memtable_point_reads_bypass_immutable_blocks_without_changing_mvcc_results`
proves current overwrites and deletes cause zero block-cache hits or misses,
while an older snapshot loads and returns the correct immutable values. The
20,000-operation Fjall/native/independent-model mutation soak, runtime hash-chain
tests, and snapshot differential remain green.

The new `ai_hotset_benchmark` runs both engines in alternating isolated
processes. Setup publishes 8,192 cold keys into immutable storage, overwrites
128 control-like keys in the active memtable, then measures 65,536 verified
current point reads. Five local x86-64 Linux trials produced these medians:

| Metric | Fjall 3.1.8 | Native VyrmKV | Ratio |
|---|---:|---:|---:|
| Hot current reads/s | 1,379,005 | 3,117,435 | 2.261× native/Fjall |
| Hot current p95 | 746 ns | 283 ns | 0.379× native/Fjall |

Run it with:

```console
cargo run --release --locked -p vyrm-store --example ai_hotset_benchmark -- \
  --trials 5 --cold-keys 8192 --hot-keys 128 --reads 65536 \
  --batch-size 128 --value-bytes 128 --output target/vyrmkv-ai-hotset.json
```

This result validates one hypothesis only. Setup time is excluded, both paths
copy the returned value for equal validation, and correctness is checked on
every read. It does not prove general superiority, negative-read performance,
range performance, sustained maintenance, or multi-writer behavior. The
versioned benchmark output retains every raw per-trial result so repeated runs
can expose regressions across machines.

## Ordered implementation gates

1. **Complete:** memtable-first single/multi-get with MVCC/cache-traffic proof
   and an isolated hot-control-set comparison.
2. Add bounded native memtable/WAL thresholds, explicit backpressure, automatic
   flush, and observability; compare burst latency and acknowledged-write
   durability against Fjall.
3. Add authenticated per-segment negative filters only after point-miss and
   multi-get fan-out baselines; freeze the format and false-positive policy.
4. Replace full-materialization, all-to-one compaction with bounded leveled
   streaming compaction that respects snapshot/checkpoint pins and family
   retention policy.
5. Test family-aware blocks/cache admission and prefix/restart compression
   against the simpler single-space design. Reject it if mixed-family evidence
   does not improve.
6. Run the complete AI matrix: hot hits, misses, historical reads, append/replay,
   mixed atomic families, vector metadata fan-out, compaction interference,
   restart, disk, RSS, and crash/storage-full boundaries.

Fjall removal requires all semantic/migration differentials plus repeated
remote AI-matrix evidence. A favorable append benchmark or this hot-set cell is
not enough.
