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
