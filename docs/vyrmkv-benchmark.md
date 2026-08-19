# vyrmKV promotion benchmark

Status: strict local M3 baseline passes; broader promotion evidence remains open.

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
dispatchable `vyrmKV benchmark` workflow reruns this exact strict gate and
larger, mixed, adversarial, and sustained workloads establish regression
bounds. The repository may claim that native passes this exact baseline; it may
not infer general superiority over Fjall, SurrealDB, Qdrant, or other databases.

Evidence: [`2026-08-19-vyrmkv-baseline.json`](../eval/results/2026-08-19-vyrmkv-baseline.json).
