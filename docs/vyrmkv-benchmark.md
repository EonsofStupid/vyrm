# vyrmKV promotion benchmark

Status: strict local M3 promotion matrix, physical mixed-mutation soak, and
safe backend migration rehearsal pass; remote performance repetition remains.

The benchmark runs Fjall and native `vyrmKV` in separate fresh child processes.
Both receive the same valid claim corpus, authoritative batch boundaries, and
bounded sequence replays. Writes are measured in isolated writer children. A
second fresh probe process measures cold reopen, maintained reads, and steady
peak RSS so write-time allocations do not contaminate the steady-state result.
Each child verifies the full semantic sequence and first/last claim after cold
reopen. Native additionally reports uncompacted recovery, compaction/GC
maintenance, maintained recovery, maintenance peak RSS, active WAL/memtable
bounds, and automatic-flush/backpressure counters. The parent
alternates backend order, reports medians, and retains every raw trial.

Run the checked-in workload:

```console
cargo run --release -p vyrm-store --example engine_benchmark -- \
  --trials 5 --operations 2048 --batch-size 64 \
  --reads 512 --read-width 32 \
  --output eval/results/2026-08-19-vyrmkv-baseline.json \
  --require-promotion
```

The 2026-08-19 x86-64 Linux result is one deliberately modest local workload,
not a universal database claim. Its medians and ratios are:

| Metric | Fjall | Native | Native versus Fjall |
|---|---:|---:|---:|
| Authoritative write throughput | 70,763 ops/s | 77,179 ops/s | 1.091× |
| Authoritative write p95 | 1,004,964 ns | 967,986 ns | 0.963× |
| Bounded replay throughput | 13,149 ops/s | 21,992 ops/s | 1.673× |
| Bounded replay p95 | 85,244 ns | 53,005 ns | 0.622× |
| Maintained cold recovery | 17,243,799 ns | 4,666,742 ns | 0.271× |
| Steady probe peak RSS | 7,664 KiB | 6,444 KiB | 0.841× |
| Disk footprint | 994,930 bytes | 308,900 bytes | 0.310× |

Correctness passed for every trial. The checked-in policy requires correctness
and equal-or-better native results in every measured dimension; this evidence
passes with no failed cells. Segment-v3 LZ4 blocks and the bounded 4 MiB cache
account for the disk/residency result. Native `VYRNSI01` sequence values carry
canonical claim bytes, so replay is one contiguous range rather than an index
scan followed by point reads. The one-segment path streams ordered MVCC groups
without rebuilding a second ordered map, while optimized SHA-256 retains
post-open block authentication on cache misses. The fresh-probe design measures
that maintained representation rather than writer high-water memory.

This closes the first local performance gap, not the entire replacement case.
Fjall remains as a compatibility oracle while the scheduled and manually
dispatchable `vyrmKV benchmark` workflow reruns the canonical profile matrix
below and mixed/adversarial workloads establish regression bounds. The
repository may claim that native passes this exact baseline; it may not infer
general superiority over Fjall, SurrealDB, Qdrant, or other databases.

Evidence: [`2026-08-19-vyrmkv-baseline.json`](../eval/results/2026-08-19-vyrmkv-baseline.json).

## M3.5 maintenance confirmation

The 2026-08-21 bounded-compaction and negative-filter tree reran the canonical
standard profile with nine alternating isolated trials. Native passed every
strict cell at 1.151× write throughput, 1.702× read throughput, 0.788× write
p95, 0.611× read p95, 0.258× recovery time, 0.855× steady RSS, and 0.310× disk
relative to Fjall. Correctness passed in every raw trial.

A same-tree three-trial diagnostic failed once because native write p95 was
1.044× Fjall while the other six performance cells passed. Three trials leave
only 96 total write-latency samples for this profile, so that diagnostic is
recorded as undersampling rather than presented as promotion evidence. The
nine-trial result below is the repository's canonical M3.5 confirmation:
[`2026-08-21-vyrmkv-m35-standard.json`](../eval/results/2026-08-21-vyrmkv-m35-standard.json).

## Canonical profile matrix

A second pass uses nine isolated trials per profile. Every profile has at least
32 authoritative batch-latency samples per trial; a discarded 512-operation,
32-wide micro-profile had only 16 samples, which makes nearest-rank p95 equal
the single maximum and is not valid promotion evidence.

