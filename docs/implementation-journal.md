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
