# vyrm — Kernel Specification v0

| Field | Value |
|-------|-------|
| Status | Draft. Pre-release. Subject to revision without migration guarantees. |
| Current compatibility substrate | Fjall 3.1.8; transitional until the Vyrm-native engine reaches conformance and performance parity |
| Scope | Tier 0 only: development-time persistence and recall for a single operator |
| Supersedes | Nothing. Extends `docs/architecture-journal.md` and `automaton/docs/00-abstract-layer.md`. |

## 1 · Conventions

### 1.1 Requirement levels

The keywords MUST, MUST NOT, SHOULD, SHOULD NOT, and MAY are to be interpreted as
described in RFC 2119. A statement without one of these keywords is descriptive,
not normative.

### 1.2 Terminology

This vocabulary is controlled. Each concept has exactly one term. Synonyms listed
under "Not" MUST NOT appear in vyrm source, documentation, or API surface.

| Term | Definition | Not |
|------|------------|-----|
| **claim** | A bi-temporal assertion of the form subject–predicate–object with provenance. | fact, record, entry, assertion, memory |
| **subject** / **predicate** / **object** | The three components of a claim. | key, field, attribute |
| **valid time** | The interval during which a claim holds in the modelled domain. Delimited by `valid_from` and `valid_to`. | event time, real time |
| **transaction time** | The instant the kernel recorded a claim. Field: `tx_time`. | write time, ingest time |
| **retirement** | Closing a claim's valid-time interval by setting `valid_to`. A retired claim remains readable. | deletion, expiry, invalidation |
| **supersession** | Replacement of a claim by a later claim over the same subject and predicate. | overwrite, update |
| **sequence** | The monotonic write ordinal assigned by the substrate. | seq, offset, LSN, index |
| **watermark** | A recorded sequence position. | checkpoint, cursor, marker |
| **sequence index** | The mapping from an append sequence to the claim key written at it. | event log, WAL, journal |
| **projection** | A read model derived from claims. Never authoritative. | view, graph, materialization, cache |
| **grounding** | Full recomputation of a projection from the claim log, differenced against the incrementally maintained projection. | validation, reconciliation, repair |
| **differential** | The claim set between two watermarks, identified by content digest. | diff, delta, changeset (lowercase) |
| **change set** | A differential that has crossed a tier gate and carries a signature. | patch, merge, promotion payload |
| **tier** | A deployment and trust level. Tiers are ordered. | level, ring, stage |
| **gate** | The ordered predicate set governing a tier crossing. | check, guard, filter |
| **promotion** | A claim crossing a tier boundary by satisfying a gate. | publish, sync, escalation |
| **recall** | Retrieval of claims for supply to a model context. | retrieval, lookup, fetch, search |
| **recall set** | The result of a recall: claims plus provenance and a content digest. | context block, payload |
| **substrate** | The persistence implementation behind the Vyrm storage port; currently the transitional Fjall adapter, ultimately Vyrm-native. | engine, backend, database, wrapper, store |
| **keyspace** | A logical isolated ordered key-value space. The compatibility adapter maps it to a Fjall 3.x keyspace. | table, column family, bucket |
| **durability class** | The persistence policy assigned to a keyspace. | sync mode, flush policy |
| **port** | A trait the kernel defines for an external implementation. | interface, hook |
| **adapter** | An implementation of a port. | driver, plugin, binding, client |
| **operator** | The human directing the system. Aligned with `docs/reality-audit.md`. | user, developer, customer |
| **seat** | An entitled operator workspace. Aligned with `automaton/02-session-model.md`. | account, instance, node |
| **producer** | The actor that authored a claim, and the model it acted on behalf of. | author, source, writer |
| **access record** | Telemetry describing a read of a claim. | audit log, trace, hit |

### 1.3 Units

Latency is expressed in microseconds (µs) or milliseconds (ms) with the unit
stated. Throughput is expressed in operations per second (op/s). Measured values
MUST cite the date and host on which they were obtained.

## 2 · Position

vyrm owns both its semantic contract and the target persistence architecture.
The current release runs over a commodity compatibility substrate while a
Vyrm-native substrate is built behind the same port. Fjall MUST NOT be described
as the permanent architecture.

Measured on `warden-devstation-01`, 2026-08-09:

| Path | Latency | Share of read |
|------|---------|---------------|
| Transport only: lock, framing, JSON; no substrate access | 0.131 ms | 97.2 % |
| Transport plus one substrate point read | 0.135 ms | Substrate: 0.004 ms / 2.8 % |
| Transport plus three inserts and two fsync | 0.562 ms | Durability: 0.431 ms |

