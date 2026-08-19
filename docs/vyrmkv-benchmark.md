# vyrmKV promotion benchmark

Status: strict local M3 four-profile matrix passes; remote repetition and safe
backend migration remain open.

The benchmark runs Fjall and native `vyrmKV` in separate fresh child processes.
Both receive the same valid claim corpus, authoritative batch boundaries, and
bounded sequence replays. Writes are measured in isolated writer children. A
second fresh probe process measures cold reopen, maintained reads, and steady
peak RSS so write-time allocations do not contaminate the steady-state result.
Each child verifies the full semantic sequence and first/last claim after cold
reopen. Native additionally reports uncompacted recovery, compaction/GC
maintenance, maintained recovery, and maintenance peak RSS. The parent
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
| Authoritative write throughput | 68,946 ops/s | 76,132 ops/s | 1.104× |
| Authoritative write p95 | 1,181,103 ns | 907,860 ns | 0.769× |
| Bounded replay throughput | 12,932 ops/s | 13,096 ops/s | 1.013× |
| Bounded replay p95 | 86,969 ns | 85,851 ns | 0.987× |
| Maintained cold recovery | 15,055,831 ns | 4,522,076 ns | 0.300× |
| Steady probe peak RSS | 7,340 KiB | 6,652 KiB | 0.906× |
| Disk footprint | 983,868 bytes | 137,906 bytes | 0.140× |

Correctness passed for every trial. The checked-in policy requires correctness
and equal-or-better native results in every measured dimension; this evidence
passes with no failed cells. Segment-v2 LZ4 block compression accounts for the
large disk change. Sparse block-backed reads avoid decoding immutable contents
into a second ordered map, and the fresh-probe design measures that maintained
representation rather than writer high-water memory.

This closes the first local performance gap, not the entire replacement case.
Fjall remains as a compatibility oracle while the scheduled and manually
dispatchable `vyrmKV benchmark` workflow reruns the canonical profile matrix
below and mixed/adversarial workloads establish regression bounds. The
repository may claim that native passes this exact baseline; it may not infer
general superiority over Fjall, SurrealDB, Qdrant, or other databases.

Evidence: [`2026-08-19-vyrmkv-baseline.json`](../eval/results/2026-08-19-vyrmkv-baseline.json).

## Canonical profile matrix

A second pass uses nine isolated trials per profile. Every profile has at least
32 authoritative batch-latency samples per trial; a discarded 512-operation,
32-wide micro-profile had only 16 samples, which makes nearest-rank p95 equal
the single maximum and is not valid promotion evidence.

| Profile | Operations / batch | Reads / width | Write throughput | Read throughput | Write p95 | Read p95 | Recovery | RSS | Disk | Verdict |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Small-batch | 2,048 / 16 | 1,024 / 16 | 1.025× | 1.039× | 0.988× | 0.942× | 0.288× | 0.875× | 0.139× | Pass |
| Standard | 2,048 / 64 | 1,024 / 32 | 1.147× | 1.055× | 0.900× | 0.945× | 0.293× | 0.881× | 0.140× | Pass |
| Read-heavy | 4,096 / 64 | 4,096 / 64 | 1.177× | 1.034× | 0.872× | 0.958× | 0.320× | 0.921× | 0.141× | Pass |
| Sustained | 16,384 / 128 | 2,048 / 64 | 1.257× | 1.019× | 0.835× | 0.967× | 0.367× | 0.974× | 0.139× | Pass |

Throughput ratios above 1 favor native; latency, RSS, and disk ratios below 1
favor native. All cells preserve semantic correctness. The sustained RSS margin
came from storing sparse-index key ranges into the canonical segment buffer
instead of cloning thousands of heap keys.

Raw evidence:

- [`small-batch`](../eval/results/2026-08-19-vyrmkv-small-batch.json)
- [`standard`](../eval/results/2026-08-19-vyrmkv-standard.json)
- [`read-heavy`](../eval/results/2026-08-19-vyrmkv-read-heavy.json)
- [`sustained`](../eval/results/2026-08-19-vyrmkv-sustained.json)

The scheduled/manual workflow runs the same four profiles with
`--require-promotion` and preserves one artifact per profile. This matrix spans
corpus size, batch size, read count, and range width. It still uses an append
then bounded-replay claim corpus; update/delete mixtures, long-duration soak,
and migration rehearsal remain separate gates before Fjall code removal.