| Profile | Operations / batch | Reads / width | Write throughput | Read throughput | Write p95 | Read p95 | Recovery | RSS | Disk | Verdict |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Small-batch | 2,048 / 16 | 1,024 / 16 | 1.039× | 1.661× | 0.982× | 0.604× | 0.264× | 0.860× | 0.308× | Pass |
| Standard | 2,048 / 64 | 1,024 / 32 | 1.159× | 1.622× | 0.892× | 0.646× | 0.272× | 0.841× | 0.310× | Pass |
| Read-heavy | 4,096 / 64 | 4,096 / 64 | 1.096× | 1.670× | 0.912× | 0.600× | 0.304× | 0.882× | 0.314× | Pass |
| Sustained | 16,384 / 128 | 2,048 / 64 | 1.177× | 1.178× | 0.882× | 0.855× | 0.331× | 0.843× | 0.317× | Pass |

Throughput ratios above 1 favor native; latency, RSS, and disk ratios below 1
favor native. All cells preserve semantic correctness. The sustained result is
the combined effect of disk-resident blocks, compact `u32` record offsets, the
strict generation-based LRU, streaming one-segment scans, and self-serving
native sequence values; no benchmark threshold was relaxed.

Raw evidence:

- [`small-batch`](../eval/results/2026-08-19-vyrmkv-small-batch.json)
- [`standard`](../eval/results/2026-08-19-vyrmkv-standard.json)
- [`read-heavy`](../eval/results/2026-08-19-vyrmkv-read-heavy.json)
- [`sustained`](../eval/results/2026-08-19-vyrmkv-sustained.json)
- [`extended`](../eval/results/2026-08-20-vyrmkv-extended.json)

The three-trial extended cell raises the corpus to 70,000 operations while
retaining the sustained batch/read shape. Native recorded 47,729,952 encoded
WAL payload bytes and 140,547 memtable versions under its 64 MiB/524,288-version
limits, with zero automatic flushes, stalls, failures, or oversized batches.
It passed at 1.214× write throughput, 1.289× read throughput, 0.853× write p95,
0.771× read p95, 0.269× recovery, 0.673× steady RSS, and 0.317× disk. This proves
the larger bounded steady cell, not threshold-crossing burst performance.

The scheduled/manual workflow runs the original four profiles plus a 70,000-
operation extended profile with `--require-promotion` and preserves one
artifact per profile. This matrix spans
corpus size, batch size, read count, and range width. It still uses an append
then bounded-replay claim corpus; update/delete mixtures, long-duration soak,
and long-duration/remote repetition remain separate gates before Fjall code
removal. The finite mixed-mutation and migration gates are now recorded below.

## Physical mutation and migration gates

The checked-in `mixed_storage_soak` applies 20,000 deterministic operations
(4,558 inserts, 11,380 overwrites, and 4,062 deletes) over 2,048 keys. It forces
10 native/Fjall reopens and 8 native compactions and compares both stores to an
independent `BTreeMap` after every fifth batch. All three finish with 1,669
visible keys and SHA-256
`66f466b2d88a0c82bd9a2d929f8fd69312a26f28df102fb89e6e97273cb53f40`.
Evidence: [`m4-storage-mixed-soak.json`](evidence/m4-storage-mixed-soak.json).

The migration matrix exports one synced cross-keyspace Fjall snapshot into the
authenticated `VYRMIG01` stream, imports bounded native batches into an absent
staging sibling, verifies exact bytes and semantic reopen, and then cuts over
with two parent-synced renames. Tests interrupt and resume after export, import,
verification, both source/cutover rename gaps, source move, and cutover. They
also deny unknown keyspaces, corrupt/truncated archives, and rollback after
post-cutover native writes. See [`vyrmkv-migration.md`](vyrmkv-migration.md).

This is a physical ordered-key/value deletion result. It does not claim a typed
runtime entity-deletion contract, which must define relation and projection
effects before it can be added safely.

## AI-runtime profiles

The append/replay matrix is not representative of all frontier-runtime access.
The dedicated AI-read matrix publishes a cold immutable corpus, overwrites a
small control set in the active memtable, and independently measures current
hot hits, cold immutable hits, point misses, historical reads, and 32-key mixed
metadata fan-out. All five local five-trial cells pass exact correctness,
native-throughput, and native-p95 gates. The design audit, commands, bounded
claims, and versioned raw results are in
[`vyrmkv-fjall-ai-audit.md`](vyrmkv-fjall-ai-audit.md). The scheduled/manual
workflow reruns each mode and retains its raw artifact separately from the
general promotion matrix.