The compatibility substrate accounts for 4 µs of a 135 µs read in this historical
point-read measurement. That result remains a baseline for the native engine; it
does not veto workloads or structures optimized for AI runtime persistence.

The Vyrm-native engine MUST preserve the cross-adapter conformance differential
and MUST meet or beat the compatibility substrate on representative runtime
workloads: latency distribution, throughput, durability, crash recovery, and
memory use. Performance claims require retained measurements.

Existing Fjall and external-system measurements remain comparison evidence.
They are not architectural ownership boundaries and do not prohibit native
implementation of capabilities covered by the project's recorded permissions
and provenance.

## 3 · Kernel API

The surface is closed. Tier policy varies; the surface MUST NOT.

```text
append_batch(claims[])          -> { first_sequence, last_sequence }
assert(claim)                   -> claim_id
as_of(subject, predicate, t)    -> claim | none
current(subject, predicate, now)-> claim | none
history(subject, predicate)     -> iterator<claim>
observe(reader, subject, pred)  -> ()
promote(claim_id, tier, evidence) -> change_set | denial
gate(tier)                      -> policy
```

Per `docs/architecture-journal.md`, the deployment profile may change; the logical
contracts MUST NOT. This property is what makes the kernel singular across tiers.

The kernel MUST NOT read a clock. Every operation requiring the current instant
takes it as a parameter, so that results are reproducible and tests are
deterministic.

## 4 · Client topology

The 0.131 ms transport floor in §2 is a property of a Node client over TCP, not of
the kernel. The recall and claim-write paths MUST NOT incur it.

```text
                        ┌──────────────────────────────┐
  latency-sensitive     │  vyrm-core (Rust, in-process)│   ~4 µs
  Clyffy / automaton ───┤  linked directly             │
                        └──────────────────────────────┘
                                     │
  tooling and inspection    vyrmd over Unix domain socket   ~131 µs
  (npm scripts, CLI)        non-latency-sensitive only
```

Components on the claim-write or recall path MUST link the kernel directly.
Components on that path MUST NOT use remote procedure calls. RPC is reserved for
cross-process tooling, cross-tier promotion, and inspection.

### 4.1 Relationship to decision D1

`automaton/PLAN.md` records **D1: Node ESM (`.mjs`) in `engine/`**. D1 stands and
is not superseded. The adapter is napi-rs: automaton retains its Node engine and
calls `vyrm-core` in-process through a native module.

A Unix-domain-socket daemon remains the fallback should napi-rs prove
incompatible with Zellij pane supervision. TCP with newline-delimited JSON is
excluded from this path.

## 5 · Module structure

```text
vyrm-core     claims, key encoding, bi-temporal resolution, supersession
   ▲
vyrm-store    substrate adapter: keyspaces, durability classes, batch commit
   ▲
vyrm-graph    projection, incremental rebuild, grounding, differentials
   ▲
vyrm-gate     tier policy, promotion, change-set signing
   ▲
   ├── vyrm-node   napi-rs adapter for automaton
   ├── vyrm-cli    operator surface
   └── vyrmd       daemon: socket tooling and cross-tier promotion
```

Dependencies MUST point inward only. `vyrm-core` MUST NOT depend on a transport,
a tier policy, or the substrate, and MUST NOT expose substrate types in its public
API. This is the law `automaton/docs/00-abstract-layer.md` already applies at the
session layer — the abstract layer is SSOT and adapters are never SSOT — applied
one level below.

**Modularity criterion:** a module satisfies this specification if and only if it
compiles and passes its tests with every outward module removed. This is
verifiable by `cargo tree` and MUST be checked in CI.

## 6 · Claim model

A claim carries two independent timelines:

- **valid time** — when the claim holds in the modelled domain
- **transaction time** — when the kernel recorded it

Superseded claims MUST be retired, not deleted. Retirement makes staleness
representable, which is the mechanism by which this specification prevents drift.

```json
{
  "subject": "wp3",
  "predicate": "status",
  "object": "in_progress",
  "valid_from": 1786000000000,
  "valid_to": null,
  "tx_time": 1786000000123,
  "producer": { "actor": "agent:clyffy", "on_behalf_of": "claude-opus-5", "session": "s-7451e063" },
  "confidence": 0.9,
  "supersedes": null,
  "signature": null,
  "tier": "local",
  "promotion_state": "unpromoted"
}
```

### 6.1 Key encoding

