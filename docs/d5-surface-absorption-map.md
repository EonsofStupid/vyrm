# D5 outward-surface absorption map

**Status:** authoritative implementation map as of 2026-08-25. This is derived
from the active workspace sources after D4. D5.1a through D5.1c are implemented
locally; D5.1 is not complete and this is not a cohesive-checkpoint claim.

## Invariant

RRD is the only authority. Connectome and daemon-mode CLI are clients.
Embedded/offline administration may enter through `RrdEngine`, but no outward
surface may compose `rrd-store`, `rrd-query`, `rrd-vector`, `rrd-estate`,
`rrd-cluster`, or `rrd-core` directly. Existing behavior is absorbed behind a
typed engine/public-contract operation before its old dependency is removed.
No file or capability is deleted merely to make the dependency test green.

## Current bypass inventory

### RRFlow CLI

`crates/rrflow-cli/src/main.rs` opens `PersistentEngine` and records invocation
rows directly. `command.rs` directly owns claim CRUD/recall/projections,
archive and migration functions, workflow/preflight/hook/query/reasoning
composition, and internal `rrd-core` wire values.

Required absorption:

| Existing CLI concern | Target owner/surface |
|---|---|
| claim assert/as-of/history/recall | task-level runtime tools over RRD session; expand only where the current 28-tool catalogue lacks the exact operation |
| preflight, hook, query, reasoning, routing, harness | existing engine runtime-tool catalogue; embedded calls engine, daemon calls `rrd-client` |
| invocation/effectiveness ledger | RRD audit/runtime event contract, not CLI-owned store writes |
| projection rebuild/ground/reset | new governed engine administration operations with typed result and audit |
| archive/backup/restore | existing public backup routes where semantics match; offline archive inspection/format recovery become explicit RRD administrative commands, never ordinary daemon-client calls |
| native/Fjall and application-format migration | engine-owned exclusive offline administration entrypoint with reopen/recovery fixtures |
| `dev doctor` | remains read-only and database-free |

### Connectome

`connectome-ui/src/main.rs` opens `PersistentEngine`. `lib.rs`, `flight.rs`,
`cluster.rs`, and `connections.rs` construct snapshots and mutations directly
from storage/query/vector/cluster/estate/runtime internals.

The current local API rows are:

- writes: `/api/flights`, `/api/demos/prompt-strength`,
  `/api/cluster/samples`, `/api/connections`;
- views: `/api/snapshot`, `/api/estate`, `/api/flights`,
  `/api/runtime/capabilities`, `/api/connections`, `/api/cluster/history`,
  `/api/changes`, `/api/runtime/events`, `/api/runtime/traces`,
  `/api/runtime/vector-artifacts`, `/api/runtime/schema`,
  `/api/runtime/retention`, `/api/runtime/query`, `/api/runtime/graph`,
  `/api/runtime/diff`, and `/api/route`.

Required absorption:

| Existing Connectome concern | Target owner/surface |
|---|---|
| capability and runtime-tool cards | public RRD endpoint/runtime/product catalogues (D0-D4 already provide the authority) |
| estate, changefeed, query, vector collections/search, audit | existing typed `rrd-client` methods |
| snapshot, temporal events, graph-at-time/diff, schema, retention, vector artifact catalogue, reasoning runs, routing, traces | one bounded diagnostic snapshot contract assembled by `rrd-engine` and served by RRD; Connectome only renders it |
| prompt-flight launch/replay/cohorts | engine-owned governed flight commands and append-only diagnostic events; provider processes remain explicit opt-in adapters |
| connection profiles | operator-local Connectome configuration may remain local, but it cannot become RRD data truth or open RRD stores |
| cluster telemetry | RRD cluster diagnostic contract; UI never reads transport journals directly |

## Dependency-critical slices

1. **D5.1 diagnostic read contract.** Freeze the bounded snapshot coordinates
   Connectome needs: instance/read stamp, catalogue/schema, model/table views,
   temporal/change cursors, graph lens, reasoning/routing/trace summaries,
   vector artifacts, estate, and capability dispositions. Implement it once in
   `rrd-engine`, transport it through `rrd-contract`/`rrd-server`, and add a
   Rust-client method.
2. **D5.2 Connectome read path.** Add explicit embedded and daemon connection
   modes. Replace read-only physical access with the D5.1 snapshot and existing
   client operations. Prove snapshot parity and remove only the dependencies
   made unreachable by that proof.
3. **D5.3 governed diagnostic writes.** Move flight, demo, routing, cluster
   sample, and other RRD mutations behind granular engine operations. Keep
   connection-profile storage explicitly operator-local. Prove denial,
   idempotency, audit, reopen, and replay.
4. **D5.4 CLI runtime modes.** Reuse the D4 authority selection and the
   engine-owned tool catalogue. Runtime commands become thin renderers over
   embedded or daemon invocation. Move exclusive recovery work behind a
   separately typed offline engine administration boundary.
5. **D5.5 zero-bypass gate.** Change the architecture test and development
   doctor from the frozen dependency list to zero Connectome/CLI physical
   dependencies. Run behavior parity before deleting the absorbed code.
6. **D5.6 topology.** Only after D5.5, implement the exact-argv command proxy,
   `rrflow dev up|status|logs|stop`, and the full-topology CI smoke.

### D5.1 progress

The first coherent diagnostic projection now exists as
`POST /v1/diagnostics/read`. It is a typed contract owned by `rrd-contract`,
assembled by `rrd-engine`, served by `rrd-server`, and consumed through an
`rrd-client` method. Its separate `diagnostics_read` grant covers readiness,
the product/runtime-tool catalogues, query-index and vector-collection
catalogues, estate state, bounded changefeed, bounded audit, the authoritative
schema registry, logical model summaries, and a temporal graph/difference
lens.

The snapshot captures claim sequence, control-journal sequence, authenticated
runtime manifest, runtime cursor, schema revision, and catalogue revision. RRD
compares the complete stamp before and after assembly and retries rather than
returning a mixed-time view. Memory, native, and Fjall-compatibility engines
now expose the same persistent control-journal head contract.

D5.1b adds schema revision and full schema data, stable record/relation/event
model summaries, and graph records/edges frozen at explicit valid-time and
known-cursor coordinates. The same response contains an exact comparison from
an explicit earlier cursor. Impossible future cursors are rejected rather than
silently clamped. Replay is bounded by a caller-declared ceiling of at most one
million changes; histories beyond that ceiling remain an explicit need for a
checkpointed graph projection rather than an unbounded diagnostic read.

D5.1c adds every live persisted snapshot lease and its derived semantic
retention pin. Because leases can change without advancing the data cursor or
control journal, their canonical set digest is a separate part of the verified
diagnostic read stamp; lease churn during assembly causes retry.

D5.1 remains open for reasoning/routing/trace summaries, vector artifact
catalogue, and cluster diagnostics currently assembled inside Connectome. A
separate table-browser projection is not invented here: bounded
data rows continue to belong to the existing typed query plane. Those sections
must be absorbed behind the same engine contract before D5.2 can remove
Connectome's physical read dependencies.

## Exit evidence

- Embedded and daemon views produce the same logical diagnostic snapshot at
  the same read stamp.
- Every UI/CLI mutation uses an exact action, caller, idempotency coordinate,
  project authority, audit completion, and replay identity.
- Connectome and CLI have no production dependency on a physical RRD crate.
- The daemon topology has one process capable of opening the store.
- The existing UI behavior and recorded history survive absorption; a green
  dependency test alone is insufficient.
