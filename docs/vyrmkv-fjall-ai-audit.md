# Fjall → vyrmKV AI-runtime audit

Status: bounded leveled-maintenance, negative-filter, and five-profile AI-read
matrix pass, 2026-08-22.

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

The `ai_hotset_benchmark` runs both engines in alternating isolated processes.
Setup publishes 8,192 cold keys into immutable storage and overwrites 128
control-like keys in the active memtable. Each mode performs 8,192 verified
requests in each of five local x86-64 Linux trials. Fan-out requests resolve 32
mixed hot, cold, and absent metadata keys; its throughput is resolved items per
second and its latency is for the complete 32-key request.

| Profile | Fjall items/s | Native items/s | Native/Fjall throughput | Fjall p95 | Native p95 | Native/Fjall p95 |
|---|---:|---:|---:|---:|---:|---:|
| Current hot hit | 1,344,294 | 3,011,405 | 2.240× | 755 ns | 279 ns | 0.370× |
| Cold immutable hit | 455,398 | 753,217 | 1.654× | 2,200 ns | 1,291 ns | 0.587× |
| Point miss | 2,116,674 | 3,214,490 | 1.519× | 415 ns | 266 ns | 0.641× |
| Historical hot-key version | 489,604 | 1,055,474 | 2.156× | 2,154 ns | 973 ns | 0.452× |
| 32-key metadata fan-out | 720,851 | 1,325,589 | 1.839× | 44,217 ns | 22,553 ns | 0.510× |

All raw trials passed exact-value correctness and the strict native-throughput
and p95 gate. The same fixture occupied 1,588,171 native bytes versus
68,300,360 Fjall bytes. That disk result is specific to this small physical
fixture and must not be extrapolated to arbitrary data.

Run it with:

```console
cargo run --release --locked -p vyrm-store --example ai_hotset_benchmark -- \
  --workload metadata-fanout --trials 5 --cold-keys 8192 --hot-keys 128 \
  --reads 8192 --batch-size 128 --value-bytes 128 --fanout-width 32 \
  --output target/vyrmkv-ai-metadata-fanout.json --require-promotion
```

Setup time is excluded, both paths copy returned values for equal validation,
and correctness is checked on every item. These profiles establish their five
bounded hypotheses; they do not prove general superiority, range performance,
sustained maintenance, or multi-writer behavior. A proposed one-segment
multi-get allocation shortcut was also measured in five before/after trials
and rejected when it failed to improve the median. The kernel was restored to
the measured baseline rather than retaining unearned complexity.

Raw evidence:

- [`current hot hit`](../eval/results/2026-08-22-vyrmkv-ai-current-hot-hit.json)
- [`cold immutable hit`](../eval/results/2026-08-22-vyrmkv-ai-cold-hit.json)
- [`point miss`](../eval/results/2026-08-22-vyrmkv-ai-point-miss.json)
- [`historical hot-key version`](../eval/results/2026-08-22-vyrmkv-ai-historical-hot-hit.json)
- [`metadata fan-out`](../eval/results/2026-08-22-vyrmkv-ai-metadata-fanout.json)

The scheduled/manual workflow now reruns all five modes with a deny-by-default
promotion gate and retains every raw trial as an artifact.

## Ordered implementation gates

1. **Complete:** memtable-first single/multi-get with MVCC/cache-traffic proof
   and an isolated hot-control-set comparison.
2. **Complete locally:** configurable encoded-WAL-payload and memtable-version
   thresholds synchronously flush the existing WAL-backed memtable before
   admitting a batch that would cross either bound. Atomic batches are never
   split; a single oversized batch is accepted as one durable WAL frame,
   retained for the next synchronous
   flush, and counted explicitly. Native physical evidence exposes the limits
   and process-local automatic-flush, write-stall, failure, and oversized-batch
   counters. Admission uses cached O(1) version cardinality and the encoded WAL
   payload length already produced for the write; it never rescans the live
   memtable or batch. Crash recovery across the segment/continuation-WAL
   boundary is covered. Retain threshold-crossing burst latency and remote
   sustained evidence as promotion gates.
3. **Complete locally:** derive authenticated block-local negative filters from
   validated v3 bytes using ten bits per physical entry and seven hashes.
   Deterministic point-miss evidence proves negative probes avoid block loads;
   false positives retain the exact read path and cannot change results. No
   wire-format revision was needed because filters are derived acceleration
   state.
4. **Complete locally:** replace full-materialization, all-to-one compaction
   with bounded deterministic level selection, forward-only k-way record merge,
   key-boundary output partitioning, higher-level non-overlap enforcement,
   lower-level tombstone protection, and physical maintenance evidence.
   Automatic steps retain all versions; explicit protected-snapshot compaction
   is the only pruning path. Asynchronous immutable-memtable flush remains a
   separate latency gate.
5. Test family-aware blocks/cache admission and prefix/restart compression
   against the simpler single-space design. Reject it if mixed-family evidence
   does not improve.
6. **Partial:** hot hits, cold hits, misses, historical reads, and vector
   metadata fan-out now have correctness-checked isolated Fjall/native gates.
   Append/replay and mixed atomic-family coverage already exist in the general
   promotion and semantic suites. Compaction interference, long-duration
   restart/RSS, and crash/storage-full performance cells remain.

Fjall removal requires all semantic/migration differentials plus repeated
remote AI-matrix evidence. Favorable local matrix cells are not enough.
