# vyrmKV promotion benchmark

Status: first local baseline recorded; promotion denied.

The benchmark runs Fjall and native `vyrmKV` in separate fresh child processes.
Both receive the same valid claim corpus, authoritative batch boundaries, and
bounded sequence replays. Each child verifies the full semantic sequence and
first/last claim after cold reopen. Native additionally reports uncompacted
recovery, compaction/GC maintenance, maintained recovery, and maintenance peak
RSS. The parent alternates backend order and reports medians while retaining
every raw trial.

Run the checked-in workload:

```console
cargo run --release -p vyrm-store --example engine_benchmark -- \
  --trials 5 --operations 2048 --batch-size 64 \
  --reads 512 --read-width 32 \
  --output eval/results/2026-08-19-vyrmkv-baseline.json
```

The 2026-08-19 x86-64 Linux baseline is deliberately modest and is not a
universal database claim. Its median ratios are:

| Metric | Native versus Fjall | Result |
|---|---:|---|
| Authoritative write throughput | 0.987× | 1.3% behind |
| Authoritative write p95 | 0.941× | 5.9% lower latency |
| Bounded replay throughput | 1.133× | 13.3% ahead |
| Bounded replay p95 | 0.890× | 11.0% lower latency |
| Maintained cold recovery | 0.707× | 29.3% lower latency |
| Steady peak RSS | 1.088× | 8.8% higher |
| Disk footprint | 1.023× | 2.3% higher |

The strict policy requires correctness and equal-or-better results in every
measured dimension. This run therefore denies replacement: write throughput,
steady peak RSS, and disk footprint remain red. The decoded in-memory segment
representation is the leading RSS target; block-backed reads and sparse indexes
must be measured against the exact executor before replacing it. Disk work must
separate structural overhead from payload and measure larger corpora before any
format change. Fjall stays live until a repeated baseline passes rather than a
single favorable sample.

Evidence: [`2026-08-19-vyrmkv-baseline.json`](../eval/results/2026-08-19-vyrmkv-baseline.json).