```text
c/{subject}\x00{predicate}\x00{inv_valid_from:020}\x00{inv_tx_time:020}

inv(t) = u64::MAX - t
```

Inverted encoding causes the newest version to sort first under a
byte-lexicographic iterator. A resolution therefore requires one seek.

Both timelines MUST participate in the key. An encoding over valid time alone
assigns two claims about the same valid-time point the same key, so a later
correction destroys the claim it corrects while the sequence watermark counts
both — leaving the watermark inconsistent with stored state and any
sequence-derived reconstruction incorrect. Within one `valid_from`, inverted
transaction time orders the most recently recorded version first, so resolution
returns current knowledge unchanged.

Subjects and predicates MUST reject the byte `0x00`. This is the sole encoding
invariant, and it MUST be enforced at construction and on the deserialization
path.

Verified against Fjall 3.1.8 on 2026-08-09 (`scratchpad/keytest`):

- prefix scan returned newest-first ordering
- resolution at valid-time boundaries 99, 100, 150, 200, 250, 300, 9999 each
  returned the correct version
- the `0x00` separator gave exact prefix isolation; the adversarial neighbours
  `wp3x/status`, `wp3/statusx`, and `wp/status` produced no leakage
- reverse iteration via `DoubleEndedIterator` returned oldest-first ordering

### 6.2 Resolution

Resolution selects the first candidate, in newest-first order, that is valid at
the requested instant. It MUST NOT select the first candidate unconditionally: the
newest claim with `valid_from <= t` may have been retired without a successor, in
which case the correct result is none.

Valid-time intervals are half-open, `[valid_from, valid_to)`. A successor
beginning at the instant of retirement is therefore unambiguous.

## 7 · Instrumentation

Removal of a claim, predicate, or subsystem MUST be decidable by query rather than
by argument.

- Every claim MUST carry a `producer`.
- Every read MUST append an access record: `{ reader, subject, predicate, at }`.
- Removal candidates are then derived: predicates with no access record within a
  stated interval.

### 7.1 Durability classes

Durability accounts for 0.431 ms of a 0.562 ms claim write (§2). Telemetry MUST
NOT incur that cost.

| Keyspace | Durability class | Rationale |
|----------|------------------|-----------|
| `claims` | `SyncAll` | Authoritative. Loss is not acceptable. |
| `sequence_index` | `SyncAll` | Authoritative. Written in the claim's own transaction. |
| `access` | Buffered, periodic flush | Telemetry. Loss on crash is acceptable. |
| `meta` | `SyncAll` | Watermarks and idempotency keys. |

The sequence index maps an append sequence to the claim key written at that
sequence, and is what makes §8.2 and §8.4 answerable. Its entry MUST be written
in the transaction that writes the claim, so that the index cannot diverge from
the watermark under termination.

Fjall persists writes across keyspaces in a single database-level journal, so
writes requiring mutual atomicity retain it across durability classes.

## 8 · Flush, rebuild, grounding, differentials

### 8.1 Flush

Per-claim fsync costs 0.431 ms, bounding throughput at approximately 1,800 op/s
and precluding amortization. Immediate durability and high throughput are not
simultaneously satisfiable; the resolution is group commit under a declared
latency bound.

```text
flush when   batch full   OR   flush_delay_us elapsed   OR   flush() invoked
```

`flush_delay_us` is the sole durability-throughput parameter and MUST be declared
in configuration rather than inferred at runtime.

Group commit yields benefit only when writers reach the commit point concurrently.
It therefore depends on §11 correction 1; under a global mutex, concurrent commit
points do not occur and the parameter has no effect.

`flush()` is an explicit durability barrier. It SHOULD be invoked at task
completion, before promotion, and before snapshot capture.

#### 8.1.1 Producer patterns

Measured on ext4, 2026-08-10. The interval amortizes only when claims arrive
while a batch is open. It cannot batch a producer that awaits each claim.

| Producer | Pattern | Cost per claim |
|----------|---------|----------------|
| Continuous, does not await durability | `submit` repeatedly; `flush` at a task boundary | 0.0055 ms |
| Requires durability before proceeding | `submit` then `flush` | 0.438 ms |
| Requires durability before proceeding | `submit`, then block on commit progress | 21.264 ms at a 20 ms interval |

A producer requiring durability before proceeding MUST call `flush` and MUST NOT
block on reported commit progress. `flush` commits immediately, and its cost is
independent of `flush_delay`; blocking on progress waits out the full interval
for a batch that will never fill.

