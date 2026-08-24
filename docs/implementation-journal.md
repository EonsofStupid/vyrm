# Vyrm implementation journal

This is the durable engineering record for dependency-critical RRD/Vyrm work.
It complements, but does not replace, the authenticated runtime and control
journals stored by Vyrm itself.

## Journal protocol

Every strict slice receives one entry before it is declared complete. Entries
are append-only in normal development and must include:

- the UTC date and landed commit (or `pending` before the checkpoint exists);
- the exact capability and dependency gate addressed;
- the production files and public contracts changed;
- verification commands and their observed result;
- evidence that supports the claim, including failure/restart coverage;
- known limits and the next dependency-critical slice.

An entry must not promote a bounded test into a product-wide claim. If later
evidence invalidates an entry, append a correction that links back to it rather
than silently rewriting the historical conclusion. Git remains the source of
truth for the exact diff; this journal is the reviewable narrative and evidence
index.

## 2026-08-23 — F0/F1 public contract and operable recovery baseline

- Commit: `b7c925c` (`feat: establish RRD persistence and recovery contracts`).
- Capability: froze transport-neutral RRD resources and established
  authenticated logical archives, backup catalogues, restore-to-new-root,
  resumable native-format migration, and the first cross-version recovery row.
- Evidence: corruption, retry, reopen, source-drift, rename-boundary, and
  legacy-archive recovery tests are recorded in `STATUS.md` and the F0/F1
  sections of `full-stack-gap-ledger.md`.
- Limit: object payload closure, signing/encryption, retention automation, and a
  broader released-version recovery matrix remain open.
- Next gate: an out-of-process RRD server with durable sessions and client
  transaction semantics.

## 2026-08-23 — F2 loopback transaction server

- Commit: `3720461` (`feat(rrd): ship async loopback transaction server`).
- Capability: shipped the first async loopback RRD process with health,
  readiness, capability negotiation, durable session/transaction coordination,
  idempotent claim commit, deadlines, bounded envelopes, and graceful server
  shutdown.
- Evidence: real-socket and real-binary tests cover restart, token rotation,
  expiry, quota, malformed/oversized requests, abort/close, disconnect/retry,
  concurrent convergence, collision, and remote-bind denial.
- Limit: cancellation, general read-your-writes, administration, result/time
  bounds, metrics, and released-version clients keep F2 open.
- Next gate: persistent estate authority and crash-resumable reconciliation.

## 2026-08-23 — F3 persistent estate authority and reconciler

- Commits: `a70a986`, `b469ecd`, and `b66b6dc`.
- Capability: added a bounded persistent estate aggregate, desired/observed
  generations, idempotency bindings, fenced leases, receipts, activity
  evidence, one-boundary reconciliation, and the stable public read projection
  consumed by RRD and Connectome.
- Evidence: native reopen and hash-chain tests cover lost acknowledgement,
  lease takeover, stale-worker fencing, supersession, and authoritative panel
  projection. Synthetic Connectome data is labeled as fallback.
- Limit: a production effect driver and control-plane process crash matrix were
  still required at this checkpoint.
- Next gate: a trusted no-shell process driver with restart-safe ownership.

## 2026-08-23 — F3 trusted local process driver

- Commit: `eaf91de` (`feat(rrd): add verified local process driver`).
- Capability: added an operator-trusted deployment catalogue, executable
  SHA-256 authentication, typed arguments, cleared environment, durable
  PID/start-time/executable ownership, idempotent effect replay, and fail-closed
  signaling.
- Verification: `cargo test -p rrd-estate --locked` and the initial
  `rrd-server` local-driver integration tests passed on Linux; strict clippy
  passed at the checkpoint.
- Evidence: the test launches a real RRD child, reopens controller objects and
  storage between boundaries, preserves PID across same-operation replay,
  stops the child without deleting its data, and denies a forged PID identity.
- Limit: controller-process kill injection, graceful child shutdown, packaging,
  operator-authorized mutations, and Windows/macOS qualification remain open.
- Next gate: kill the actual controller process at every durable/effect
  transition and prove convergence.

## 2026-08-23 — F3 controller crash-recovery matrix

- Commit: `cc55d8c` (`feat(rrd): qualify controller crash recovery`).
- Capability: added the one-step `rrd-estate-controller` process and a
  black-box harness that kills that process after each start and stop boundary,
  including after the external effect but before the durable `applied` record.
- Verification:
  `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test -p rrd-server --test local_estate_driver controller_process_kill_matrix --locked -- --nocapture`
  passed; `cargo clippy -p rrd-estate -p rrd-server --all-targets --locked -- -D warnings`
  passed; `git diff --check` passed.
- Evidence: start recovery retains the same live child PID when the first
  acknowledgement is lost; stop recovery observes the already-stopped child,
  records the operation exactly once, retains its data directory, and reaches
  the same desired state. The harness kills the controller after leased,
  prepared, effect-gap, applied, observed, and completed transitions.
- Limit: this is Linux debug-test evidence. The failpoints are rejected in
  release builds. Graceful managed-child shutdown, Windows/macOS behavior,
  packaging, authorized mutations, and per-instance backup/restore remain open.
