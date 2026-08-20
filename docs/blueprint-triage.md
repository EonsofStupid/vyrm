# Blueprint Triage — HTAP GraphRAG architecture document

> **Superseded architecture decision (2026-08-18).** The measurements and
> capability comparisons below remain historical evidence, but Fjall is no
> longer the permanent substrate decision. Vyrm will replace it with a native
> engine behind the existing conformance port. Compatibility behavior is a
> baseline to meet or beat, not a reason to discard AI-specific storage gains.

| Field | Value |
|-------|-------|
| Source | Operator-supplied research blueprint, received 2026-08-10 |
| Status | Triaged. Items below are adopted, deferred with a trigger, or rejected with evidence. |
| Authority | `SPEC.md` remains authoritative. Nothing here amends it except through its own revision process. |

## 1 · Framing correction — the "built-from-scratch" substrate already exists

The blueprint describes Vyrm as "a built-from-scratch, AI-optimized LSM engine
inspired by Fjall." Every substrate capability it attributes to that engine is a
shipped Fjall 3.x feature:

| Blueprint claim | Existing Fjall feature | Evidence |
|---|---|---|
| "highly tuned byteview-based memory model", "German string" `Slice` | `fjall-rs/byteview`: thin immutable zero-copy byte slice, Umbra-style German strings, ≤20-byte inlining | [byteview repo](https://github.com/fjall-rs/byteview), [Fjall 2.6 announcement](https://fjall-rs.github.io/post/fjall-2-6-byteview/) |
| "custom block format featuring sparse indexing, AI-native prefix truncation" | Fjall 3.0 block format: sparse indexing, prefix truncation, optional hash indexes | [Fjall 3.0 release](https://fjall-rs.github.io/post/fjall-3/), [block format post](https://fjall-rs.github.io/post/block-format/) |
| "isolated keyspaces … as column families", "cross-keyspace atomic WriteBatch" | Fjall keyspaces (3.x rename of partitions); single database-level journal makes cross-keyspace writes atomic | Verified in-session 2026-08-09 (docs fetch); relied on by `vyrm-store` §7.1 |
| "optimistic and single-writer concurrency models" | `OptimisticTxDatabase` / `SingleWriterTxDatabase` — the exact two types this project already chose between | In use in `vyrm-store/src/store.rs` |
| "key-value separation" for blob payloads | Fjall 3.0 integrated key-value separation with GC | [Fjall 3.0 release](https://fjall-rs.github.io/post/fjall-3/) |

Conclusion: the blueprint, read accurately, endorses the measured decision in
`SPEC.md` §2. The substrate accounts for 4 µs of a 135 µs read; there is no
measured deficiency; building the described engine would mean re-implementing
Fjall feature-for-feature. The fork condition stands as written: a measured
workload mismatch, per the Qdrant/Gridstore precedent. Until then, vyrm is the
semantic layer and Fjall is the substrate.

## 2 · Adopted

| Item | Where it lands | Why |
|---|---|---|
| Scope isolation: one keyspace per scope (`sys_global`, `proj_X`), surfaced to DataFusion as Catalog → Schema → Table | §9 tier/gate work; Clyffy multi-project | Context contamination is the memory-layer failure mode; physical keyspace isolation plus a per-request session context is the right enforcement shape. Read-only-ness of the global scope is enforced by the gate layer, not the substrate — Fjall has no read-only keyspace mode. |
| `spawn_blocking` + `RecordBatchStreamAdapter` + bounded mpsc for bridging synchronous iterators into async streams | `vyrmd` | Correct and standard; blocking the async executor on LSM I/O is a real failure mode. |
| Snapshot-retention and block-pinning contention warnings | Operational constraints, recorded when `vyrmd` lands | Real risks: a long-lived snapshot blocks GC; an `Arc`-pinned block shrinks effective cache. Route long analytical scans to Vortex segments; copy batches flagged for prolonged retention. |
| Routing stable ranges to Vortex + DataFusion rather than live keyspaces | Already running | This is the architecture journal's 2026-08-03 split (Fjall hot / Vortex derived / DataFusion compute), live on :4388/:4389 since 2026-08-04. The blueprint independently re-derives it. |

## 3 · Deferred, with triggers

| Item | Trigger |
|---|---|
| HNSW dense index, Tantivy BM25, RRF fusion (k=60) | `SPEC.md` §15 stands: claims are a bi-temporal relational workload. Trigger: an unstructured-object corpus (docs, transcripts, embeddings) at a scale where term-table routing measurably misses — measured, not assumed. |
| Dedicated embedding keyspace storing raw little-endian `f32` runs | Same trigger as above. The blueprint's guidance is sound for that data class when it exists. |
| Arrow IPC streaming to frontends | A frontend consumer exists. |

## 4 · Rejected or corrected

| Claim | Correction |
|---|---|
| Zero-copy transmute of LSM value bytes into Arrow arrays via serializer-side 64-byte padding | The serializer does not control final placement: values are packed into blocks after keys, so interior alignment of a value inside a (possibly compressed) block is not the serializer's to guarantee. The blueprint itself concedes row-format payloads need the copy loop into `MutableBuffer` — that honest path is the default. The workable zero-copy subset is a dedicated blob path for embedding vectors. |
| "64-byte alignment or undefined behavior" | Overstated. The Arrow spec *recommends* 64-byte alignment for SIMD; arrow-rs requires element-type alignment (4 bytes for `f32`) for typed views, and misalignment surfaces as a constructor panic, not silent UB. |
| Fixed-width `#[repr(C)]` claim records | Conflicts with the claim model: claims carry variable-length subjects, predicates, objects, and producers (`SPEC.md` §6). Fixed-width layouts apply to a future embedding/node payload class, not to claims. |
| "Built-from-scratch AI-optimized LSM engine" | See §1. `SPEC.md` §2 stands. |

## 5 · Effect on current work

None on the active task. The blueprint contains no material on symbol
extraction or ranking, which is where the measured gap is (5.61× achieved vs
~10× published for tree-sitter-graph approaches). The plan of record is
unchanged: swap extraction to tree-sitter, re-measure, then revisit ranking.