Under sustained load the interval is not reached, because `max_batch` triggers
first; three interval settings produced identical throughput at a mean batch of
500. This corresponds to the self-clocking behaviour reported for group commit
above a device-set load threshold. The interval therefore governs sparse arrival
only, and the guidance above is what makes it safe there.

### 8.2 Rebuild

Projections are derived and MUST NOT be treated as authoritative.

```text
trigger   (current_sequence - projection_watermark) >= rebuild_interval  OR  idle
apply     claims in (projection_watermark, current_sequence]
commit    advance projection_watermark atomically with the projection write
```

The watermark MUST advance in the same transaction as the projection write, so
that a crash mid-rebuild replays the interval rather than skipping it.

### 8.3 Grounding

Grounding is full recomputation of a projection from the claim log, differenced
against the incrementally maintained projection. Divergence MUST halt the
projection. It MUST NOT be silently repaired.

```text
ground()   recomputed  = project_from_claims(as_of = now)
           differential = compare(recomputed, incremental_projection)
           empty     -> emit grounded { at, sequence, digest }
           non-empty -> halt, emit divergence { differential }, quarantine projection
```

A projection that silently diverges from its source log is the precise failure
this specification exists to prevent. The kernel MUST therefore detect that
failure in itself before claiming to detect it elsewhere.

Grounding is O(claims) and SHOULD run on a longer interval than rebuild.

### 8.4 Differentials

A snapshot is the claim set at a watermark. A differential is the claim set
between two watermarks, identified by content digest.

A differential and a change set MUST be the same object. The analytical path
consumes it unsigned; a tier gate consumes it signed. A second representation
would constitute a second lineage and MUST NOT be introduced.

```text
differential(from_watermark, to_watermark) -> { claims[], from, to, digest, signature? }
```

## 9 · Tiers and gates

Tiers correspond to the progression recorded in `docs/architecture-journal.md`
(operator, workspace, enterprise).

```text
tier 0  local     one operator, one substrate database, no hosted dependency
   │    gate: verification and signature
tier 1  primary   merged operator branches, signed change sets, explicit conflict decisions
   │    gate: catalog admission and provenance completeness
tier 2  tenant    shared analytical segments, cataloged snapshots, governance
```

A claim MUST NOT cross a tier boundary except by promotion. `promote()` evaluates
the target tier's gate against the claim's evidence and, on success, emits a
content-addressed signed change set.

Per the source-of-truth boundaries in `docs/architecture-journal.md`, a claim
derived from the analytical layer MUST carry its source snapshot and provenance
before promotion into an authoritative layer. This specification enforces that
rule mechanically rather than by convention.

Cross-tier reads MUST be pull-based: the lower tier requests and the higher tier's
gate decides. No tier may become authoritative for another implicitly.

### 9.1 Gate evaluation

```text
gate = ordered [ predicate ]
predicate: evaluate(claim, evidence) -> pass | fail { reason }
```

- A tier with no configured gate MUST deny. An unconfigured gate MUST NOT permit.
- Evaluation MUST return all failing predicates, not the first.
- Every evaluation, permitting or denying, MUST be recorded as a claim, so that
  the reason a claim remains unpromoted is answerable by query.
- Promotion above tier 0 MUST NOT be automatic. `docs/architecture-journal.md`
  requires explicit conflict decisions.

Candidate predicates, derived from `docs/architecture-journal.md`:
provenance completeness, verification state, confidence threshold, signature
presence, absence of unresolved conflict at the target tier, and supersession-chain
coherence. The predicate set and its thresholds are open and are not settled by
this revision.

### 9.2 Tier fields at tier 0

Every claim MUST carry `tier` and `promotion_state` from the first revision, with
values `local` and `unpromoted`. Gate evaluation at tier 0 is unconditional.

`automaton/docs/11-theming-scale-architecture.md` states the governing principle:
if a later capability can only be delivered by rewriting existing records, the
architecture has failed. Promotion MUST therefore be a state transition and MUST
NOT require a migration.

## 10 · Recall

Recall is substrate- and provider-agnostic at the semantic layer. Providers are
adapters, consistent with `automaton/docs/00-abstract-layer.md`.

```text
recall(query, budget) -> recall_set { claims[], digest, token_estimate }
   │
   ├── frontier adapter   renders to a token-bounded context block, supplied as a
   │                      message or tool result. The context window is the only
   │                      available injection surface.
   │
   └── local adapter      MAY render the same block, or MAY inject below the token
                          layer via prefix-cache reuse, logit biasing, or adapter
                          weights.
```

