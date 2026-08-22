# vyrmKV promotion benchmark

Status: the physical mixed-mutation soak, migration rehearsal, corrected local
general promotion, and dedicated AI-read matrix pass their bounded gates.

The benchmark runs Fjall and native `vyrmKV` in separate fresh child processes.
Both receive the same valid claim corpus, authoritative batch boundaries, and
bounded sequence replays. Writes are measured in isolated writer children.
Each backend is measured at the same three lifecycle boundaries: active,
cleanly reopened without explicit maintenance, and explicitly maintained then
reopened. Every current key is verified after both reopens. Footprint evidence
reports apparent and allocated bytes plus file-class attribution instead of
mistaking sparse logical length for physical allocation. The parent alternates
backend order, reports medians, and retains every raw trial.

Run the checked-in workload:

```console
cargo run --release --locked -p vyrm-store --example engine_benchmark -- \
  --trials 9 --operations 2048 --batch-size 64 \
  --reads 1024 --read-width 32 \
  --output eval/results/2026-08-22-vyrmkv-corrected-standard.json
```

The corrected 2026-08-22 x86-64 Linux result is one deliberately modest local
workload, not a universal database claim. Ratios above 1 favor native for
throughput; ratios below 1 favor native for latency, RSS, and footprint:

| Metric | Native versus Fjall | Gate |
|---|---:|---|
| Authoritative write throughput | 1.333× | Pass |
| Clean-reopen read throughput | 1.393× | Pass |
| Authoritative write p95 | 0.769× | Pass |
| Clean-reopen read p95 | 0.710× | Pass |
| Maintained read throughput | 1.248× | Diagnostic |
| Maintained read p95 | 0.808× | Diagnostic |
| Clean-reopen recovery | 0.160× | Pass |
| Maintained recovery | 0.230× | Diagnostic |
| Steady probe peak RSS | 0.936× | Pass |
| Clean-reopen allocated footprint | 0.915× | Pass |

Correctness and every strict performance cell passed in all nine aggregated
trials. The change set removes duplicated inline claims from current sequence
index writes, caches validated batch length, keeps the common one-version chain
inline, transfers decoded recovery ownership directly into the memtable, and
streams each WAL frame exactly once into the database open path. Native writes
are grouped by logical keyspace for ordered-tree locality. On Linux, only a
substantial first batch in the initial WAL receives one best-effort 1 MiB
`KEEP_SIZE` reservation; it never changes logical recovery length, grows, or
applies to successor WALs. The high-entropy AI footprint gate verifies that
this bounded reservation does not erase Vyrm's allocation result.

Fjall remains a compatibility and performance oracle. The scheduled and
manually dispatchable workflow runs with `--require-promotion`; remote and
sustained repetitions remain retirement gates. The repository may claim only
the bounded results recorded here; it may not infer general superiority over
Fjall, SurrealDB, Qdrant, or other databases.

Evidence: [`2026-08-22-vyrmkv-corrected-standard.json`](../eval/results/2026-08-22-vyrmkv-corrected-standard.json).

## Scale qualification remains open

The scheduled five-profile matrix is intentionally stricter than the canonical
fixture. On the same 2026-08-22 host, the final format-3 code passes the
small-batch and standard profiles. Read-heavy misses only raw peak RSS at
1.015× Fjall. Sustained misses raw peak RSS at 1.123×, despite winning write
throughput/p95, clean-reopen read throughput/p95, recovery, and allocated
footprint. The 70,000-operation extended profile records 1.181× RSS and 1.004×
allocated footprint; its other promotion cells pass. Backend-native maintained
reads are retained in every artifact but are diagnostic because the maintenance
actions differ.

Accordingly, the canonical and eight AI profiles are green, but the scheduled
matrix remains red at scale. Fjall compatibility retirement still requires a
more compact mutable byte representation plus repeated sustained/remote
evidence; the repository does not average those failures away.

## Invalidated legacy M3/M3.5 evidence

The 2026-08-21 bounded-compaction and negative-filter tree reran the former
canonical standard profile with nine alternating isolated trials. Native passed every
strict cell at 1.151× write throughput, 1.702× read throughput, 0.788× write
p95, 0.611× read p95, 0.258× recovery time, 0.855× steady RSS, and 0.310× disk
relative to Fjall. Correctness passed in every raw trial.

A same-tree three-trial diagnostic failed once because native write p95 was
1.044× Fjall while the other six performance cells passed. Three trials leave
only 96 total write-latency samples for this profile, so that diagnostic is
recorded as undersampling rather than presented as promotion evidence. The
nine-trial result below is retained as historical diagnostic evidence only:
[`2026-08-21-vyrmkv-m35-standard.json`](../eval/results/2026-08-21-vyrmkv-m35-standard.json).

Those runs applied compaction/GC to native before its read and footprint probe
without an equivalent Fjall maintenance phase. They also summed apparent file
length, counting Fjall's sparse 64 MiB journal as physically consumed space.
They therefore cannot support promotion, even though their internal semantic
checks remain useful.

## Legacy profile matrix (not promotion evidence)

A second pass uses nine isolated trials per profile. Every profile has at least
32 authoritative batch-latency samples per trial; a discarded 512-operation,
32-wide micro-profile had only 16 samples, which makes nearest-rank p95 equal
the single maximum and is not valid promotion evidence.

| Profile | Operations / batch | Reads / width | Write throughput | Read throughput | Write p95 | Read p95 | Recovery | RSS | Apparent bytes | Status |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Small-batch | 2,048 / 16 | 1,024 / 16 | 1.039× | 1.661× | 0.982× | 0.604× | 0.264× | 0.860× | 0.308× | Legacy |
| Standard | 2,048 / 64 | 1,024 / 32 | 1.159× | 1.622× | 0.892× | 0.646× | 0.272× | 0.841× | 0.310× | Legacy |
| Read-heavy | 4,096 / 64 | 4,096 / 64 | 1.096× | 1.670× | 0.912× | 0.600× | 0.304× | 0.882× | 0.314× | Legacy |
| Sustained | 16,384 / 128 | 2,048 / 64 | 1.177× | 1.178× | 0.882× | 0.855× | 0.331× | 0.843× | 0.317× | Legacy |

Throughput ratios above 1 favor native; latency, RSS, and apparent-byte ratios
below 1 favor native. All cells preserve semantic correctness, but asymmetric
maintenance and sparse-file accounting invalidate their performance verdicts.
The sustained result is the combined effect of disk-resident blocks, compact
`u32` record offsets, the
strict generation-based LRU, streaming one-segment scans, and self-serving
native sequence values (the former inline format, no longer emitted); no
benchmark threshold was relaxed in that legacy harness.

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
It reported 1.214× write throughput, 1.289× read throughput, 0.853× write p95,
0.771× read p95, 0.269× recovery, 0.673× steady RSS, and 0.317× apparent bytes.
It preserves a larger-workload diagnostic, not valid promotion evidence.

The scheduled/manual general workflow preserves one artifact per profile and
uses `--require-promotion`; the corrected local canonical execution now passes.
The legacy matrix spans corpus size, batch size, read count, and range width. It
still uses an append
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
metadata fan-out. It runs repeated-byte, structured JSON,
deterministic-entropy, and embedding-like payloads as applicable. All eight
local five-trial cells pass exact correctness, native-throughput, native-p95,
and clean-reopen allocated-footprint gates. The design audit, commands, bounded
claims, and versioned raw results are in
[`vyrmkv-fjall-ai-audit.md`](vyrmkv-fjall-ai-audit.md). The scheduled/manual
workflow reruns each mode and retains its raw artifact separately from the
general promotion matrix.
