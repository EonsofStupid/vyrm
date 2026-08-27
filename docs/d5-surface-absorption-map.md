# D5 outward-surface absorption map

**Status:** authoritative implementation map as of 2026-08-27. This is derived
from the active workspace sources after D4. D5.1a through D5.1c, the D5.2a
client-only Connectome cutover, D5.4a embedded CLI boundary, D5.5 zero-bypass
gate, and D5.6 supervised topology are implemented. D5.1 through D5.4 remain
incomplete and this is not a cohesive-checkpoint claim.

## Invariant

RRD is the only authority. Connectome and daemon-mode CLI are clients.
Embedded/offline administration may enter through `RrdEngine`, but no outward
surface may compose `rrd-store`, `rrd-query`, `rrd-vector`, `rrd-estate`,
`rrd-cluster`, or `rrd-core` directly. Existing behavior is absorbed behind a
typed engine/public-contract operation before its old dependency is removed.
No file or capability is deleted merely to make the dependency test green.

## Current bypass inventory

### RRFlow CLI

The CLI's physical-store bypass is closed: production code depends on
`rrd-engine` and uses `RrdEngine` for bounded embedded and offline
administration. There is no second embedded handle or physical-store escape.
The remaining gap is authenticated daemon-mode selection and
embedded/daemon behavior parity for runtime commands.

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

The compiled `connectome-ui` runtime is client-only and has no physical RRD
dependencies. Legacy flight/cluster/connection source remains uncompiled as
migration input until equivalent governed contracts exist; those missing
views and writes return explicit `501` responses.

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

D5.1 remains open for reasoning/routing/trace summaries and cluster diagnostics
previously assembled inside Connectome. A
separate table-browser projection is not invented here: bounded
data rows continue to belong to the existing typed query plane. Those sections
must be absorbed behind the same engine contract before D5.2 can restore those
views without reintroducing physical read dependencies.

### D5.2a progress

The in-repository `connectome-ui` runtime is now a strict `rrd-client` and
`rrd-contract` consumer. It authenticates to one loopback RRD daemon, renews a
bounded session, validates `DiagnosticSnapshot`, executes scope-bound RRFlowQL,
and invokes only tools obtained from RRD's runtime-tool catalogue. Its normal
dependency graph contains no physical engine, store, query, vector, estate,
cluster, or core crate. A real-socket integration boots RRD, authenticates the
client, reads and validates a seeded diagnostic graph/model snapshot, executes
a query, and invokes `rrflow_service_status`.

Temporary embedded diagnostic mutations and projections that lack a public RRD
authority now return an explicit `501` with the required contract action; they
are not silently emulated in Connectome. Flights, causal-trace summaries,
cluster telemetry/history, and a first-class source-routing response therefore
remain D5.1/D5.3 work. The canonical changefeed and temporal graph remain
renderable through `/api/snapshot`. This closes the zero-bypass architecture
gate for Connectome, not the complete Connectome feature-parity gate.

### D5.4a progress

`rrflow-cli` no longer has production dependencies on `rrd-core` or
`rrd-store`. Ordinary embedded commands open the canonical project-bound
`RrdEngine`; its concrete persistent backend remains private. Runtime,
lifecycle, routing, reasoning, work-plan, query, recall,
projection, and invocation behavior crosses that handle. Exclusive migration,
format-upgrade, logical-archive, and backup operations are also named engine
administration entrypoints rather than direct store calls.

The security bootstrap and three estate executables are also owned by
`rrflow-cli` as thin adapters. Their policy/repository/driver construction and
all local authority openings live inside typed `RrdEngine` operations. The
estate and security crates expose physical/domain behavior but declare no
product authority executables.

The complete pre-cutover CLI suite passes unchanged: four binary unit tests,
two fixture tests, sixteen operator-surface tests, and nine runtime-experience
tests. The workspace architecture suite now enforces zero physical dependencies
for both Connectome and CLI. This closes the embedded CLI dependency boundary;
authenticated daemon-mode selection and embedded/daemon behavior parity remain
part of D5.4/G06-W04 rather than being inferred from this refactor.

### D5.6 progress

`rrflow dev up|status|logs|stop` is now an executable supervisor rather than a
reserved command name. It creates or verifies one dedicated instance, uses the
existing one-time security bootstrap with a persistent owner-only 256-bit
credential and catalogue-derived least-privilege grants, starts RRD first,
waits for `/v1/health/ready`, then starts the
authenticated client-only Connectome and waits for `/api/snapshot`. Its atomic
manifest records process IDs, exact executables, ports, logs, and paired
shutdown markers but never credential material. Logs are bounded on read and
both services must acknowledge graceful shutdown; timeout retains failed state
for diagnosis instead of silently claiming success.

The CI `topology-smoke` job builds the same binaries, runs `rrflow dev up`,
probes both endpoints, checks supervisor status, and runs `rrflow dev stop`.
A local black-box run passed the same sequence. `rrflow dev doctor` is now
20 passed, zero blocked, zero warnings. This closes D5.5/D5.6 infrastructure;
it does not close the remaining diagnostic/write parity or daemon-mode CLI
work in D5.1 through D5.4, and it does not change the CAP-01–CAP-20 audit.

## Exit evidence

- Embedded and daemon views produce the same logical diagnostic snapshot at
  the same read stamp.
- Every UI/CLI mutation uses an exact action, caller, idempotency coordinate,
  project authority, audit completion, and replay identity.
- Connectome and CLI have no production dependency on a physical RRD crate.
- The daemon topology has one process capable of opening the store.
- The existing UI behavior and recorded history survive absorption; a green
  dependency test alone is insufficient.
- Supervised-topology evidence includes the private credential reopen test,
  Connectome unit and real-socket tests, 33 CLI tests, a real RRD + Connectome
  up/status/logs/stop run, and the checked-in CI black-box smoke. The doctor is
  green at 20/0/0; that means the canonical development topology is runnable,
  not that the remaining D5 behavior parity or twenty engine capabilities are
  complete.