- Next gate: graceful shutdown and cross-platform process qualification.

## 2026-08-23 — F3 graceful shutdown and native process qualification

- Commits: `530a556`, `4a0b5b4`, `d0a1f11`, `ea4e6af`, `9da9dc2`,
  `acf161e`, `3f2da6d`, and `f9e5a89`.
- Capability: added a persisted bounded request/completion-file shutdown
  policy, graceful RRD drain, ownership reauthentication before forced kill,
  per-instance process logs, startup stability detection, portable token and
  storage publication, and the minimal Windows platform environment required
  for loopback networking.
- Verification:
  `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test -p rrd-server --test local_estate_driver --locked -- --nocapture`
  passed locally; `cargo clippy -p rrd-estate -p rrd-server --all-targets --locked -- -D warnings`
  passed locally. GitHub Actions run
  [`32666043965`](https://github.com/EonsofStupid/vyrm/actions/runs/32666043965)
  passed persistent authority, RRD binary contracts, real child/controller
  recovery, and strict clippy on Ubuntu, Windows, and macOS.
- Evidence: a normal stop requires RRD's durable completion marker after Axum
  drain; a deliberately unwatched request forces the bounded fallback without
  fabricating completion; effect-gap recovery retains process/data identity;
  forged PID identity remains denied. Preserved stderr identified and closed a
  Windows Winsock startup failure caused by clearing `SystemRoot`.
- Limit: packaging, operator-authorized mutation, per-instance backup/restore,
  and bounded process-log rotation/retention remain open. The aggregate
  workspace job in the cited run failed the separate OpenRaft
  `real_consensus_replicates_canonical_runtime_truth_to_every_voter` test, so
  the run is evidence for this native process matrix, not a repository-wide
  green claim.
- Next gate: make the deployment catalogue/install surface operable without
  hand-authored executable paths, then expose explicitly authorized estate
  mutations before backup/restore jobs.

## 2026-08-23 — correction: atomic runtime-plan format lowering

- Corrects: the repository-wide red limit recorded in “F3 graceful shutdown
  and native process qualification” above.
- Commit: `130b8c5` (`fix(cluster): lower atomic runtime plans to storage format`).
- Capability: an external coordinator can now combine a prepared native
  runtime transaction with its own metadata in one VyrmKV batch only after the
  plan lowers canonical staged keys to the target database's authenticated
  application format. The raw staged-parts method is no longer public.
- Verification:
  `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test -p vyrm-store -p vyrm-cluster --all-features --locked`
  passed; strict all-target/all-feature clippy passed. GitHub Actions run
  [`32667681611`](https://github.com/EonsofStupid/vyrm/actions/runs/32667681611)
  is green across the full workspace verification job and the Ubuntu, Windows,
  and macOS estate-process matrix.
- Evidence: the four-node real OpenRaft test commits canonical runtime truth,
  waits for every voter, snapshots, purges the source log, hydrates a new
  learner, shuts down, and reopens every node through `NativeEngine`. All four
  retain runtime cursor `1` and the exact commit outcome. Before the correction,
  Raft metadata survived but runtime keys were written in staged tag form under
  a legacy-format manifest, so reopen correctly exposed cursor `0`.
- Limit: this closes a format-composition defect; it does not add a distributed
  production deployment, operator authorization, or an F3 backup job.
- Next gate: operable deployment packaging, followed by explicitly authorized
  estate mutations and per-instance backup/restore.

## 2026-08-23 — F3 trusted deployment-catalogue generator

- Commits: `98ba3af` (`feat(rrd): generate trusted deployment catalogues`) and
  `e8979ff` (`ci(rrd): qualify catalogue generation natively`).
- Capability: added a validated constructor that canonicalizes and hashes a
  deployment executable and a local `rrd-deployment-catalog` binary that emits
  the complete typed RRD launch and shutdown policy from its installed sibling.
  The output is owner-private, synced, create-new, and never overwrites an
  existing catalogue.
- Verification: all `rrd-estate` and `rrd-server` tests passed locally; strict
  all-target clippy passed. GitHub Actions run
  [`32668383982`](https://github.com/EonsofStupid/vyrm/actions/runs/32668383982)
  passed the real generator, authority, server process, child/controller crash,
  and strict-clippy matrix on Ubuntu, Windows, and macOS.
- Evidence: the black-box generator test uses the actual built sibling
  `rrd-server`, reloads and validates its emitted catalogue, checks executable,
  version, typed instance argument and bounded shutdown policy, then proves a
  second invocation is denied without corrupting the first file.
- Limit: this is catalogue packaging, not an MSI/pkg/deb, service manager,
  auto-updater, signing authority, or remote operator mutation surface.
- Next gate: define and implement a local operator authorization boundary for
  estate creation and desired-state mutation before exposing either remotely.

## 2026-08-23 — F3 local operator-authorized estate mutations

- Commits: `0f2f306`, `bccc9ac`, `86247cc`, `1915f82`, `b3d4efb`,
  `4d7c253`, `c8bd372`, and `07a8fcd`.
- Capability: added a strict local operator policy bound to exact identity,
  32-byte key digest, validity window, estate and create/set-desired permission;
  added the `rrd-estate-admin` process and frozen `EstateMutationResult` output.
- Verification: all `rrd-contract`, `rrd-estate`, and `rrd-server` tests passed
  locally with strict all-target clippy. GitHub Actions run
  [`32671973317`](https://github.com/EonsofStupid/vyrm/actions/runs/32671973317)
  passed the full workspace and the real process/admin matrix on Ubuntu,
  Windows, and macOS. Earlier red runs exposed and drove fixes for a
  Windows-only test warning, transient executable inspection, hard-link path
  identity, PID reuse between child exit and `/proc` discovery, asynchronous
  redirected-log delivery, and retryable driver deferral in the crash matrix.
- Evidence: unauthorized estate work is rejected before database creation;
  exact create and desired-state retries report durable replay; native reopen
  retains revision and instance state; the hash-chained journal contains only
  `estate.create` and `estate.desired.set` under the policy-bound operator.
- Limit: policy/key provisioning, Windows ACL-owner inspection, credential
  rotation/revocation, remote authentication, RBAC/ABAC, and remote mutation
  remain open. Create replay has an explicit 65,536-entry alpha scan bound.
- Next gate: implement per-instance backup/restore jobs under the same local
  operator boundary, including durable job state and replay-safe recovery.

## 2026-08-23 — F3 durable per-instance backup execution

- Parent contract: `b71a612` (`feat(rrd): persist quiesced backup jobs`).
- Capability: added fenced lease, prepared, completed, and failed transitions
  for the strict backup-job resource; added a one-boundary reconciler and a
  local effect driver fixed to
  `<state-root>/instances/<instance>/rrd-data` and
  `<state-root>/backups/<instance>`. The driver denies retained process records
  before catalogue creation and authenticates the F1 catalogue before and
  after each content-addressed logical backup.
- Verification: all `rrd-estate` tests pass with strict all-target/all-feature
  clippy. Tests cover durable-effect/lost-ack recovery across native reopen,
  lease takeover with stale-worker fencing, one-entry catalogue convergence on
  exact replay, and process-record denial before filesystem mutation.
- Evidence boundary: a prepared job survives controller loss, then replay
  returns the identical backup identity without a second catalogue revision.
  Lease takeover preserves prepared evidence while advancing the fencing
  epoch.
- Limit: this checkpoint does not yet expose the reconciler as an operator
  process, authorize backup scheduling, restore an absent instance, or provide
  retention pruning. Cross-platform process-level qualification is still open.
- Next gate: ship the one-step local backup controller and authorized schedule
  command, then run the real-process crash/reopen matrix.

## 2026-08-23 — F3 authorized backup operation and process recovery

- Commit: `732587a` (`feat(rrd): operate authorized estate backups`).
- Capability: added the explicit `schedule_backup` local policy grant and
  `rrd-estate-admin schedule-backup`; accepted work returns the strict
  `EstateBackupMutationResult`. Added the one-step `rrd-backup-controller`,
  which carries no operator credential and advances only the durable backup job
  selected by the estate authority.
- Verification:
  `cargo test -p rrd-estate -p rrd-server --all-features --locked` passed,
  including every RRD socket, session, process, admin, estate and backup test;
  strict all-target/all-feature clippy passed for both crates; `git diff
  --check` passed.
- Evidence: the black-box process test kills the backup controller after its
  lease, after prepare, and after the authenticated archive/catalogue effect but
  before the completion receipt. Native reopen retains the prepared job; replay
  records one succeeded result while catalogue revision and entry count remain
  exactly one. The admin black-box test separately proves authorization,
  schedule replay, native reopen, and the exact operator journal actor.
- CI: the native estate-process matrix now includes the backup-controller
  binary and its real-process crash test. The pushed run is pending at this
  journal boundary; no cross-platform claim is made until it is green.
- Limit: backup scheduling and creation are now operable, but restore to an
  absent instance root, restore-job recovery, retention/pruning, remote
  administration, and service/installer packaging remain open.
- Next gate: freeze the restore-to-absent-instance contract and implement its
  verified, replay-safe state machine before retention work.

## 2026-08-23 — execution-order correction: full foundation first

- Decision: stop deepening one F3 subsystem while later product layers remain
  absent. Establish an honest breadth-first walking skeleton through F3–F9,
  preserving explicit alpha limits, before exhaustive hardening, optimization,
  or competitive benchmark claims.
- Required order: close local restore/retention/lifecycle operability; add the
  F4 security/audit skeleton; generate all F5 SDK skeletons from one contract;
  establish F6 multi-model/query/realtime; establish F7 vector/AI/TurboQuant
  paths; establish F8 distributed/Kubernetes/hybrid deployment; then make F9
  Connectome the authoritative operations and diagnostics client.
- Non-deferrable checks: each breadth slice must still prevent corruption,
  destructive overwrite, unauthenticated mutation, and false capability
  claims. These are implementation gates, not post-foundation optimization.
- Deferred depth: exhaustive platform/fault matrices, fine-grained policy,
  performance tuning, and SurrealDB/Qdrant/Fjall comparisons follow only after
  every foundation layer has a real executable path.
- Trigger: operator review correctly identified that the prior sequence was
  over-investing in F3 depth relative to the stated full-product objective.

## 2026-08-23 — RRD exact query service backbone

- Commit: `2e3d558` (`feat(rrd): expose exact query service`).
- Capability: added the transport-neutral, strictly bounded `ExecuteQuery`
  contract and the authenticated `POST /v1/query` service path. It uses the
  real VyrmQL parser and VyrmMX catalogue, binder, planner, and executor rather
  than a parallel HTTP query implementation.
- Evidence: the public response retains typed values, canonical query, read
  manifest, cursor, schema revision, planner candidates and exactness/order/
  authorization contract, stamp validation, scanned changes, returned rows,
  output bytes, and truncation. A real-socket test proves unauthenticated and
  wrong-instance-scope denial and returns an exact persisted record.
- Verification:
  `cargo test -p rrd-contract -p rrd-server --all-features --locked`, strict
  all-target/all-feature clippy for both crates, and `git diff --check` passed.
- Limit: this is the read-only service skeleton. It does not yet provide
  mutating VyrmQL, typed public multi-model transactions, live subscriptions,
  remote F4 identity, SDKs, or durable query spans; request-level JSON tracing
  is currently ephemeral.
- Next gate: extend the same persistent RRD transaction boundary with typed
  schema, record, relation, event, vector, time-series, geo, and object
  mutations before adding another isolated subsystem.

## 2026-08-23 — RRD atomic public multi-model transactions

- Commit: `4c3956b` (`feat(rrd): expose atomic multi-model transactions`).
- Capability: extended the transport-neutral transaction vocabulary with
  schema registries, records/documents, graph relations, events,
  dense/sparse/multi-dense vectors and embedding provenance, time-series
  samples, WGS84 geo values, and pre-staged immutable object references. The
  `data` transaction scope lowers them explicitly into the existing
  authoritative runtime instead of exposing private Rust representations.
- Atomicity: claims and every typed model share one `RuntimeCommit`, exact
  global-cursor compare-and-swap, hash chain, audit envelope, projection
  outbox, and content identity. The persistent RRD prepared intent freezes the
  runtime time and commit digest; restart recovery resolves that digest from
  the runtime commit catalogue before attempting another write.
- Evidence: one real HTTP socket transaction commits all nine mutation
  families as eleven changes, advances one claim sequence and one eleven-entry
  runtime interval, restarts the server, returns the same commit SHA-256 as an
  idempotent replay, and retains exactly eleven scoped changes with no
  duplicate claim.
- Verification:
  `cargo test -p rrd-contract -p rrd-server --all-features --locked`, strict
  all-target/all-feature clippy for both crates, and `git diff --check` passed.
- Limit: prospective data preview currently validates the public mutations but
  is not yet a complete read-your-writes graph projection. Mutating VyrmQL,
  object upload/staging, live subscriptions, model-specific administration,
  data-scope process-kill qualification, F4 identity, and SDKs remain open.
- Next gate: expose retained runtime changefeeds/live subscriptions and the
  existing vector/search execution path through this same authenticated public
  service backbone.

## 2026-08-23 — RRD canonical vector-search service

- Commit: `f393037` (`feat(rrd): expose exact vector search`).
- Capability: added the typed, authenticated `POST /v1/vector/search`
  contract for bounded dense, sparse, and multi-dense/MaxSim queries using
  cosine, dot, Euclidean, or Manhattan scoring.
- Evidence: the service captures an authoritative runtime read stamp, denies
  incomplete scans rather than returning partial truth, builds the canonical
  candidate set, and runs the existing Vyrm vector planner and exact oracle.
  Responses carry manifest/cursor, scan count, plan digest, selected access
  path, exactness, typed vector/subject references, source cursor, and score.
- Verification: the multi-model real-socket fixture proves unauthenticated
  denial and exact cosine retrieval of the vector committed in the preceding
  public transaction. All `rrd-contract` and `rrd-server` tests and strict
  all-target/all-feature clippy passed; `git diff --check` passed.
- Limit: the public path is canonical exact search only. Filter algebra,
  embedding model binding, persisted exact/HNSW/TurboQuant artifact serving,
  recommendation/discovery algebra, and GPU selection remain open and are not
  advertised as available.
- Next gate: expose retained, cursor-addressed runtime changefeed replay so
  Connectome and SDKs can observe and reconstruct the same committed activity.

## 2026-08-24 — RRD retained typed changefeed replay

- Commit: `e4532c5` (`feat(rrd): expose retained changefeed replay`).
- Capability: added the authenticated, page-bounded
  `POST /v1/changes/read` contract over the authoritative runtime log. Clients
  supply an exact global cursor and resume only from the returned
  `through_cursor`, preventing sparse scoped feeds from stalling on unrelated
  activity.
- Fidelity: every entry preserves cursor, commit SHA-256 and ordinal, scope,
  runtime time, actor, prior/current change SHA-256, complete claim provenance,
  or the corresponding typed public schema/record/relation/event/vector/
  series/geo/object mutation. The page also carries authenticated-read method,
  change-read count, and proof-node count.
- Evidence: the real HTTP fixture proves unauthenticated denial, pages one
  eleven-change transaction as `3 + 8`, verifies digest-chain continuity at
  the page boundary, restarts the server, and resumes from cursor ten to return
  only cursor eleven. No secondary event store or projection is involved.
- Verification: all `rrd-contract` and `rrd-server` tests passed, including the
  new strict request contract and real-socket replay matrix; strict all-target/
  all-feature clippy and `git diff --check` passed.
- Limit: this is retained pull replay. Push/long-poll delivery, subscription
  leases, backpressure, heartbeats, and disconnect/reconnect qualification
  remain open before claiming real-time live queries.
- Next gate: implement bounded authenticated live-follow over the same cursor
  contract, retaining pull replay as the reconnect source of truth.

## 2026-08-24 — RRD bounded changefeed follow

- Commit: `6795151` (`feat(rrd): add bounded changefeed follow`).
- Capability: added authenticated `POST /v1/changes/follow`, a maximum
  five-second long-poll over the exact retained replay coordinate. It returns
  either the first typed page after the cursor or an explicit timeout carrying
  the newest observed `through_cursor`; reconnect remains stateless and uses
  the retained feed.
- Bounds: page and wait limits are frozen in `rrd-contract`; a declared request
  deadline must cover the requested wait. The implementation polls in the
  existing blocking worker pool in 25 ms bounded intervals and never holds an
  engine transaction or snapshot across the wait.
- Evidence: one real HTTP connection waits after cursor two while another
  commits an event at cursor three. The waiter wakes with exactly that typed
  event. A second follow after cursor three waits 50 ms and returns an explicit
  empty timeout at the same cursor.
- Verification: all `rrd-contract` and `rrd-server` tests passed, including 14
  real-socket cases; strict all-target/all-feature clippy and `git diff
  --check` passed.
- Limit: this is bounded long-poll, not SSE/WebSocket push. Durable subscription
  ownership, heartbeats, disconnect cancellation, fan-out backpressure, and
  server-side live-query maintenance remain open and are not advertised.
- Next gate: expose backup/restore and diagnostics through the canonical RRD
  service, then begin F4 identity and policy rather than deepening transport
  streaming first.

## 2026-08-24 — RRD managed logical backup and restore

- Commit: `42456c0` (`feat(rrd): operate managed backup restore`).
- Capability: added authenticated `POST /v1/backups`,
  `POST /v1/backups/list`, and `POST /v1/restores` over the existing logical
  archive and authenticated catalogue. Public requests are path-closed; the
  server derives per-instance backup and restore roots and restores only into
  a generated absent root.
- Recovery: backup and restore operations durably bind idempotency key,
  operation digest, request identity, and prepared/completed state. Backup
  archive labels are operation-qualified so a completed filesystem effect can
  be rediscovered after a lost acknowledgement. An existing restore target is
  accepted only after reopening it and matching archive claim/runtime
  watermarks.
- Evidence: the real-socket fixture denies unauthenticated creation, proves
  idempotency collision denial, creates and verifies the catalogue, restores
  and reopens a new root, checks its runtime cursor, restarts the server, and
  replays both operations without duplicating either effect.
- Verification: all `rrd-contract` and `rrd-server` tests passed, including 15
  real-socket cases and the existing estate process-kill matrices; strict
  all-target/all-feature clippy and `git diff --check` passed.
- Limit: the archive declares object payloads referenced-only and application
  completeness false. Retention/RPO policy, active-root deployment switch,
  and an exact process-kill injection between this service's filesystem effect
  and completed control record remain open.
- Sequencing correction: establish a runnable breadth baseline across F4-F9
  and Automaton/LFG integration before returning to performance optimization.
  The next product slice is F4 identity, deny-by-default policy, secrets/TLS,
  and comprehensive public-operation audit.

## 2026-08-24 — persistent F4 security authority core

- Commit: `3bc71f9` (`feat(security): establish persistent policy authority`).
- Ownership: introduced `rrd-security` as the RRD identity, authorization, and
  audit owner instead of placing policy inside VyrmKV, query execution, RRO,
  or Connectome.
- Capability: persistent bounded user/service/node principals carry only
  credential SHA-256 verifiers, validity/disable state, and exact closed-action
  resource-prefix grants. Missing policy, unknown identity, bad credential,
  expired/disabled identity, wrong action, and wrong resource deny by default.
- Audit: typed records retain identity, action, resource, request/operation
  coordinates, decision, status, and request/response digests while excluding
  bodies, credentials, tokens, and arbitrary headers. Immutable audit IDs deny
  rebinding and replay through the authenticated control journal.
- Evidence: native reopen tests prove exact allow, every principal denial above,
  wrong-instance denial after restart, audit idempotency/collision, journal
  verification, and absence of the raw credential from serialized evidence.
- Verification: package tests, strict all-target/all-feature clippy, and `git
  diff --check` passed.
- Limit: this is the persistent authority core, not completed F4. The next
  slice binds session creation and every current HTTP route to it and records
  allowed/denied/failed outcomes. TLS/mTLS, secret providers, row/field policy,
  rate limits, and provisioning remain open; remote bind stays denied.

## 2026-08-24 — principal-bound HTTP action enforcement

- Commit: `fb01db6` (`feat(rrd): enforce principal action policy`).
- Session boundary: an instance with initialized security state requires
  `X-RRD-Principal` and `Authorization: ApiKey …` for session creation. The
  authenticated principal is durably bound to the session and included in
  idempotency collision checks; API-key material is not stored.
- Authorization: every currently authenticated RRD route maps to one closed
  `rrd-security::Action`. Each request authenticates the bearer lease and then
  re-evaluates current principal validity and exact resource-prefix grants, so
  a disabled identity or removed grant affects an existing session.
- Compatibility boundary: instances without initialized authority remain in
  explicitly advertised loopback development mode. Security capability state
  is visible in negotiation. Non-loopback bind remains denied in every mode.
- Evidence: a real-socket differential denies missing and wrong API keys,
  allows the granted exact VyrmQL query, denies an ungranted backup with 403,
  and verifies the runtime cursor did not change.
- Verification: all `rrd-security` and `rrd-server` tests passed, including 16
  real-socket cases and existing process-kill matrices; strict all-target/all-
  feature clippy and `git diff --check` passed.
- Limit: endpoint audit completion is the next F4 slice. Provisioning, TLS/
  mTLS, secret providers, row/field policy, rate limits, and remote exposure
  remain open.

## 2026-08-24 — secured HTTP authorization and outcome audit

- Commit: `e7541d9` (`feat(rrd): audit secured request outcomes`).
- Public contract: froze closed security actions, authorization/completion
  phases, allow/deny/fail decisions, bounded `ReadAudit`, and redacted public
  audit snapshots. The resume coordinate advances through unrelated global
  control-journal history rather than stalling at the last matching record.
- Pre-effect evidence: after policy allows a routed operation, RRD durably
  appends an `authorized/allowed` reservation before execution. Denied requests
  receive a terminal completion only; accepted work receives a final allowed
  or failed completion after the response is known. Missing completion remains
  observable after a process/audit-writer gap.
- Coverage: secured session creation and every authenticated handler now emit
  redacted records. Public health/capability inspection, unknown routes,
  malformed envelopes, missing bearer headers, authorization denials, success,
  and execution failure are covered at the routed boundary.
- Read path: `POST /v1/audit/read` is itself protected by the exact `audit_read`
  grant and returns request/response digests rather than bodies, credentials,
  tokens, or arbitrary headers.
- Evidence: the real socket test observes public inspection, unknown route,
  missing/wrong API keys, allowed query, ungranted backup, missing bearer, and
  failed query. It reads thirteen authorization/completion records spanning all
  decision classes, reopens the journal, and proves API-key material and the
  authorization scheme were not persisted.
- Verification: all `rrd-contract`, `rrd-security`, and `rrd-server` tests
  passed; strict all-target/all-feature clippy and `git diff --check` passed.
- Limit: oversized-body and handler-join failures, retention/rotation, external
  archival, and atomic application-mutation/audit-completion publication remain
  open. F4 also still requires provisioning, TLS/mTLS, secret providers, row/
  field policy, and rate limits.

## 2026-08-24 — machine-readable F5 endpoint catalogue

- Commit: `d9cd948` (`feat(contract): publish endpoint catalogue`).
- Canonical source: `rrd-contract::EndpointCatalogue` freezes all 20 current
  public operations with canonical identity, HTTP method/path template,
  authentication mode, mutation/idempotency classification, closed security
  action, and public request/response type names.
- Validation: operations and method/path pairs are unique, sorted, bounded,
  ASCII and versioned; mutating GET routes and private Rust type paths fail
  closed.
- Runtime: `GET /v1/schema/endpoints` serves the exact catalogue and capability
  negotiation advertises its endpoint count. A real-socket test decodes the
  version and all 20 entries.
- Verification: `rrd-contract` and `rrd-server` tests plus strict all-target/
  all-feature clippy and `git diff --check` passed.
- Limit: the catalogue is the route-generation source, not a complete OpenAPI/
  JSON Schema bundle or a supported client. Next is the Rust network client and
  shared black-box fixture, followed by TypeScript, Python, Go, Java, and .NET
  packages generated from the same operation/type vocabulary.

## 2026-08-24 — supported asynchronous Rust RRD client

- Commit: `0e6f850` (`feat(sdk): add async Rust RRD client`).
- Boundary: introduced `rrd-client` as the first supported SDK consumer of the
  public `rrd-contract`; its release graph does not depend on storage, query,
  server, estate, or security implementation crates.
- Surface: typed async methods cover capability/catalogue negotiation, session
  lifecycle, transactions, VyrmQL, vector search, changefeed, managed backup/
  restore, estate projection, and protected audit.
- Safety: the client rejects remote cleartext, validates request envelopes and
  response identity, caps response accumulation at four MiB, applies per-attempt
  and absolute deadlines, maps typed API errors, and bounds transport retries
  to reads or mutations carrying contract-enforced idempotency keys.
- Evidence: a real secured server behind a TCP fault proxy drops the first
  connection and proves negotiation recovery, wrong-key classification,
  session creation, exact query, expired-deadline denial, transaction preview/
  abort, retained changefeed, protected audit, and non-loopback rejection.
- Verification: `rrd-contract`, `rrd-client`, `rrd-security`, and `rrd-server`
  tests passed, including the 16-case HTTP process suite and process-kill
  matrices; strict all-target/all-feature Clippy and `git diff --check` passed.
- Limit: F5 remains open. Shared generated schemas/conformance and TypeScript,
  Python, Go, Java, and .NET clients are next; TLS/distributed qualification and
  released-version compatibility remain later gates.

## 2026-08-24 — authoritative OpenAPI and JSON Schema projection

- Commit: `6a3727e` (`feat(contract): serve deterministic OpenAPI schemas`).
- Single authority: every public RRD wire type now derives JSON Schema, while
  `openapi_document()` combines those exact schemas with the sorted endpoint
  catalogue. No route or payload is independently described by hand.
- Runtime/export: `GET /v1/schema/openapi` serves the OpenAPI 3.1 document from
  a real RRD process, `rrd-contract-export` emits the identical pretty JSON for
  package generation, and the Rust client negotiates and validates it.
- Drift gate: the canonical document SHA-256 is frozen; all 21 catalogue
  operations must have request/response schemas and intentional wire drift
  requires explicit review.
- Evidence: contract tests traverse every catalogue operation and assert its
  method, operation ID, request body where applicable, and response schema. The
  real-socket server and fault-proxied Rust-client tests fetch the document.
- Verification: contract/client/server tests and strict all-target/all-feature
  Clippy passed; `git diff --check` passed.
- Limit: the document is generator input, not completion of F5. The next slice
  creates TypeScript/ArkType/Biome, Python, Go, Java, and .NET packages plus the
  shared language-neutral black-box fixture.

## 2026-08-24 — generated TypeScript RRD client foundation

- Commit: `d82ee13` (`feat(sdk): add generated TypeScript RRD client`).
- Contract correction: SDK generation exposed schema-local recursive `$defs`
  that ordinary OpenAPI tooling could not resolve. `QueryValue` is now one
  canonical OpenAPI component, all references resolve, and the frozen document
  digest covers the corrected generator-compatible shape.
- Generated surface: OpenAPI TypeScript emits exact `paths`/`operations` for
  all 21 routes; the generator separately derives the closed runtime endpoint
  map from operation, method, path, auth, and mutation metadata. Drift checking
  regenerates both from `rrd-contract` without a shell.
- Runtime: `RrdClient.call()` is strongly keyed by generated operation ID and
  payload/result types. It enforces loopback cleartext, canonical identities,
  resource shape, idempotent mutations, bounded streamed responses, deadlines,
  cancellation, safe retries, API-key/session headers, response identity, and
  typed API errors without including credentials in errors.
- Validation/tooling: ArkType rejects malformed outer response envelopes and
  Biome 2 is the only formatter/linter. pnpm locks exact tool versions and
  explicitly permits only the reviewed `esbuild` dependency build.
- Evidence: generation drift, Biome, strict TypeScript, and three Node tests
  pass, covering transport retry, session/query envelope and auth construction,
  remote-cleartext/deadline denial, and typed permission errors. Rust contract,
  real-client, and full real-server suites plus strict Clippy also pass.
- Repository correction: commit `a28574b` removed accidentally staged
  `node_modules` from the Git index and added a nested dependency ignore rule;
  only source, generated types, configuration, and the lockfile remain tracked.
- Limit: per-payload ArkType generation, browser/released-package matrices, and
  shared real-server conformance remain open. Python, Go, Java, and .NET are the
  next breadth slices.

## 2026-08-24 — generated Python RRD client foundation

- Commit: `00dd754` (`feat(sdk): add generated Python RRD client`).
- Generated surface: a shell-free generator projects the closed operation ID,
  HTTP method/path, authentication, and mutation map for all 21 routes from the
  authoritative OpenAPI document. Checked-in output has an explicit drift gate.
- Runtime: the synchronous HTTPX client provides generic coverage for every
  operation plus capability, catalogue, OpenAPI, and session helpers. It
  enforces loopback-only cleartext, canonical identities, resource paths,
  mutation idempotency, bounded streamed responses, absolute/per-attempt
  deadlines, response correlation, safe retry, API-key/session headers, and
  typed API errors.
- Validation/tooling: Pydantic rejects malformed outer response envelopes;
  exact dependency and tool versions are locked with uv. Ruff is the sole
  formatter/linter and strict mypy gates the public source.
- Evidence: generation drift, Ruff, strict mypy, four pytest transport tests,
  and wheel/source-distribution builds pass. Tests cover retry negotiation,
  session/query auth and envelope construction, remote-cleartext/deadline
  denial, typed permission errors, and complete catalogue generation.
- Limit: this is a synchronous generic-payload walking skeleton. Async support,
  generated per-payload models, shared real-server/version conformance, and
  package publication remain open. Go, Java, and .NET are the next breadth
  slices.

## 2026-08-24 — generated Go RRD client foundation

- Commit: `a1450ae` (`feat(sdk): add generated Go RRD client`).
- Generated surface: a shell-free Go generator projects a closed operation
  constant set plus method/path/authentication/mutation metadata for all 21
  routes from authoritative OpenAPI. Go's formatter canonicalizes checked-in
  output and the generator supplies an exact drift gate.
- Runtime: the standard-library client covers every operation plus capability,
  catalogue, OpenAPI, and session helpers. It accepts caller cancellation,
  combines absolute and per-attempt deadlines, disables redirects, restricts
  cleartext to credential-free loopback, validates identities/resources,
  requires mutation idempotency, bounds response bodies, correlates response
  identity, applies API-key/session auth, and retries only safe calls after
  transport failure.
- Validation: strict JSON decoding rejects unknown envelope fields and invalid
  success/error discrimination before returning a generic object payload.
- Evidence: generation drift, `gofmt`, `go vet`, behavioral tests, and the race
  detector pass. Tests cover retry negotiation, capability identity, session/
  query auth and envelopes, resource identity, remote-cleartext/deadline
  denial, typed permission errors, and all 21 generated operations.
- Limit: generated per-payload types, shared real-server/version conformance,
  examples/reference generation, and module publication remain open. Java and
  .NET are the next breadth slices.

## 2026-08-24 — generated Java RRD client foundation

- Commit: `9204459` (`feat(sdk): add generated Java RRD client`).
- Generated surface: a shell-free generator emits a closed Java enum carrying
  method/path/authentication/mutation metadata for all 21 OpenAPI operations,
  with exact checked-in drift detection.
- Runtime: the Java 21 client provides generic operation coverage plus
  capability, catalogue, OpenAPI, and session helpers. It disables redirects,
  rejects non-loopback cleartext without arbitrary DNS resolution, validates
  identities/resources, requires mutation idempotency, combines absolute and
  per-attempt deadlines, bounds response bodies, correlates response identity,
  authenticates API-key/session calls, and retries only safe I/O failures.
- Validation/tooling: Jackson 3.2 parses JSON but explicit closed-field checks
  reject malformed envelope/outcome/error shapes. Maven pins its plugins and
  compiles Java 21 with all lint warnings treated as errors.
- Evidence: the generator drift gate, packaged JAR, and three JUnit 6
  real-loopback tests pass. Tests cover dropped-connection retry, capability
  identity, session/query auth and envelopes, resource identity, remote-
  cleartext/deadline denial, typed permission errors, and all 21 operations.
- Limit: async/caller cancellation, generated per-payload models, dependency
  verification, shared real-server/version conformance, and Maven Central
  publication remain open. .NET is the final F5 breadth slice.

## 2026-08-24 — generated .NET RRD client foundation

- Commit: `e99786a` (`feat(sdk): add generated .NET RRD client`).
- Generated surface: a shell-free generator emits a closed enum and endpoint
  switch carrying method/path/authentication/mutation metadata for all 21
  OpenAPI operations, with exact checked-in drift detection.
- Runtime: the asynchronous .NET 10 client uses only `HttpClient` and
  `System.Text.Json` at runtime. It covers every operation plus capability,
  catalogue, OpenAPI, and session helpers; accepts cancellation; combines
  absolute/per-attempt deadlines; rejects redirects and non-loopback cleartext;
  validates identities/resources; requires mutation idempotency; bounds streamed
  bodies; correlates response identity; applies API-key/session auth; and retries
  only safe transport failures.
- Validation/tooling: closed-field parsing rejects malformed envelope/outcome/
  error shapes. Nullable analysis and all warnings fail the build; package
  dependency graphs are locked.
- Evidence: generator drift, locked restore, `dotnet format`, Release build,
  three explicitly enumerated xUnit v3 transport/auth/error tests, and NuGet
  packing pass with no package warning.
- Limit: all six intended F5 language clients now have executable walking
  skeletons. Generated payload completeness, shared real-server/released-version
  conformance, examples/reference output, and publication remain open. The next
  breadth slice moves to F6 multi-model/query/index/realtime foundations.

## 2026-08-24 — VyrmQL series and geospatial read foundation

- Commit: `9ca9d1f` (`feat(query): expose series and geo in VyrmQL`).
- Corrected boundary: public transactions already atomically persist every
  current data family. F6 therefore extends missing read semantics instead of
  adding a parallel mutation path or wrapper.
- Grammar/planner: added `series:<kind>` and `geo:<kind>` sources beside record,
  relation, event, and claim. Both use explicit valid/known time, the captured
  schema/read stamp, a digest-bound exact plan, deterministic identity order,
  and the existing execution budgets.
- Execution: series rows expose sample/series identity, observation time, and
  typed scalar values; geo rows resolve the active version and expose subject,
  field, validity, geometry kind, and canonical decimal point/bounding-box
  coordinates. Custom properties remain visible through `PROJECT *`, while
  filtering/explicit projection is intentionally limited to frozen built-ins.
- Evidence: parser corpus/canonicalization, eight VyrmMX tests, three-engine
  series/geo differentials, pre-observation/pre-validity exclusion, the secured
  real-RRD atomic-data test, and strict Clippy pass.
- Limit: recursive traversal, general numeric/spatial operators, indexes and
  statistics, full text, mutating/multi-statement VyrmQL, streaming batches,
  and push live subscriptions remain open F6 breadth.