The asymmetry between adapters is a design constraint, not a configuration option.
A frontier model's reasoning is not externally addressable, so recall MUST
materialize into tokens for that adapter. A recall set MUST therefore be defined
as semantic content with provenance, and MUST NOT be defined as a rendered prompt
string. Rendering is the responsibility of the adapter.

## 11 · Substrate corrections

Derived from audit of `native/fjall-vortex-runtime/src/main.rs`, 2026-08-09.

1. **Sequence allocation MUST occur inside the transaction.** `main.rs:115`
   performs a read-modify-write outside the transaction. It is correct only
   because a global mutex serializes it. This is a correctness prerequisite for
   any concurrency work, not an optimization.
2. **Sequence increment MUST use `checked_add`.** `saturating_add` converts
   overflow into silent key reuse.
3. **A claim write MUST issue one fsync.** `main.rs:138` commits with `SyncAll`;
   `main.rs:141` persists with `SyncAll` again.
4. **Reads MUST NOT acquire the write lock.** Fjall is internally synchronized for
   multi-threaded access; the external mutex protects only correction 1.
5. **`append_batch` precedes concurrency work.** Batching amortizes both the
   transport floor and the fsync without altering the writer model and without
   optimistic-concurrency retry logic.
6. **The accept path MUST bound its queue.** `main.rs:591` spawns one unbounded
   thread per connection. Backpressure signalling requires a bounded queue.

## 12 · Acceptance

The current runtime emits `"persistMode": "sync_all"` and `"authority": "fjall"`
on every write with no test verifying either. A durability property MUST NOT be
asserted in an API response unless a test verifies it.

| Property | Verification |
|----------|--------------|
| Durability | Process termination mid-batch; committed claims present, uncommitted absent |
| Resolution correctness | Property test across supersession and retirement boundaries |
| Prefix isolation | Adversarial subject and predicate neighbours |
| Throughput | Recorded load measurement, so stated figures are measurements |
| Removal candidacy | A predicate with a recent access record is never a candidate |
| Modularity | `cargo tree` shows no substrate or transport dependency in `vyrm-core` |

## 13 · Trigger promotion

Every trigger ships manual and instrumented before it ships automatic. Automatic
policy MUST be derived from recorded manual invocations.

```text
stage 1   operator invokes explicitly: recall, flush, rebuild, ground, snapshot
          every invocation recorded
stage 2   analyse recorded invocations against outcomes and cadence
stage 3   promote a trigger to automatic on event, interval, or threshold
```

A trigger MUST NOT be automated before its recorded manual invocations
demonstrate the automation criterion.

### 13.1 Effectiveness ledger

Token reduction is a measurement and MUST NOT be stated as a property without one.
Every recall MUST record:

```json
{
  "trigger": "manual | event | interval | threshold",
  "query": "...",
  "claims_returned": 7,
  "tokens_emitted": 1400,
  "baseline_tokens": 5000,
  "baseline_mode": "unstructured_context",
  "provider": "frontier:claude | local:...",
  "outcome": "accepted | corrected | discarded | unknown"
}
```

`baseline_tokens` MUST be obtained from a controlled comparison against
unstructured context on the same query. Without it, the reduction is unverified.

`outcome` is the signal from which trigger policy is derived: a recall that
consistently precedes `corrected` is being invoked at the wrong point.

### 13.2 Content-addressed objects

Large objects — files, traces, transcripts — MUST be stored once by digest and
referenced by digest in claims and recall sets. They MUST NOT be inlined.
Retransmission of an unchanged object then costs a digest rather than a payload.
This is the mechanism used for differentials in §8.4: identity by content digest,
transfer of the differential only.

## 14 · Consumer

The first consumer is Clyffy, acting as task executor.

```text
operator ──> Clyffy ──> frontier model
                │       (terminal, browser, or application)
                │
                └── vyrm-core, linked in-process
                    writes: decisions, findings, task state, provenance
                    reads:  current(), as_of(), history()
```

Clyffy reaches frontier models through automaton's adapter layer, in which the
abstract session and event API is SSOT and provider CLIs are adapters. vyrm is the
claim store those sessions read from and write to.

Clyffy writes claims continuously during task execution, which is why §4 excludes
RPC from that path, and why `producer` is mandatory: a claim written by an
executor on behalf of a model MUST be attributable to both.

## 15 · Out of scope for v0

Vector indexes and rank fusion. Claims are short, structured, and number in the
thousands, which is a bi-temporal relational workload rather than a
similarity-search workload. Rank fusion is a query-layer concern, is decoupled
from this schema, and is introduced when a corpus justifies it.
