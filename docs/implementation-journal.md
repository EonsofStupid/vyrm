# RRFlow implementation journal

## 2026-08-26 — owner-controlled `0.1.0` release train

- Product decision: RRFlow remains at `0.1.0`. Internal protocol, contract,
  fixture, schema, and persisted-format `v1` identifiers are compatibility
  domains and are not product release numbers.
- Single source: root `VERSION` and `[workspace.package].version` carry the
  canonical release. All 22 Rust workspace crates now inherit rather than
  repeat it.
- Cross-ecosystem parity: TypeScript, Python, Java, and .NET client package
  manifests and the public implementation fixture now report exact `0.1.0`.
  Python's lockfile was reconciled without dependency changes.
- Enforcement: `scripts/check_version.py` validates SemVer, Rust inheritance,
  every packaged SDK, the Python lock, the public implementation fixture, and
  a future canonical `apps/connectome` mount. Linux CI runs the check before
  the workspace architecture and test gates.
- Approval boundary: `.github/CODEOWNERS` assigns `VERSION`, the guard, its CI
  step, and this policy to the repository owner. GitHub currently reports the
  default `main` branch as unprotected, so owner approval is requested but is
  not yet server-enforced. Branch protection must require Code Owner review to
  make the approval rule mandatory.
- Connectome truth: the canonical Connectome application is still not mounted
  in RRFlow as the planned `apps/connectome` submodule. Its separate dirty
  checkout has conflicting package versions and was deliberately not mutated.
- Verification: the version-policy check, locked Cargo metadata, Python lock
  check, 25 `rrd-contract` targets, and `git diff --check` pass locally.

## 2026-08-26 — enforced foundation work plan and first runtime gate

- Commit: `pending`. Evidence is local to the current dirty pre-release tree;
  no full-workspace or remote-platform qualification is claimed.
- Scope: added the checked-in `rrflow.workplan.toml` as one machine-readable
  dependency graph with 13 gates and 65 stable items covering cohesive RRD,
  persistence/recovery, multi-model query, vector/inference, service/security,
  generated surfaces, provider-neutral runtime enforcement, estate/cluster,
  Connectome, persistent end-to-end tests, competitor evidence, and firm-alpha
  release. `docs/full-stack-gap-ledger.md` now projects those gates and states
  that RRD—not Markdown—owns completion.
- Public contract: `rrd-contract` now owns strict V1 work-plan definitions,
  status snapshots, and sealed hash-chained event envelopes. Validation rejects
  unknown fields, unsupported versions, duplicate IDs, unsorted dependencies,
  missing dependencies, cycles, empty acceptance evidence, invalid digests,
  and tampered events.
- RRD authority: `rrd-engine::runtime::workplan` persists the exact plan and
  digest through the existing control-state CAS and authenticated journal. It
  enforces dependency-ready single-item activation, an attunement/tree-bound
  recorded plan, one exact outstanding tool authorization, exact post-tool
  consumption, all-passing verification evidence, and verified-only
  completion. An unreviewed source-plan change is denied rather than silently
  treated as a new revision. Close/reopen reconstructs the same aggregate and
  consumed authorization state.
- Lifecycle integration: preflight installs and renders a checked-in plan.
  User-prompt hooks inject its current status. Pre-tool hooks deny a project
  mutation when the plan has no active item, no recorded plan, stale tree or
  attunement evidence, or another outstanding authorization. Post-tool hooks
  close the exact authorization and persist an observation. Generated Claude
  wiring now observes Edit, Write, NotebookEdit, and Bash completion rather
  than Bash alone.
- Operator path: `rrflow work-plan sync|status|activate|record|verify` exposes
  the first trusted control flow. `record` reads the reviewed plan file and
  exact verification argv arrays while deriving source-tree and attunement
  digests itself. `verify` executes stored argv without a shell, with a
  15-minute time limit and 16-MiB combined output limit; binds exit status and
  output digests to the unchanged tree, repository revision (or explicit tree
  identity for a non-Git project), and platform; then requests the engine's
  verified transition. A model cannot provide a boolean success flag.
- Setup correction: the RRFlow CLI default database path is now the same
  canonical `.rrflow/rrd` used by instance binding, RRD server startup, and
  generated hook wiring. The previous `.rrflow/dev/rrd` default could split
  one project across two authorities.
- Executable evidence: the real compiled CLI test runs session preflight,
  sync, activate, record, pre-tool, post-tool, bounded verification, process
  reopen, and status; it observes `[x]` only after verification. A separate
  hook test proves an unscoped mutation is denied and a bound mutation is
  allowed. Persistent engine tests prove dependency denial, stale-evidence
  denial, one-shot authorization, failed-verification denial, unreviewed-plan
  denial, verification, and exact reopen replay.
- Verification: `cargo fmt --all -- --check`; all targets for `rrd-contract`,
  `rrd-engine`, and `rrflow-cli` (139 tests, 139 passed, zero failed); strict
  all-target Clippy for the same packages with `-D warnings`; and
  `git diff --check` passed locally.
- Open G00 gates: the full provider-neutral session/turn/planning lifecycle is
  not yet represented by one canonical state machine; automatic work-plan
  generation for projects without a checked-in plan is absent; the exact-argv
  proxy does not yet govern every runner/tool path; MCP, SDK, and Connectome
  work-plan projections are not generated; provider conformance and the remote
  Linux/macOS ARM/Windows matrix are not run. Consequently G00 remains open
  and no full-foundation completion claim is made.
- Next dependency-critical item: G00-W02, complete the canonical lifecycle
  envelope and transition machine over the same RRD journal, then bind the
  existing work-plan events to session/turn/preflight/planning identities.

## 2026-08-25 — MCP foundation correction and reviewed expansion map

- State at the start of this correction: A1-A7 were implemented locally while
  A8-A17 lacked executable evidence. The later A8-A12 and A13-A17 journal
  entries supersede that point-in-time count.
- Finding: MCP discovery is generated correctly, but only from the 11-entry
  reasoning/runtime catalogue. The 28 public RRD endpoint operations are
  represented in the product capability catalogue with MCP disposition
  `planned`; they were never adapted to MCP. Therefore the current MCP is a
  partial runtime surface, not the full RRFlow foundation.
- Decision: do not expose raw session renewal or transaction lease plumbing as
  model tools. `docs/rrflow-mcp-foundation.md` freezes 17 task-level adapters
  over already executable RRD capabilities, producing 28 executable tools when
  complete. It also records 12 additional engine gaps that may not be
  advertised early.
- Security invariant: embedded and daemon modes are explicit and mutually
  exclusive for one database. Initialized security denies uncredentialed MCP;
  process credentials use operator-owned references/files and never
  model-authored arguments. Every mutation requires the ordinary RRD policy,
  idempotency, transaction, audit, and preflight path.
- Environment evidence: local Tailscale is online as
  `warden-devstation-01.tail7934c8.ts.net` and already serves HTTPS port 4387;
  Rust/Cargo 1.98.0, Git, Tailscale, and Cloudflare tooling are installed. No
  additional host installation is required for the private development route.
- A1 implementation: `rrflow_service_status` is now generated from a typed
  schema and executable through the engine-owned registry. It returns the real
  RRD readiness, initialized-security state, all 28 public endpoint
  descriptors, the executable MCP registry, and the shared product capability
  catalogue. This makes 12 executable MCP tools; it does not complete the
  A-series.
- Security correction: governed embedded runtime tools now fail
  unauthenticated when the persistent security authority is initialized.
  Public service status remains readable, matching RRD's public health and
  capability endpoints. Credentialed daemon mode remains required before
  governed MCP works against a secured instance.
- Verification: all three `rrflow-mcp` stdio tests passed, including a real
  A1 call reporting 28 endpoints and exact executable-tool count; the engine
  capability-catalogue tests passed; the dedicated initialized-security test
  proved governed-tool denial and public-status allowance; strict Clippy for
  `rrd-engine` and `rrflow-mcp` passed; `git diff --check` passed.
- A1 checkpoint gate: build A2 atomic multi-model commit on the shared
  generated-schema/authorization adapter contract. That gate is satisfied in
  the immediately following entry. No independent handwritten MCP list is
  permitted.

### A2 atomic multi-model MCP adapter

- Capability: `rrflow_data_commit` is the thirteenth executable MCP tool. Its
  generated input schema consumes the public ordered `TransactionMutation`
  vocabulary and maps to the existing `transaction-commit` product capability
  instead of creating a second MCP-specific feature row.
- Cohesion: the adapter owns only high-level lease plumbing and calls the
  ordinary `RrdEngine` session/begin/commit path. Schema, claim, document/
  record, relation/native-edge, event, vector, time-series, geo, and object-
  reference mutations share one read stamp and durable commit. No second
  database, transaction coordinator, catalogue, or token surface was added.
- Lifecycle: application data commits require a fresh project-attunement
  receipt bound to the exact tool name and arguments. RRD automatically closes
  the authorization and records the post-tool observation. Missing/mismatched
  authorization fails before execution; initialized security denies the
  uncredentialed adapter before lifecycle or data mutation.
- Recovery correction: exact transaction-begin replay is now resolved before
  session liveness is applied. A committed transaction can therefore be found
  and replayed after its session has been durably expired, without allowing a
  new transaction on an expired session.
- Evidence: `rrd-engine`'s 26 library tests passed; focused capability,
  security, and two A2 integration tests passed; the latter commits nine
  schema/document/edge/event/vector/series/geo mutations, queries four models,
  reopens the database, and proves an idempotent replay. All three real-process
  `rrflow-mcp` stdio tests passed with generated 13-tool discovery. All-target,
  all-feature `rrd-engine` tests, strict Clippy for engine/MCP, formatting, and
  `git diff --check` passed. Full workspace and remote CI are not yet recorded
  for this slice.
- Next gate: A3 query-index ensure through the same typed capability mapping,
  security, exact-attunement, idempotency, reopen, and stdio evidence contract.

### A3 governed query-index ensure adapter

- Capability: `rrflow_query_index_ensure` is the fourteenth generated MCP tool
  and binds to the existing `query-index-ensure` product capability. Its schema
  flattens the public `EnsureQueryIndex` contract beside the required adapter
  idempotency key and optional operation time.
- Internal boundary: high-level adapters now share one domain-separated
  `AdapterSession` implementation. It derives stable internal session/request/
  operation identities, keeps tokens out of schemas and results, and calls the
  ordinary RRD session authority. A2 was migrated onto the same helper before
  A3 was added.
- Recovery: query-index ensure now checks persistent authorization policy and
  an exact stored operation receipt before lease liveness. This permits only a
  completed exact replay after expiry; a new index create/rebuild still expires
  the session and fails closed.
- Evidence: the multi-model integration commits two documents, builds a ready
  title index with two artifact rows, reopens RRD, advances beyond the session's
  maximum absolute lease, and receives an idempotent replay. All-target,
  all-feature engine tests, all MCP stdio tests, capability conformance, strict
  engine/MCP Clippy and `git diff --check` passed locally. Workspace and remote
  CI remain unrecorded.
- Next gate: A4 authorized query-index catalogue list using the same generated
  capability mapping and shared adapter-session boundary.

### A4 governed query-index catalogue adapter

- Capability: `rrflow_query_index_list` is the fifteenth generated MCP tool and
  maps to `query-index-list`. It consumes `ListQueryIndexes` and invokes the
  existing engine catalogue read without duplicating catalogue state.
- Lease boundary: non-idempotent reads do not ask the model to manufacture an
  operation key. The adapter creates at most one internal read session per
  operation and maximum-lease window; tokens and leases remain internal.
- Evidence: the A2/A3 integration reopens RRD and lists the real ready index;
  initialized-security coverage proves the uncredentialed governed read is
  denied; capability parity, MCP stdio discovery, focused engine tests, strict
  Clippy, formatting, and `git diff --check` passed locally. Workspace and
  remote CI remain unrecorded.
- Next gate: A5 bounded live-query poll over the existing RRD engine operation.

### A5-A7 governed realtime adapters

- Capabilities: generated MCP discovery now has 18 executable tools. A5 maps
  live RRFlowQL delta polling to `query-live-poll`; A6/A7 map retained page read
  and bounded follow to `changefeed-read`/`changefeed-follow`.
- Real behavior: the integration reports both committed documents as live-query
  additions. Its retained changefeed identifies exactly nine mutations under
  the A2 commit digest while also retaining RRD reasoning/lifecycle/trace
  events. The page carries bounded hash-chain validation evidence, and follow
  from head returns an explicit bounded timeout. Existing real-server coverage
  proves a waiting follow wakes on a concurrent commit.
- Security and bounds: all three are governed reads; initialized security
  denies the uncredentialed adapter. Request schemas retain the public query,
  row, page, scan, output and maximum five-second wait constraints.
- Evidence: focused adapter, capability and security tests; all three MCP stdio
  tests; strict engine/MCP Clippy; formatting; and `git diff --check` pass
  locally. Full workspace and remote CI remain unrecorded.
- Next gate: A8 governed vector-collection ensure.

## 2026-08-25 — Executable development-topology gate

- State: implemented and locally verified; not committed or remotely
  qualified.
- Capability: added the repository-native `cargo rrflow-dev doctor --root .`
  command. It parses the real workspace/package manifests, checks canonical
  package identity and surface dependency boundaries, proves the RRD health and
  capability routes exist, verifies isolated developer state and required
  toolchains, reports optional remote-exposure tooling, emits JSON or readable
  output, and exits nonzero while any required topology invariant is blocked.
- Current evidence: the expanded report has 13 passing checks and five
  blockers. The blockers are Connectome's direct dependencies on seven
  physical/internal RRD crates, the CLI's direct dependencies on `rrd-core` and
  `rrd-store`, the absent exact-argv command proxy, the absent lifecycle
  supervisor, and the absent full-topology CI smoke test. `rrd-server`,
  `rrflow-mcp`, package identity, readiness routes, isolated state, the pinned
  local/CI Rust toolchain, the Cargo lock, Cargo, Rust, Git, Tailscale, and
  Cloudflare tooling pass their present checks.
- Verification:
  `cargo test -p rrflow-cli dev::tests::current_workspace_reports_real_boundary_blockers --offline`
  passed; `cargo clippy -p rrflow-cli --all-targets --offline -- -D warnings`
  passed; `cargo rrflow-dev doctor --root . --json` emitted the expected report
  and exited 1; `git diff --check` passed.
- Invariant: the canonical integration topology is one `rrd-server` authority;
  every other process is a protocol client. Embedded topology is a separate
  one-process profile. A supervisor may not start the current direct-store
  Connectome beside the daemon and call that integration.
- Next gate: define the contract-backed operator/diagnostic projection needed
  by Connectome, serve it from RRD, consume it through `rrd-client`, and remove
  Connectome's production physical-crate dependencies before implementing
  `rrflow dev up/status/logs/stop`.

### Development-topology truth correction

- Finding: report v1 accepted `surface.mcp-engine-boundary` because MCP had no
  physical-layer dependencies. That proved dependency hygiene only; it did not
  prove the report's canonical daemon-client topology. The binary still opened
  `RrdEngine` directly and accepted only `--db`/`--root`.
- Correction: report v2 preserves the passing dependency-boundary check and
  adds required `surface.mcp-daemon-mode`. The new gate requires the
  `rrd-client` dependency plus explicit `RuntimeMode` and `--url` daemon path.
  Current evidence is 13 passed, six blocked, zero warnings. No additional host
  tools are missing.
- Contract: `docs/rrflow-mcp-daemon-mode.md` freezes mutually exclusive
  embedded/daemon profiles, server-owned project binding, caller-preserving
  granular authorization, one generated catalogue/dispatcher, failure/replay
  semantics, conformance tests, and D0-D5 dependency order. It explicitly
  forbids a URL-only shortcut, duplicate registry, anonymous server execution,
  and a second database opener.
- Verification:
  `cargo test -p rrflow-cli dev::tests::current_workspace_reports_real_boundary_blockers --offline`,
  strict all-target CLI Clippy, and report-v2 execution passed locally. Doctor
  correctly exited 1 because the six blockers are real. Full workspace and
  remote CI remain unrecorded.

### D0 runtime invocation and granular policy contract

- Contract: added strict, bounded, versioned `RuntimeToolCatalogue`,
  `RuntimeToolDescriptor`, `RuntimeToolInvocation`, and
  `RuntimeToolInvocationResult` types. Invocation arguments and returned
  content carry verified SHA-256 digests; unknown fields, non-object arguments,
  oversize values, unsorted/duplicate catalogues, unsafe public actions, and
  version drift fail closed.
- Policy: replaced the runtime catalogue's binary-only policy description with
  an explicit engine-owned `SecurityAction` for all 28 tools. Existing query,
  transaction, vector, changefeed, backup, restore, estate, and audit actions
  are reused. Memory context/inspect/recall/retire/write, lifecycle, project
  attunement/routing, and reasoning read/write receive dedicated actions; no
  blanket runtime-invoke grant was added.
- Single registry: `runtime_tool_contract_catalogue()` projects the same 28
  executable definitions into the public wire representation and validates it
  before returning. Its test freezes every exact name-to-action pair and the
  sole public service-status operation.
- Schema review: the intentional `SecurityAction` expansion changed the
  generated OpenAPI digest to
  `5032de567b8ecdd08e99e5e0fe2b2c95250ba6100d517a7533d3b1213df64e1d`;
  the TypeScript OpenAPI types were regenerated and their drift check passes.
- Verification: all `rrd-contract` and `rrd-engine` targets passed, including
  the new contract and 28-action exhaustiveness tests. Strict all-target Clippy
  passed for contract, engine, MCP, and Connectome. This is local D0 evidence,
  not a daemon-mode, workspace, or remote-matrix claim.
- Next gate: D1 binds `rrd-server` to one persisted `InstanceBinding` and
  authoritative canonical project root before any runtime endpoint is added.

This is the durable engineering record for dependency-critical RRFlow/RRD work.
It complements, but does not replace, the authenticated runtime and control
journals stored by RRD itself.

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

## 2026-08-25 — engine-generated MCP memory surface and Connectome projection

- Commit: `pending`; this is verified local slice evidence, not a landed or
  cross-platform product checkpoint.
- Gate: R1 plus the first R2 increment from the active recovery board. The handwritten MCP catalogue was
  removed from `rrflow-mcp`; `rrd-engine::runtime` now owns each tool's name,
  description, JSON input schema, mutability classification, allow-list check,
  and executable dispatch.
- Memory operations: added claim-backed `rrflow_remember`, bounded
  `rrflow_context`, provenance/history `rrflow_inspect`, and history-preserving
  `rrflow_forget`. Forget appends a bitemporal retirement correction; it does
  not erase prior facts. Existing preflight, exact recall, route, RRFlowQL,
  reasoning, and lifecycle tools remain in the same generated catalogue.
- Conformance: MCP `tools/list` serializes the engine catalogue directly.
  Connectome's Capabilities screen consumes the same catalogue and shows every
  executable tool with its read/mutation classification. The stdio test asserts
  exact advertised/executable name parity and executes remember, inspect,
  context, forget, and post-retirement recall through a real MCP process.
- Cross-surface contract: `rrd-contract` now defines the five canonical product
  surfaces and their available/experimental/planned/not-applicable
  dispositions. `rrd-engine` generates 47 validated capability rows from all
  28 RRD HTTP operations, all 11 executable MCP tools, and eight explicit
  planned gaps. Validation requires sorted unique IDs, every surface on every
  capability, a real entrypoint for every available disposition, and no
  entrypoint for not-applicable dispositions. Connectome renders the generated
  matrix directly.
- Local evidence: `cargo test -p connectome-ui --test capabilities --offline`,
  `cargo test -p rrflow-mcp --all-targets --offline`, and `cargo clippy -p
  rrd-engine -p rrflow-mcp -p connectome-ui --all-targets --offline -- -D
  warnings` passed. `cargo test -p rrd-engine --test capability_catalogue
  --offline` also passed coverage for every HTTP operation, every executable
  runtime tool, and all eight planned foundation rows. The rebuilt local UI at
  `/api/runtime/capabilities` returned 11 tools and 47 capability rows.
- Limits: exact-subject recall is not hybrid lexical/vector recall. Document
  ingestion, collection deletion, reflection/synthesis, full CRUD/schema/graph
  administration, and the remaining RRD server operations are not advertised
  as executable MCP tools. Connectome's higher-level capability cards still
  require migration into the authoritative cross-surface catalogue.
- Next gate: implement the persisted file-tree attunement receipt required by
  R3, bind it to the shared mutation authorization path, and test stale/absent
  receipt denial before expanding R2 further.

## 2026-08-25 — durable planning attunement and one-tool authorization

- Commit: `pending`; local evidence only.
- Gate: R3. Preflight and user-prompt lifecycle events now record a hash-sealed
  `ProjectAttunementReceipt` through RRD's authoritative control-state CAS and
  hash-chained journal.
- Bound evidence: canonical project root, serialized project profile, stable
  source-tree digest, source-file count, routing generation/symbol count,
  content fingerprints for manifests, lockfiles, Biome config, project agent
  instructions, RRFlow instance/workflow policy, and GitHub Actions workflows,
  plus the RRD read stamp, issuing actor/time, and optional prompt digest.
- Enforcement: pre-tool refreshes the real source projection and compares it
  with the previously recorded receipt. Source edits, additions/removals,
  manifest-only drift, missing/corrupt receipts, and routing corruption fail
  closed until a new preflight. An allowed mutation atomically binds the
  receipt to one reasoning run and one exact tool-name/tool-input digest; a
  competing request is denied until matching post-tool observation consumes
  the authorization.
- Persistence: a native `PersistentEngine` test closes and reopens RRD between
  preflight and pre-tool authorization. The durable receipt remains valid and
  the gate allows the unchanged project after reopen.
- UI: Connectome's Capabilities view renders the receipt, source-tree digest,
  planning inputs, routing generation, RRD read stamp, prompt digest, reasoning
  binding, and any outstanding tool authorization. The local demo snapshot
  reported a verified receipt, 11 engine-generated MCP tools, and 47 declared
  product capability rows.
- Local evidence: `cargo test -p rrd-engine --all-targets --all-features
  --offline`, `cargo test -p rrflow-mcp --all-targets --offline`, `cargo test -p
  connectome-ui --all-targets --offline`, and strict Clippy over contract,
  engine, MCP, and Connectome passed. `git diff --check` passed.
- Contract correction: the broader audit caught a frozen public fixture still
  expecting implementation `rrflow` and a stale OpenAPI digest after the
  pre-release RRFlow contract cutover. The fixture now records `rrflow`, the
  reviewed OpenAPI SHA-256 is
  `7b10be80754894895996fbcc7de8b421b975b9224b19368df45c65b7a98674c2`,
  and all 18 `rrd-contract` tests plus strict Clippy pass.
- Limits: provider-neutral session/turn envelope work is still incomplete;
  arbitrary shell execution still needs the canonical exact-argv command proxy
  and environment/working-directory binding; Connectome and CLI still bypass
  lower physical crates. The current worktree is not a remote-matrix checkpoint.
- Next gate: R4/R5—move remaining Connectome capability cards behind the
  generated catalogue, then eliminate Connectome and CLI lower-crate bypasses
  before workspace-wide qualification.

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
  [`32666043965`](https://github.com/EonsofStupid/rrflow/actions/runs/32666043965)
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
  runtime transaction with its own metadata in one RRD LSM batch only after the
  plan lowers canonical staged keys to the target database's authenticated
  application format. The raw staged-parts method is no longer public.
- Verification:
  `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test -p rrd-store -p rrd-cluster --all-features --locked`
  passed; strict all-target/all-feature clippy passed. GitHub Actions run
  [`32667681611`](https://github.com/EonsofStupid/rrflow/actions/runs/32667681611)
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
  [`32668383982`](https://github.com/EonsofStupid/rrflow/actions/runs/32668383982)
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
  [`32671973317`](https://github.com/EonsofStupid/rrflow/actions/runs/32671973317)
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
  real RRFlowQL parser and RRD query executor catalogue, binder, planner, and executor rather
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
  mutating RRFlowQL, typed public multi-model transactions, live subscriptions,
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
  is not yet a complete read-your-writes graph projection. Mutating RRFlowQL,
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
  candidate set, and runs the existing RRFlow vector planner and exact oracle.
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
  audit owner instead of placing policy inside RRD LSM, query execution, RRO,
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
  allows the granted exact RRFlowQL query, denies an ungranted backup with 403,
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
  lifecycle, transactions, RRFlowQL, vector search, changefeed, managed backup/
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

## 2026-08-24 — RRFlowQL series and geospatial read foundation

- Commit: `9ca9d1f` (`feat(query): expose series and geo in RRFlowQL`).
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
- Evidence: parser corpus/canonicalization, eight RRD query executor tests, three-engine
  series/geo differentials, pre-observation/pre-validity exclusion, the secured
  real-RRD atomic-data test, and strict Clippy pass.
- Limit: recursive traversal, general numeric/spatial operators, indexes and
  statistics, full text, mutating/multi-statement RRFlowQL, streaming batches,
  and push live subscriptions remain open F6 breadth.

## 2026-08-24 — bounded recursive graph traversal

- Commit: `4740d7f` (`feat(query): add bounded graph traversal`).
- Grammar: added `traverse:<relation> START <kind>:<id> DIRECTION
  <OUTGOING|INCOMING|BOTH> DEPTH <1..32>` as a typed RRFlowQL source. Canonical
  rendering round-trips and malformed direction/depth inputs fail with offsets.
- Binding: the relation and start record kinds must exist at the captured schema
  revision and the start kind must be an allowed relation endpoint.
- Execution: breadth-first traversal reads the same explicit valid/known graph
  snapshot, processes relations in canonical identity order, tracks visited
  nodes to terminate cycles, and emits the deterministic first shortest path
  with depth, node, edge, endpoint, and complete path fields.
- Evidence: nine RRD query executor tests, parser corpus/mutation tests, strict Clippy, a
  cyclic graph fixture, outgoing/incoming three-engine differentials, and the
  secured real-RRD atomic-data/query test pass.
- Limit: traversal currently returns nodes/paths for one relation type and does
  not yet implement path predicates, weighted paths, all-path enumeration, or
  index-assisted expansion. Index/realtime breadth remains next.

## 2026-08-24 — typed scalar comparison predicates

- Commit: `bfa4e15` (`feat(query): add typed comparison predicates`).
- Contract: filters now retain `=`, `!=`, `<`, `<=`, `>`, or `>=` through the
  typed RRFlowQL AST, canonical text, bound logical plan, physical-plan digest,
  and reference executor. Older serialized equality filters remain readable
  through an equality default.
- Safety: equality and inequality use exact typed values. Ordering accepts only
  same-type integer, unsigned integer, and string values; decimal, null,
  boolean, structured, and mixed-type ordering fail during binding rather than
  receiving implicit coercion or lexical numeric semantics.
- Planning: event-cursor point lookup remains eligible only for equality, so a
  range predicate cannot accidentally inherit a point-access contract.
- Evidence: parser canonical/malformed/mutation tests, plan goldens, a truth
  table for every operator, three-engine persistent differentials, unsupported
  decimal ordering rejection, the authenticated real-RRD process query, and
  strict Clippy all pass.
- Limit: this is predicate algebra, not an index claim. Index catalogues,
  maintenance, selection evidence, statistics, spatial/full-text operators,
  and live subscriptions remain the next F6 foundation slices.

## 2026-08-24 — persistent index catalogue and lifecycle

- Commit: `3263075` (`feat(query): add persistent index catalogue`).
- Authority: compound scalar index definitions are schema-bound before write
  and stored through authoritative control-state compare-and-swap. Create,
  rebuild, ready publication, quarantine, and retirement each enter the
  authenticated control journal.
- Freshness: the shared projection stamp binds definition/configuration,
  generation, artifact digest, lifecycle state, and exact per-scope source
  cursor. Rebuild increments the generation and stale publishers are fenced.
- Planning: matching leading filter fields emit an `index:<id>` candidate with
  generation, lifecycle, freshness, and matched-prefix evidence. It remains
  explicitly unselected/non-exact until a verified artifact reader exists, so
  the authoritative log stays the only answer path.
- Integration correction: the full workspace run found that `rrd-engine` trace
  identity had not been extended for the earlier series, geo, and traversal
  sources. Those source identities are now explicit rather than hidden behind
  a wildcard.
- Evidence: lifecycle differentials pass on memory, Fjall compatibility, and
  native RRD LSM; stale-generation/invalid-field denial, hash-chain verification,
  planner rejection evidence, and native reopen pass. Strict Clippy passes for
  RRD query executor/RRFlow Node, and the complete workspace test suite is green.
- Limit: no scalar artifact is built or read yet, uniqueness is not enforced,
  and no public RRD index route exists. Those are the next index slices before
  live-query delivery.

## 2026-08-24 — resumable semantic live-query deltas

- Commit: `82323ea` (`feat(query): add resumable semantic live deltas`).
- Semantics: one `KNOWN HEAD` query is evaluated at an explicit resume cursor
  and one captured head, then rows are deterministically classified as added,
  updated with before/after values, or removed by stable identity.
- Resume contract: results carry from/through/head cursors and a SHA-256 binding
  of query contract version, canonical query, and typed parameters. Repeating
  `through_cursor` yields an empty poll until the log advances.
- Safety: future resumes, fixed-known queries, zero delta budgets, truncated
  snapshots, duplicate identities, and oversized deltas fail closed.
- Evidence: additions, updates, removals, empty replay, cursor validation, and
  budget behavior are identical on memory, Fjall compatibility, and native
  RRD LSM; all RRD query executor tests and strict Clippy pass.
- Limit: this is the semantic engine, not yet public realtime. The authenticated
  RRD route, changefeed wakeup, streaming/backpressure, retained subscriptions,
  and all six SDK surfaces remain the next delivery slice.

## 2026-08-24 — authenticated semantic live-query polling

- Commit: `28a4362` (`feat(rrd): expose semantic live query polling`).
- Public boundary: `POST /v1/query/live/poll` now accepts one strict bounded
  resume request and returns the RRD query executor added/updated/removed delta through one
  captured head. Scope, query, parameters, execution budget, and delta budget
  are validated before execution.
- Security: the route requires a live session and the distinct
  `query_live_poll` action. A grant for ordinary query execution does not imply
  permission to poll live results.
- SDK authority: the deterministic OpenAPI contract now publishes 22
  operations and regenerated route catalogues cover Rust, TypeScript, Python,
  Go, Java, and .NET. TypeScript's generated payload surface includes the new
  strict request/result schemas.
- Evidence: the real RRD process denies an unauthenticated poll and returns the
  exact persisted series-row addition after authentication. Contract,
  security, server, and Rust-client suites pass; strict Clippy passes; all five
  non-Rust SDK generation/format/type/build/test gates pass; and
  `cargo check --workspace --all-targets` is green.
- Limit: this is resumable polling, not push delivery. Changefeed-assisted
  wakeup, streaming/backpressure, retained subscription leases, and ergonomic
  typed helpers in every SDK remain open.

## 2026-08-24 — verified exact scalar index artifacts

- Commit: `600873e` (`feat(query): serve verified scalar index artifacts`).
- Build path: a building catalogue generation executes the all-fields reference
  query at one captured head and explicit valid-time, then durably publishes
  canonical rows under a content-addressed projection name before marking the
  generation ready.
- Selection: a matching index is exact only when source cursor and valid-time
  equal the query, lifecycle is ready, and artifact coverage is present. The
  planner chooses longest matching prefix with stable identity tie-breaking;
  newer/older cursors or different valid-times retain the authoritative scan.
- Execution: the reader independently verifies the captured read stamp, bytes,
  SHA-256, scope, definition/configuration, generation, cursor, schema revision,
  valid-time, and row count before reapplying the shared filter/projection
  evaluator.
- Evidence: real open/closed document rows select and execute through the
  artifact identically on memory, Fjall compatibility, and native RRD LSM; a
  subsequent write proves stale fallback; native reopen serves the artifact;
  corrupted bytes fail closed. RRD query executor strict Clippy and the full workspace test
  suite pass.
- Limit: this is an exact snapshot artifact, not incremental maintenance.
  Uniqueness, public RRD administration, count/spatial/full-text index families,
  and historical-valid-time artifact policy remain open.

## 2026-08-24 — authenticated query-index administration

- Commit: `95be4ed` (`feat(rrd): add query index administration`).
- Public surface: `POST /v1/query/indexes/ensure` parses a restricted RRFlowQL
  definition, synchronously creates/rebuilds the content-addressed exact
  artifact, and returns its generation, source cursor, valid-time, row count,
  configuration digest, artifact digest, and lifecycle state.
  `POST /v1/query/indexes/list` returns the stable authoritative catalogue.
- Lifecycle: accepted ensure operations persist a bounded operation-digest and
  entry snapshot under the caller's idempotency key. Exact retry replays;
  changed payload under the same key conflicts. Building work resumes, stale
  ready work rebuilds, and quarantined/retiring state requires explicit
  recovery. Unique requests are denied until the write path enforces them.
- Security/SDKs: ensure and list have separate deny-by-default actions. The
  frozen OpenAPI digest now describes 24 operations, and regenerated Rust,
  TypeScript, Python, Go, Java, and .NET catalogues include both routes.
- Evidence: a real RRD process denies unauthenticated ensure, builds over the
  atomic multi-model fixture, replays the result, rejects collision, lists it,
  and then returns a query plan selecting `index:document-title` with the exact
  row. Contract/security/server/client/RRD query executor tests, strict Clippy, all five
  non-Rust SDK gates, and workspace all-target compilation pass.
- Limit: ensure is synchronous and exact-snapshot only. Incremental maintenance,
  concurrent same-key convergence, uniqueness, background jobs, and broader
  index families remain open.

## 2026-08-24 — bounded semantic live-query waiting

- Commit: `bf6ec7b` (`feat(rrd): add bounded live query waiting`).
- Delivery: the existing resumable live-query request now accepts an optional
  wait bounded to five seconds. Immediate mode remains the zero default. A wait
  returns as soon as the authoritative cursor advances—even when the semantic
  delta is empty—or returns explicit `timed_out` and `waited_ms` evidence at the
  same cursor.
- Safety: HTTP preflight rejects a declared wait that can outlive the absolute
  request deadline. Every retry still evaluates the exact query from the
  caller's unchanged resume cursor to one newly captured head; no transient
  server subscription state becomes authoritative.
- Evidence: a real server holds a poll at cursor three, wakes after a separate
  authenticated transaction updates the record at cursor four, and returns one
  exact before/after row change. A second wait times out at cursor four without
  inventing changes. Contract bounds, complete server/client suites, strict
  Clippy, TypeScript generation/Biome/types/tests, and OpenAPI drift checks pass.
- Limit: this is bounded long-poll delivery, not SSE/WebSocket streaming.
  Retained subscription leases, per-subscriber backpressure, streaming
  cancellation, and ergonomic helpers across every SDK remain open.

## 2026-08-24 — persistent vector collections and named spaces

- Commit: `e7945bb` (`feat(vector): add persistent collection administration`).
- Authority: `rrd-vector` now owns a versioned per-scope collection catalogue
  in CAS-protected control state. Each transition enters the authenticated
  control journal; bounded idempotency receipts survive reopen, exact retries
  replay, changed payloads conflict, and concurrent same-operation losers
  converge on the winning receipt.
- Contract: a collection contains unique named dense, sparse, or multi-dense
  vector spaces binding field, dimensions, metric, optional model digest, and
  pinned/cached/cold placement policy. Search can address collection plus vector
  name and validates kind/dimensions before using the exact oracle. The legacy
  direct field/metric address remains accepted for protocol compatibility.
- Public surface: authenticated `POST /v1/vector/collections/ensure` and
  `/list` have distinct deny-by-default actions. The reviewed OpenAPI digest is
  `1d25005eeccb5d39ebd927ee842036e2d8fbf74d21a33e3887d73c8b6b0c3d50` and
  now describes 26 operations; Rust and all five generated SDK route surfaces
  include both operations.
- Evidence: native reopen preserves collection generation, receipt, and journal
  history. A real RRD process proves unauthenticated denial, ensure, exact
  replay, 409 collision, catalogue list, collection-addressed exact search, and
  dimension-mismatch denial. Vector/contract/server/client suites and strict
  Clippy pass; TypeScript, Python, and Go complete their generation and quality
  gates. Java/.NET generation drift checks pass; their runtime suites were not
  rerun because Maven and the .NET SDK are absent from this host.
- Limit: this does not yet implement point/payload administration, payload
  indexes, physical memory-tier enforcement, persistent HNSW serving,
  inference, or TurboQuant.

## 2026-08-24 — collection-bound atomic vector point writes

- Commit: `537f27e` (`feat(vector): bind point writes to named collections`).
- Contract: `put_vector` additively accepts paired `collection_id` and
  `vector_name` coordinates. Unpaired coordinates fail strict validation; the
  reviewed OpenAPI digest is now
  `d31ea8c4d50edb1c3b1640abf55fc18657ba49ac134785c4d3f682d7c4af3c2f`.
- Commit gate: after authenticating the session and before recording commit
  intent, RRD resolves every addressed vector against the persistent catalogue
  and validates field, dense/sparse/multi-dense kind, dimensions, and required
  model name/digest provenance. The existing vector properties remain the
  point payload and commit atomically with records, relations, events, claims,
  series, geo, and object references.
- Evidence: the real multi-model process fixture now creates the collection
  before committing its point, binds the vector to `documents/title`, then
  searches it through the same catalogue. Strict contract pairing, server
  compilation, focused real-process execution, TypeScript generation/ArkType/
  Biome/type/tests, and strict Clippy pass.
- Limit: this reuses the unified ACID transaction authority and does not invent
  a parallel point store. Dedicated retrieve/delete/scroll/batch point routes,
  payload-index administration, and collection-aware SDK convenience methods
  remain open.

## 2026-08-24 — public typed vector payload filters

- Commit: `8725f38` (`feat(vector): expose bounded payload filters`).
- Contract: vector search now accepts equality, inequality, membership, range,
  existence, and recursive all/any/not filters bounded to depth 32 and 4,096
  nodes. Public `QueryValue` operands lower into the existing exact
  `rrd-vector` evaluator, avoiding a second transport-only semantics path.
- Schema correction: recursive filter definitions initially exposed a genuine
  OpenAPI generator defect. `VectorPayloadFilter` is now a canonical component
  alongside `QueryValue`, all local references are rebased, and the reviewed
  OpenAPI digest is
  `a6809faf79e177c4196f9697c5b66ec26ecd9946c295a18c4eaa5f6aaa70807e`.
- Evidence: the real collection-bound point carries a tenant payload; an exact
  matching filter returns it and a non-matching filter returns zero hits.
  Complete contract/server suites, focused process behavior, strict Clippy,
  and TypeScript generation/Biome/types/tests pass.
- Limit: exact filtering is public, but payload-index administration and
  persisted filter-aware HNSW serving are not yet implemented.

## 2026-08-24 — deterministic collection point scrolling

- Commit: `6b86963` (`feat(vector): add deterministic point scrolling`).
- Shared semantics: `rrd-vector` now exposes one validated visibility request
  and `materialize_visible` primitive. Exact search and point scroll both use
  it for read-stamp scope, latest transaction-visible version, valid time,
  field, model binding, retirement, and payload filtering.
- Public route: authenticated `POST /v1/vector/points/scroll` has a distinct
  deny-by-default action and returns bounded reference-ordered point pages with
  vector value, provenance, payload, source cursor, read manifest, known cursor,
  truncation, and resume reference. The reviewed OpenAPI digest is
  `91a5681109cf45c4d544856733533acb3e44bb70630cb1ba03c048aaaf164da7`
  across 27 operations; every generated SDK route catalogue includes it.
- Evidence: the real server denies an unauthenticated scroll and, after
  authentication, returns the collection-bound point and tenant payload at the
  exact cursor. Vector tests, strict Clippy, Rust client, TypeScript, Python,
  and Go quality gates pass; Java/.NET generation drift checks pass.
- Limit: this slice does not yet add direct point retrieval or a first-class
  deletion/tombstone mutation.

## 2026-08-24 — exact collection point retrieval

- Commit: `1709a12` (`feat(vector): add exact point retrieval`).
- Contract: `POST /v1/vector/points/retrieve` accepts 1..=4,096 unique typed
  references and one collection/vector/valid-time coordinate. It returns found
  points in caller order and an explicit missing-reference list under one read
  manifest/cursor, rather than making omission ambiguous.
- Semantics/security: retrieval resolves the persistent named-vector contract,
  scans within the declared bound, and uses the same shared visibility
  primitive as search and scroll. It has its own deny-by-default action.
- Evidence: the real process denies unauthenticated retrieval, then returns one
  stored point and one explicit missing identity after authentication. Contract,
  server, strict Clippy, TypeScript, Python, Go, and all route-generation gates
  pass. The reviewed 28-operation OpenAPI digest is
  `68d87505b85d7d99e6a58ce49404040d832295c6572f70f791db635dd45bbf64`.
- Limit: first-class point deletion/tombstones remain open.

## 2026-08-24 — Connectome RRD connection authority

- Commit: `e21eb6e` (`feat(connectome): add persistent RRD connection profiles`).
- Contract: the local panel now owns a bounded connection catalogue with stable
  profile identities, modes, exact RRD instance identities, endpoints, and
  credential references. Secret values are not part of the contract.
- Semantics: a loopback RRD profile is persisted only after the supported Rust
  client negotiates protocol/version and verifies the returned instance. The
  catalogue advances through control-state CAS plus the hash-chained journal;
  request replays converge and idempotency-key rebinding fails.
- UI: the real Connections workspace shows the embedded authority, retained RRD
  profiles, generations, live negotiated implementation/capability evidence,
  and the remaining authenticated-source-switching boundary.
- Evidence: 17 Connectome Rust unit/integration tests pass, including a live
  socket capability negotiation; strict Clippy and JavaScript syntax checks
  pass. The pre-slice F8/F9 audit also passed 64 cluster/Connectome tests,
  including independent-process Raft recovery and mTLS transport.
- Limit: remote HTTPS profiles validate structurally but are not probed until
  the F4 TLS client exists. Profiles do not yet establish authenticated data
  sessions or select a remote RRD as the panel's active data source.

## 2026-08-24 — deterministic MSE TurboQuant codec

- Commit: `ff02cbc` (`feat(vector): add deterministic TurboQuant codec`).
- Codec: `TurboQuantVector` supports fixed standard-normal Lloyd-Max 4/2/1/1.5
  bit modes, deterministic seeded randomized Hadamard rotation, bit packing,
  original/centroid norm correction, reconstruction, and asymmetric dot,
  cosine, Euclidean, and Manhattan scoring. The 1.5-bit mode rotates a 1.5x
  padded coordinate space before one-bit coding.
- Evidence: frozen determinism/packing/size tests cover all modes; irregular
  dimensions prove norm-preserving invertibility; malformed/non-finite inputs
  fail closed; a fixed exact-oracle Recall@10 gate covers the four-bit path.
- Limit: this is the practical MSE variant, not the paper's residual QJL
  estimator. SIMD and broad production-corpus quality/latency evidence remain
  open.

## 2026-08-24 — authenticated planner-visible TurboQuant artifacts

- Commit: `6308362` (`feat(vector): serve authenticated TurboQuant artifacts`).
- Artifact: the custom bounded binary format separates canonical JSON metadata
  from packed vector bytes, authenticates both under one SHA-256 identity, and
  retains temporal identity, payload-filter properties, cursors, model binding,
  and norm corrections without storing full-f32 vector payloads in the
  artifact.
- Serving: TurboQuant is a typed approximate access path in the shared
  catalogue/planner/runtime. It revalidates scope, field, metric, dimensions,
  model, filter coverage, generation, source cursor, and artifact kind before
  proposing candidates; the authoritative exact-f32 oracle performs final
  reranking.
- Evidence: binary reopen, byte accounting, payload filtering, corruption and
  stale-read denial, every-artifact codec roundtrip, planner selection, exact
  reranking, affected server/node/vector tests, full-workspace compilation, and
  strict Clippy pass.
- Limit: no public build/lifecycle route, SIMD/mmap kernel, physical tier
  placement, or production-scale recall/bias/latency/recovery matrix is claimed.

## 2026-08-24 — remote RRD TLS 1.3 mutual-auth boundary

- Commit: `6286ee4` (`feat(rrd): add remote mutual TLS transport`).
- Server: plain HTTP remains loopback-only. The separate mTLS constructor and
  CLI mode require a server chain/key/client CA together, require an initialized
  `rrd-security` authority before opening the listener, and restrict Rustls to
  TLS 1.3. The capability handshake distinguishes unavailable cleartext remote
  listen from the experimental authenticated mode.
- Client: `rrd-client` now has a distinct HTTPS transport accepting explicit
  Rustls identity/trust configuration. It retains the same bounds, retry rules,
  envelope validation, and exact instance negotiation as local HTTP.
- Evidence: a real TLS socket accepts the trusted client, denies a client with
  no certificate, denies a valid client certificate when the server name is
  wrong, and reports `remote-listen` as experimental. Complete RRD server/client
  suites, strict Clippy, and full-workspace compilation pass.
- Limit: certificates are startup-loaded. Live rotation, CRL/OCSP, secret-
  provider/Kubernetes integration, certificate-to-principal binding, HTTP/2,
  and distributed qualification remain open.

## 2026-08-24 — offline security-authority bootstrap

- Commit: `7a48c32` (`feat(security): add offline authority bootstrap`).
- Surface: `rrd-security-bootstrap` accepts an explicit database, instance,
  versioned JSON manifest, and timestamp. Principal entries reference absolute
  mounted credential files rather than embedding credentials.
- Safety: manifest and credential reads are bounded; paths may use Kubernetes
  projected-secret symlinks only inside their mount; credential files must deny
  other access and group write/execute on Unix. Only SHA-256 digests enter the validated
  persistent `SecurityState` and authenticated control transition.
- Recovery: the same material is an idempotent no-op after process/database
  reopen. Any changed policy or credential fails without replacing the initial
  authority.
- Evidence: the black-box process test initializes, repeats, reopens, verifies
  secret absence, changes the credential, and observes drift denial. Strict
  Clippy passes.
- Limit: authorized ongoing principal/policy mutation and secret rotation APIs
  remain open; this command owns initial offline provisioning only.

## 2026-08-24 — Kubernetes RRD operator foundation

- Commit: `20dafe6` (`feat(kubernetes): add secured RRD operator baseline`).
- API: `rrflow.io/v1alpha1 RrdInstance` is a generated namespaced structural
  CRD with status subresource and CEL rules for contract version, digest-pinned
  image, retained storage, bootstrap time, names, and conservative quantities.
- Controller: kube-rs supplies watch/relist recovery, owned-StatefulSet events,
  finalizer handling, server-side apply, status updates, and bounded retry. The
  finalizer removes owned workload/network resources while StatefulSet PVC
  retention preserves data.
- Workload: deterministic resources include headless/client Services, one
  secured StatefulSet with offline bootstrap and mTLS, PDB, and default-deny
  NetworkPolicy. Pods are non-root, drop capabilities, use a read-only root,
  omit service-account tokens, and carry explicit resource bounds.
- Packaging: checked CRD, least-privilege operator RBAC/Deployment template,
  and example instance are under `deploy/kubernetes`; the placeholder image
  digest must be replaced by a published release artifact.
- Evidence: generated/checked CRD equality, structural/status/CEL assertions,
  RBAC exclusion of Secrets/exec/wildcards, deterministic resource/digest
  replay, and unsafe image/storage/name/identity denial pass with strict Clippy.
- Limit: one RRD pod is intentional. Public RRD is not yet integrated with the
  separate Raft state machine, so no Multi-AZ, safe upgrade, CSI recovery,
  certificate rotation, or real-cluster qualification is claimed.

## 2026-08-24 — local-process identity discovery hardening

- Commit: `8c76532` (`fix(estate): tolerate bounded pre-exec identity observation`).
- Failure: the Linux pull-request matrix intermittently observed the test
  harness image for a newly spawned RRD PID before the target executable image
  became visible, then rejected startup immediately. The identical push job
  passed, proving the process-boundary test was nondeterministic rather than the
  documentation-only commit being platform-verified.
- Safety: a mismatched executable is still never authenticated, recorded, or
  treated as owned. Discovery now waits only within the existing three-second
  bounded window for the trusted executable identity. A child that exits is
  reported immediately; a mismatch that remains at the deadline is killed,
  reaped, and denied permanently.
- Local evidence: the edited file passes `rustfmt --check`; the exact fallback
  test passed four consecutive focused executions; all `rrd-estate` tests, the
  complete four-test `local_estate_driver` suite, and strict Clippy for
  `rrd-estate` plus `rrd-server` passed.
- Remote evidence: [push CI 32752929744](https://github.com/EonsofStupid/rrflow/actions/runs/32752929744)
  and [pull-request CI 32752934173](https://github.com/EonsofStupid/rrflow/actions/runs/32752934173)
  passed the full verification job and Linux, macOS, and Windows process
  matrix, including the formerly flaky child/controller recovery test.
- Limit: `cargo fmt --all -- --check` remains blocked by unrelated existing and
  concurrent formatting differences, including the unfinished maintenance
  slice; those files were not reformatted or staged with this fix.

## 2026-08-24 — RRD engine ownership and catalogue-stamped transactions

- Commit: `pending` (this checkpoint).
- Identity: the physical persistence package and Rust namespace moved from
  `rrd-store`/`rrflow_store` to `rrd-store`/`rrd_store`. No alias package or
  forwarding namespace remains. Existing durable byte magics and digest
  domains were deliberately not rewritten without a versioned recovery plan.
- Composition: `rrd-engine` is the concrete embedded composition root and
  `rrd-server` now depends in production only on that engine and the public RRD
  contract. Server HTTP and command implementations were split into bounded
  responsibility modules; estate and security commands moved to their owning
  packages.
- Transaction authority: session transactions retain the complete persisted
  `ReadStamp` and commit through `DataTransaction`. Query-index and vector-
  collection mutations now update a per-scope catalogue revision atomically
  with their materialized control record and hash-chained journal entry.
  Catalogue changes alter the semantic manifest without advancing the data
  cursor, and stale transactions fail closed on every backend.
- Security: embedded engine operations re-evaluate persistent grants, and a
  regression test proves a session-create-only principal cannot invoke backup
  creation through the embedded API. Public engine errors no longer expose an
  `rrd-store` error type.
- Evidence: `cargo test --workspace --all-features --exclude rrd-maintenance
  --locked` passed after catching and correcting an integration fixture that
  opened a transaction before installing its vector catalogue. `cargo clippy
  --workspace --all-targets --all-features --exclude rrd-maintenance --locked
  -- -D warnings` passed. Differential catalogue tests cover memory, Fjall
  compatibility, native persistence, reopen, unrelated scopes, stable data
  cursors, changed manifests, and stale-read rejection.
- Limits: HTTP still duplicates parts of security policy and completion-audit
  orchestration; Connectome, CLI, and MCP still have frozen direct-component
  dependency debt; remaining RRFlow-named packages and durable format identities
  require dependency-ordered migration. The postponed `rrd-maintenance` work
  was excluded from this checkpoint and was not staged.
- Next gate: introduce the transport-neutral invocation boundary using the
  existing RRD `RequestContext`, then move policy decisions and durable audit
  completion wholly into `rrd-engine` before removing HTTP authority.

## 2026-08-25 — named-vector persistence and MCP A8-A12

- Scope: completed the existing-engine vector adapter slice without creating a
  second MCP catalogue or vector store. Generated MCP discovery now contains 23
  executable tools; A13-A17 remain planned.
- Persistence correction: `RuntimeVector` now carries an optional validated
  collection/vector address. Transaction lowering retains the public
  `collection_id`/`vector_name` pair, collection reads filter on that exact
  address before temporal visibility, and compact-dense/TurboQuant metadata
  preserve it. Legacy rows deserialize with no address and remain readable
  through legacy field search, but do not match a named collection.
- Executable adapters: collection ensure/list, exact point retrieve,
  deterministic point scroll, and exact dense/sparse/multi-dense search all use
  the engine-owned typed catalogue, ordinary RRD session/security boundaries,
  and the existing endpoint capability rows. Mutation attunement and
  idempotency remain mandatory for collection ensure; read-session mechanics
  are internal and bounded.
- Cohesive evidence: one integration commits multi-model data, creates
  `documents/title`, commits a collection-bound point through the shared
  transaction authority, reopens RRD, and retrieves, scrolls, and searches it.
  A legacy unbound vector sharing `title-embedding` is explicitly missing and
  excluded. Exact replay after lease expiry/reopen remains covered for the
  collection mutation.
- Verification: all `rrd-core`, `rrd-vector`, and `rrd-engine` targets pass;
  MCP's three stdio integration tests and initialized-security denial test pass;
  strict Clippy with `-D warnings` passes for core, vector, engine, and MCP. The
  renamed core golden fixture was regenerated because its visible command had
  changed from `rrd-core` to `rrd-core` while its downstream hash-chain
  digests had not been recomputed.
- Limit: this is focused local evidence, not a workspace or remote matrix
  checkpoint. MCP search truthfully reports the exact path. Collection-bound
  HNSW/TurboQuant selection, full-text/hybrid retrieval, A13-A17, daemon-mode
  MCP, Connectome zero-bypass, and the development supervisor remain open.

## 2026-08-25 — MCP A13-A17 administration adapters

- Scope: completed the reviewed existing-engine adapter map. The sole dynamic
  MCP catalogue now contains 28 executable tools; a conformance test freezes
  that reviewed count and each A13-A17 capability mapping.
- Backup: `rrflow_backup_create` requires exact attunement and idempotency and
  calls the existing authenticated logical archive/catalogue authority.
  `rrflow_backup_list` optionally verifies every catalogued archive before
  returning it.
- Restore: `rrflow_restore` requires exact attunement, idempotency, and the
  literal `restore_to_new_root` acknowledgement. Only a canonical restore ID is
  accepted; RRD derives the isolated root and the model cannot provide a path
  or overwrite the active database.
- Operations: `rrflow_estate_read` exposes a persistent estate's desired,
  observed, activity, lease, operation, and receipt state.
  `rrflow_audit_read` exposes a bounded page of the persistent security audit
  journal. Neither adapter owns a second control-plane document or log.
- Cohesive evidence: the integration stores a fact, creates a logical backup,
  reopens and replays it, verifies the catalogue, restores to a new root, opens
  the restored engine, and reads the fact. It also reads a persisted estate and
  a persisted audit event. All engine targets, MCP's three stdio tests,
  initialized-security denial, and strict engine/MCP Clippy pass locally.
- Limit: the 28-tool adapter map is locally executable, but the full A-series
  exit gate remains open. A subsequent interactive stdio test now calls every
  implemented domain, and Connectome parity passes against the same generated
  catalogues. Initialized security has denial evidence but MCP lacks
  credentialed daemon-mode allowance, and no full workspace or remote platform
  matrix was run for this worktree.

### Process-level MCP and Connectome parity closure

- The new interactive stdio harness executes data commit, query-index ensure/
  list, live query, changefeed, vector collection/point/retrieve/scroll/search,
  logical backup/list, dependent restore-to-new-root, estate read, and audit
  read through the actual `rrflow-mcp` binary.
- The harness reads each JSON-RPC response before issuing dependent work; the
  restore request uses the actual backup digest instead of fixture knowledge or
  a direct engine bypass.
- Connectome's capability test passes against the exact 28-entry runtime
  catalogue and validates the engine-owned product surface catalogue. Strict
  Clippy passes for MCP and Connectome.
- Daemon-mode MCP cannot be implemented as a URL flag alone: `rrd-client`
  covers public data operations, while preflight/reasoning currently require a
  project root and have no `rrd-server` protocol. Opening an embedded runtime
  store beside the daemon would create two authorities and is prohibited. The
  daemon runtime protocol therefore requires a reviewed project binding,
  authenticated runtime invocation contract, and granular tool authorization.

## 2026-08-25 — D1 project-bound server and deployment authority

- Server authority: the production `rrd-server` binary now requires one
  project `--root`, discovers its `InstanceBinding`, opens only the canonical
  `.rrflow/rrd` store, and persists an immutable project/store binding in RRD.
  An ordinary restart verifies that binding; copying or moving the project and
  database cannot silently establish a new authority.
- Explicit provisioning: `rrd-server initialize --root PROJECT --instance ID`
  atomically creates or verifies a dedicated instance manifest. It refuses an
  existing different identity and is an explicit deployment action rather than
  a side effect of serving.
- Estate integration: local deployment catalogue format 2 authenticates one
  executable and declares bounded typed preparation arguments. The driver runs
  project initialization with a ten-second deadline and retained logs, then
  launches the same executable with `--root`; the deleted server `--db` and
  `--instance` launch path is no longer generated.
- Kubernetes integration: the StatefulSet now initializes project authority,
  runs security bootstrap against the canonical project store, and serves the
  same project root. This is deterministic manifest evidence, not a live
  cluster or distributed qualification claim.
- Evidence: all eight runtime-instance tests pass; all `rrd-estate` and
  `rrd-server` all-target tests pass, including the four 96-second real child,
  graceful fallback, forged-PID, and controller crash/reopen tests; all
  `rrd-kubernetes` contract/CRD tests pass; strict all-target Clippy with
  `-D warnings` passes for engine, estate, server, and Kubernetes.
- Remaining daemon sequence: D2 caller-preserving dispatch, D3 authenticated
  runtime protocol/client routes, D4 mutually exclusive MCP embedded/daemon
  modes, then D5 Connectome/CLI attachment and supervised topology. No full
  workspace or remote platform matrix was run for this worktree.

## 2026-08-25 — D2 caller-preserving runtime dispatch

- Engine authority: every one of the 28 runtime tools now resolves to a typed
  `RrdOperation` and exact `SecurityAction`. API-key callers create
  principal-bound nested sessions; session callers retain their existing
  session identity; unsecured embedded calls remain explicitly anonymous.
- Audit: successful allowance, execution completion, authenticated policy
  denial, and nested execution preserve the original principal. Failed
  credentials never manufacture a trusted principal. Exact serialized payload
  digests are checked before execution.
- Evidence: all `rrd-security` and `rrd-engine` targets pass, including the
  exhaustive name/action/mutation mapping and API-key/session/denial/digest
  integration tests. Strict all-target Clippy passes for both crates.

## 2026-08-25 — D3 authenticated runtime protocol and Rust client

- Public contract: the endpoint catalogue now contains 30 operations. The
  governed catalogue read has a dedicated `RuntimeToolCatalogueRead` action.
  Runtime invocation uses an explicit `EndpointAction::RuntimeToolDescriptor`
  policy source; there is no grantable generic dispatch action.
- Server boundary: `POST /v1/runtime/tools/list` and
  `POST /v1/runtime/tools/invoke` require session transport. Invocation
  independently resolves the selected engine-owned descriptor, revalidates
  its mutation/idempotency requirement, preserves the session principal, and
  uses only the server's persisted project root.
- Rust client: catalogue fetch returns a validated versioned descriptor set.
  Invocation requires that fetched catalogue and derives its client-side
  mutation requirement from the selected descriptor; the server resolves it
  again rather than trusting the client.
- Security evidence: a project-bound real-server test permits catalogue read
  and `rrflow_service_status` with exact grants, denies `rrflow_context`
  without `memory_context_read`, and observes the denied `rust-sdk` principal
  in durable audit. Server, client, contract, engine, security, and MCP target
  suites pass locally; the real child-process server/estate suite also passes.
- Generated surfaces: TypeScript, Python, Go, Java, and .NET maps now contain
  both runtime operations. TypeScript, Python, and Go drift/format/type/test
  gates pass. Java and .NET drift checks pass, but their build/test gates were
  not run because this host lacks `mvn` and `dotnet`.
- Remaining daemon sequence: D4 mutually exclusive MCP modes, then D5 outward
  client attachment/supervision. This is targeted local evidence, not a full
  workspace or remote-platform checkpoint.

## 2026-08-25 — D4 mutually exclusive MCP authorities

- Mode contract: `rrflow-mcp` now requires either `embedded --db PATH --root
  PROJECT` or `daemon --url http://LOOPBACK:PORT --instance ID --principal ID
  --api-key-file ABSOLUTE_PATH`. Missing, mixed, duplicate, unknown, and
  cross-mode arguments fail before MCP protocol processing. No compatibility
  parser or inferred mode remains.
- Single authority: embedded mode validates and opens the project-bound engine.
  Daemon mode accepts no database/project path and uses only `rrd-client` over
  the D3 routes. Both render `tools/list` from the same validated public
  28-tool catalogue.
- Credential/session boundary: daemon API-key bytes come only from a bounded,
  owner-only absolute file and must be visible ASCII without whitespace. The
  process creates a principal-bound session, rejects initial or renewed
  catalogue drift, derives idempotency from the tool descriptor, refreshes an
  expired session, and closes it on ordinary stdin shutdown.
- Process evidence: a black-box test starts one secured project-bound server,
  launches MCP with URL/instance/principal/key-file only, proves exact tool
  name parity and authenticated service-status execution, then verifies the
  MCP principal in durable audit after clean close. Existing embedded stdio and
  full-foundation process suites still pass.
- Architecture/doctor evidence: MCP's only production workspace dependencies
  are `rrd-engine`, `rrd-contract`, and `rrd-client`; physical bypass remains
  absent. Doctor report v3 now reports `surface.mcp-daemon-mode` passing and
  moves from 13/6 to 14/5 pass/block counts. Strict MCP Clippy and diff checks
  pass.
- Remaining: D5 Connectome/CLI attachment, exact-argv proxy, supervisor,
  every-domain daemon differentials, response-loss replay, remote mTLS, and
  full-topology CI. No full workspace or remote matrix was run.

## 2026-08-25 — D5.1a coherent diagnostic projection

- Public contract: `POST /v1/diagnostics/read` is operation 31 and has its own
  `DiagnosticsRead` security action. Its strict bounded request accepts only a
  project scope plus changefeed/audit cursors and limits; it accepts no path or
  physical-storage selector.
- Cross-authority stamp: the response records claim sequence,
  control-journal sequence, runtime cursor, schema revision, catalogue
  revision, and authenticated runtime-manifest digest. The storage `Engine`
  trait now exposes the authoritative control-journal head with matching
  Memory, native, Fjall-compatibility, persistent-delegation, and reopen
  behavior.
- Engine assembly: RRD reads product/runtime-tool catalogues, query-index and
  vector-collection catalogues, estate state, bounded changefeed, and bounded
  audit, then compares the complete monotonic stamp before and after. It
  retries up to three times and returns a retryable storage conflict instead
  of presenting mixed-time data as one snapshot.
- Transport/client: `rrd-server` authenticates the exact operation and
  `rrd-client::read_diagnostic_snapshot` validates the returned contract. The
  project-bound real-server test exercises the path and verifies instance,
  runtime/control coordinates, manifest identity, section catalogue, and
  unchanged 28-tool registry. A separate engine test proves that a session
  without `DiagnosticsRead` cannot call it.
- Generated surfaces: TypeScript OpenAPI types and TypeScript/Python/Go/Java/
  .NET endpoint maps contain operation 31. TypeScript, Python, and Go full
  local gates pass; Java and .NET generator drift passes, but this host lacks
  Java/Maven and .NET for their build/test gates.
- Rust evidence: the focused control-journal differential/reopen test, all
  contract/engine/server/client targets, real child-process estate/server
  tests, architecture tests, and strict all-target Clippy pass locally.
- Remaining D5.1: schema/model/table views, graph-at-time/diff,
  reasoning/routing/trace summaries, retention, vector artifact catalogue,
  and cluster diagnostics still live behind Connectome physical reads. This
  is not D5.1 completion and no full-workspace or remote-matrix claim is made.

## 2026-08-25 — D5.1b schema/model and bounded temporal graph diagnostics

- Schema/model lens: the diagnostic response now carries the authoritative
  public schema registry plus stable record/relation/event summaries with
  property, required-property, and constraint counts bound to the captured
  schema revision. Bounded table rows remain the query plane's responsibility;
  Connectome is not given a second ad hoc table authority.
- Temporal graph lens: the request names valid time, optional known cursor,
  comparison cursor, and a replay ceiling. RRD reconstructs records and native
  typed edges from the exact authenticated read stamp and returns a lossless
  added/removed/changed differential. It rejects future/reversed coordinates
  instead of preserving Connectome's old silent head clamp.
- Boundedness: graph replay is capped by an explicit caller budget no greater
  than one million source changes. This is correct for the current log-backed
  implementation but not the final large-history design; checkpointed graph
  projections remain a recorded follow-up rather than an unbounded scan.
- Contract evidence: graph identities, validity intervals, ordering,
  difference category separation, and read-stamp coordinates are validated.
  The generated OpenAPI digest is
  `e8503d7be13079b4b81b689bd97af576571e009eeef805dee89a8213c0f8d335`.
- Runtime evidence: all targets for `rrd-contract`, `rrd-engine`, `rrd-server`,
  and `rrd-client` pass, including the project-bound real-server test and the
  real child-process estate/server tests. Strict all-target Clippy with
  `-D warnings` passes for `rrd-store` and those four public/engine packages.
- SDK evidence: TypeScript generation/Biome/typecheck/tests, Python generation/
  Ruff/mypy/pytest, and Go generation/vet/race tests pass. Java and .NET
  generation drift passes; their build/test gates remain host-blocked by the
  previously recorded missing JDK/Maven and .NET SDK.
- Remaining D5.1: reasoning/routing/trace, retention, vector-artifact, and
  cluster diagnostic lenses. Connectome still opens physical crates and this
  work is not a cohesive product, workspace, or remote-platform checkpoint.

## 2026-08-25 — D5.1c verified snapshot-retention diagnostics

- Public lens: the diagnostic snapshot now includes each live persisted
  snapshot lease and its exactly derived semantic retention pin, including
  scope, owner, lifetime, runtime/schema/catalogue coordinates, manifest
  identity, and the oldest cursor that must remain serviceable.
- Consistency correction: snapshot leases do not advance the runtime cursor or
  control journal. RRD therefore hashes the canonical complete live set into a
  separate `retention_sha256` read-stamp coordinate, reads it before/during/
  after assembly, and retries if those identities differ.
- Contract checks require sorted unique SHA-256 identities, one pin per live
  lease, exact lease/pin scope/manifest/cursor/expiry agreement, active
  lifetimes at the fixed observation time, and digest agreement with the read
  stamp.
- Reopen evidence: the real project-bound server fixture persists a snapshot
  lease before closing the physical setup handle, reopens through RRD, and
  verifies the lease, pin, owner, and retained cursor through `rrd-client`.
- Verification: all affected contract/engine/server/client targets, including
  the real child-process estate/server suite, and strict all-target Clippy pass.
  TypeScript, Python, and Go complete local gates pass; Java and .NET generation
  drift passes. The current OpenAPI digest is
  `1d12d4ca0f5429b8cfe6d4dad78e03141ccd91e144f765d6a6be26b4971ebe22`.
- Remaining D5.1: reasoning/routing/trace, vector-artifact, and cluster lenses.
  Connectome physical access and the cohesive workspace/remote gates remain
  open.

## 2026-08-25 — Q1–Q3 typed Arrow, DataFusion 55, and BM25 execution

- Dependency baseline: `rrd-query` pins the current Apache DataFusion release,
  `55.0.0`; its resolved Arrow family is `59.2.0`. The dependency update and
  affected compile were completed before executor work.
- Arrow boundary: every RRFlowQL source is materialized as an immutable typed
  `RecordBatch` with a hidden stable RRD identity. Boolean, signed, unsigned,
  text, decimal, digest, null, mixed, list, and map values have reversible RRD
  semantics rather than a lossy JSON table representation.
- DataFusion boundary: non-lexical predicates, deterministic identity order,
  projection, and limit now execute through a registered DataFusion `MemTable`
  and its logical/physical planners. Physical plan evidence names
  `data_fusion_evaluate`; the former bespoke row evaluator has been removed.
  RRFlow null-inequality behavior is explicitly lowered instead of inheriting
  SQL three-valued behavior accidentally.
- BM25 boundary: the shared query catalogue supports deterministic,
  content-addressed BM25 artifacts and `MATCH`. The scorer uses the Qdrant
  reference equation, survives engine reopen, invalidates only on relevant
  source changes, and retains exact authoritative fallback behavior.
- Verification: all `rrd-query` unit, golden, parser, query, index catalogue,
  and live-query tests pass; the `rrd-engine` multi-model transaction/reopen
  test passes; strict all-target Clippy for `rrd-query` and `rrd-engine` passes
  with `-D warnings`.
- Remaining foundation gate: the public engine vector path still constructs an
  exact-only runtime. Durable HNSW/TurboQuant catalogue code exists below the
  composition root, but RRD does not yet own its object store, build contract,
  approximate-search policy, and reopen selection as one operation. Hybrid
  retrieval and the required ten-or-more engine-only persistent scenario
  matrix therefore remain open. No full-workspace, remote-matrix, or competitor
  superiority claim is made.

## 2026-08-25 — Linux workspace gate and edge-profile correction

- Exact workspace evidence: with the CI-pinned pgvector container and
  disposable database contract enabled, `cargo test --workspace
  --all-features --locked` passes. This includes the real OpenRaft process and
  mutual-TLS suites, native/Fjall differential and migration suites,
  DataFusion/BM25 execution, pgvector live integration, RRD server process
  tests, CLI lifecycle tests, MCP embedded/daemon tests, and the long estate
  child-process kill/restart matrix.
- Warning gate: `cargo clippy --workspace --all-targets --all-features
  --locked -- -D warnings` passes. Formatting is clean under `cargo fmt --all
  --check`. The architecture ownership test remains 8/8.
- Feature-unification defects fixed: public OpenAPI and checked-in core/operator
  golden JSON are now canonical across standalone and unified DataFusion
  builds. The current generated OpenAPI digest is
  `83f71f1fb825f8c9a8bf6cc252a23a9ac60f4cc5893c689cf9d627ae2ee8e433`.
- Index freshness correction: BM25/scalar index artifacts use the query
  source's watermark, but the engine had still compared that coordinate to the
  global transaction cursor. RRD now compares the same source-specific
  coordinate at build and reuse. A second independent idempotency key proves
  unrelated model mutations do not rebuild generation 1; the HTTP fixture's
  document index correctly names source cursor 4 inside an 11-mutation unified
  transaction.
- Lifecycle/surface corrections: CLI process tests now exercise explicit
  preflight, source-change re-attunement, exact pre/post-tool digest closure,
  and the typed decision-before-verification transition. Generated MCP tests
  derive counts from the authoritative catalogue instead of freezing 30 after
  operation 31 landed. Runtime-written harness instructions and control-plane
  command recognition use RRFlow/RRD terminology in the touched surface.
- Distributed-test correction: the mutual-TLS snapshot test now follows the
  leader legitimately elected at the highest Raft term instead of attempting
  to force node 1 and timing out while node 3 was healthy and authoritative.
- Edge regression found and fixed: composing the full engine into
  `rrflow-edge` raised the release binary to 7,990,736 bytes, violating the
  2,097,152-byte gate. `rrd-engine` now exposes one minimal `edge` feature and
  one default `full` feature; the edge binary still consumes the engine API but
  excludes DataFusion, server, cluster, and network stacks. Its verified
  release size is 1,733,720 bytes. Edge offline build/query/reopen tests and
  strict Clippy pass.
- Remaining local CI-equivalent evidence also passes: controlled evaluation
  evidence is 8/8 with zero recorded regressions; the one-dependency
  `rrd-core` kernel boundary passes; edge and local-inference network dependency
  denials pass; and the exact default-feature estate/server build, tests,
  process recovery, and strict Clippy sequence used by the three-OS job passes
  on Linux.
- This is a local Linux checkpoint only. macOS/ARM and Windows remain
  unverified until a reviewed change set is pushed and every required remote
  job completes. The RRD-owned HNSW/TurboQuant public lifecycle, unified hybrid
  retrieval, and ten-or-more independent persistent end-to-end scenario gate
  remain open; no database-superiority claim is authorized by this checkpoint.

## 2026-08-25 — RRD-owned persistent HNSW lifecycle

- Composition correction: `RrdEngine` now owns both the one selected persistent
  engine and its local immutable object tier. A borrowed `DataRuntimeRef` uses
  the same stage, content verification, transaction, and idempotent commit
  coordinator as the owning data runtime; RRD does not reopen storage or create
  a second vector transaction authority.
- Public search contract: vector searches explicitly select `exact`,
  `allow_approximate`, or `require_approximate`. Approximate requests bind
  `top_k <= exact_rerank <= ef_search` under the existing bounded-search limit.
  Exact remains the serde/default behavior for existing clients. This reviewed
  wire change advances the generated OpenAPI digest to
  `9d3490d2b24d3b96ff0d941a1a2a1400221299be662862770b81bdcebcae37f9`.
- Build contract: the engine accepts one bounded HNSW configuration for a
  declared dense named vector, reads canonical vectors at one authenticated
  stamp, builds deterministic immutable graph bytes, and atomically commits the
  typed artifact-catalogue record plus verified object reference. A
  content-equivalent ensure returns the existing generation; a changed source
  builds exactly the next generation.
- Freshness correction: planning compares an artifact to the newest relevant
  vector mutation cursor, not the global runtime cursor. Catalogue publication,
  trace, or unrelated model mutations therefore do not make the graph stale;
  a later mutation of the addressed vector family does.
- Search composition: ordinary `RrdEngine::search_vectors` reconstructs its
  serving view from canonical vectors plus the authoritative object-backed
  catalogue. Required approximate search fails closed when no fresh artifact
  qualifies. Allow-approximate may select the exact oracle as an explicit
  fallback; every HNSW candidate set is exactly reranked.
- Persistence evidence: the engine test commits three named vectors, publishes
  generation 1, proves content-idempotent ensure, selects HNSW, closes and
  reopens RRD, reproduces the plan, commits a fourth vector, proves stale HNSW
  denial and exact fallback, publishes generation 2, and selects HNSW again.
  Existing missing/corrupt-object and atomic-record/object tests remain green.
- Verification: all targets for `rrd-contract` and `rrd-engine` pass. Strict
  all-target Clippy with `-D warnings` passes for `rrd-store`, `rrd-contract`,
  and `rrd-engine`.
- Remaining vector foundation: TurboQuant must enter this same build/reopen/
  freshness path, followed by one hybrid BM25+dense engine contract. The new
  index-build engine contract is intentionally not yet claimed as an HTTP,
  MCP, SDK, CLI, or Connectome surface; those are regenerated after the
  engine-level persistent scenario gate. Logical backup currently guarantees
  referenced metadata, not inclusion of immutable vector payload bytes, so
  vector-bearing backup/restore remains an explicit scenario/gap rather than a
  completed claim.

## 2026-08-25 — RRD-owned persistent TurboQuant lifecycle

- Shared lifecycle: TurboQuant is a second physical artifact kind behind the
  same authenticated `EnsureVectorIndex` engine contract, source-specific
  freshness rule, immutable object tier, typed catalogue, CAS revision, reopen
  path, and vector planner used by HNSW. It does not own a parallel collection,
  transaction, or serving catalogue.
- Typed configuration: the public engine contract selects 4-, 2-, 1.5-, or
  1-bit TurboQuant encoding, deterministic seed, and declared one-stage filter
  properties. The existing codec performs randomized orthogonal Hadamard
  rotation, MSE centroids, compact packing, asymmetric query scoring, and
  exact final reranking against canonical vectors.
- Coexistence and selection: HNSW and TurboQuant may both be published for one
  named vector under distinct stable projection identities. The shared planner
  compares only fresh, identity-compatible, filter-complete candidates;
  `require_approximate` removes exact fallback and selected TurboQuant in the
  controlled fixture. Search still reports the actual chosen path.
- Persistence/compression evidence: the engine fixture publishes a 2-bit
  TurboQuant generation over four canonical dense vectors, verifies its packed
  vector payload is smaller than the 32-bit source-vector payload, proves
  content-idempotent ensure, exact top-hit reranking, closes and reopens RRD,
  and reproduces the same TurboQuant plan digest.
- Verification: all targets for `rrd-contract` and `rrd-engine` pass after the
  HNSW/TurboQuant composition. Strict all-target Clippy with `-D warnings`
  passes for `rrd-store`, `rrd-contract`, and `rrd-engine`.
- Remaining retrieval foundation: BM25 and dense/approximate retrieval are
  individually real but do not yet execute through one typed hybrid request,
  fusion policy, read stamp, and result explanation. Compression effectiveness
  at fixture scale is not a latency, recall, or database-superiority claim.

## 2026-08-25 — Shared-stamp BM25 and vector hybrid retrieval

- One engine operation: `RrdEngine::search_hybrid` requires both query and
  vector-search authorization, captures one immutable RRD read stamp, binds
  BM25 to its explicit known cursor, and executes vector retrieval against the
  same stamp. It rejects any branch whose manifest or known cursor differs.
- Typed contract: hybrid requests name the canonical document kind/text field,
  BM25 query, named vector, dense/sparse/multivector query, optional one-stage
  vector filter, exact/allow/require vector mode, bounded candidate/result
  counts, and weighted reciprocal-rank parameters. Validation requires both
  retrieval branches to carry non-zero bounded weights and requires
  `top_k <= candidate_k`.
- Deterministic fusion: text rows and vector hits join by the canonical vector
  subject identity. Fusion uses one-based ranks and a versioned request-bound
  weighted RRF formula; fused ties resolve by canonical subject identity. The
  response preserves text/vector raw scores and ranks, both physical access
  paths, both plan digests, the shared read coordinates, and a SHA-256 fusion
  plan identity.
- Physical paths remain honest: BM25 may use its fresh materialized index or
  exact authoritative fallback; vectors may use exact scan, HNSW, or
  TurboQuant under the existing freshness/filter rules. Hybrid does not
  concatenate independent server responses or establish another catalogue.
- Persistence evidence: the fixture builds a persisted BM25 document index and
  persisted 2-bit TurboQuant vector artifact, selects both physical indexes at
  one read stamp, joins their canonical document identities, ranks the shared
  top hit first in both branches, closes and reopens RRD, and reproduces the
  complete hybrid result including all three plan digests.
- Verification: all targets for `rrd-contract` and `rrd-engine` pass; strict
  all-target Clippy with `-D warnings` passes for `rrd-store`, `rrd-contract`,
  and `rrd-engine`.
- Remaining gate: hybrid/index administration is an engine contract, not yet a
  generated HTTP/MCP/SDK/CLI/Connectome surface. Before surface expansion, the
  next gate is ten or more independent persistent engine scenarios, including
  object-bearing backup/restore behavior, with 100% success.

## 2026-08-25 — Application-complete RRD backup activation boundary

- Audit finding: logical backup catalogues declared immutable payloads
  `referenced_only`, while restore replayed object-reference mutations without
  retaining their bytes. A second independent defect stored vector-collection
  and query-index catalogue records outside the logical archive. A restored
  vector runtime therefore had canonical vectors and artifact references but
  could not resolve its named collection and could not open the artifact after
  source loss.
- Stable-cut contract: the application backup now captures claim, runtime, and
  control watermarks; exports one logical cut; collects its exact unique object
  reference closure; captures final query/vector catalogue records and each
  scope's authoritative catalogue revision; and rejects publication if any
  source watermark changes during capture.
- Authenticated artifacts: backup catalogue format 2 binds the logical archive
  digest, an authenticated content-addressed object manifest, retained verified
  object payloads, and an authenticated content-addressed catalogue manifest.
  Public coverage now reports `catalogues=included`,
  `object_payloads=included`, and `application_complete=true` for the RRD
  service path. The lower-level logical-only API remains explicitly incomplete
  with excluded catalogues and referenced-only objects.
- Atomic activation: logical replay, catalogue materialization/revision
  reconstruction, immutable payload streaming, engine reopen, catalogue
  verification, and payload verification occur inside one hidden staging root.
  The root is published only after every check succeeds. Existing-restore
  idempotency also re-verifies catalogue state, revisions, and payloads.
- Positive executable evidence:
  `application_backup_restores_turboquant_payload_before_instance_activation`
  publishes TurboQuant through `RrdEngine`, creates and restores an
  application-complete backup, removes the source root, reopens the restored
  RRD root, and requires the planner to select `turboquant`. The correctly
  targeted run reports 1 passed, 0 failed.
- Negative executable evidence:
  `application_backup_payload_corruption_fails_before_restore_publication`
  alters one retained payload byte, proves authenticated catalogue verification
  fails, proves restore fails, and proves the target root remains absent. The
  independent run reports 1 passed, 0 failed.
- Wire review: adding truthful public catalogue coverage intentionally advances
  the generated OpenAPI digest to
  `35b62d736846ff1fc2c353c54de42b6455e9a4ec2d5a9e793eec724418c9cfdc`.
- Scenario gate: `docs/rrd-persistent-scenario-matrix.md` freezes 15 independent
  persistence/recovery scenarios and the required post-scenario workspace and
  platform order.
- Scenario execution: all 15 tests were then invoked as separate Cargo test
  processes with exact filters. Every process reported 1 passed and 0 failed:
  native identity/reopen; reasoning transaction replay; unified multi-model
  evidence reopen; bitemporal correction history; authenticated historical
  proofs; persistent snapshot leases; live-query deltas; persisted query-index
  planner selection; HNSW/TurboQuant/BM25 hybrid persistence and staleness;
  source-independent TurboQuant backup restore; payload-corruption denial;
  logical-archive corruption/retry; cross-format logical recovery; resumable
  migration fault boundaries; and persistent deny-by-default security.
- Gate result: persistent scenario matrix 15/15 (100%) on the current Linux
  workspace.
- Full-workspace environment incident: the first locked, all-feature workspace
  run stopped during linking because the shared Cargo target volume had only
  2.4 GB free. No test failure was produced. A read-only disk audit identified
  67 GB of disposable Cargo incremental state under the exact workspace cache
  target. Only that incremental cache was removed; it is recoverable by a
  rebuild. Available space increased to 68 GB.
- Full-workspace executable evidence: with incremental compilation disabled and
  the disposable pgvector test endpoint configured, `cargo test --workspace
  --all-features --locked` completed successfully on Linux, including workspace
  unit, integration, process, architecture, recovery, HTTP, MCP, edge,
  Connectome, pgvector, and documentation tests.
- Strict Clippy evidence: after the operator-requested pause ended, the exact
  gate was rerun with incremental compilation disabled. `cargo clippy
  --workspace --all-targets --all-features --locked -- -D warnings` completed
  successfully on Linux with exit status 0.
- Pause boundary: no specialized workflow gates, remote Linux/macOS/ARM/Windows
  matrix, final diff audit, public-surface generation/reconciliation, commit, or
  push was performed. The unified RRFlow foundation is not declared complete;
  those gates and the wider capability/cohesion audit remain unfinished.

## 2026-08-25 — RRFlow `0.1.0` alpha identity cutover

- Decision: RRFlow is the Reason Ready Flow product. RRD is the Reason Ready
  Daemon and the product's single engine/runtime authority. The retired
  pre-release identity is not preserved as a package, alias, forwarding crate,
  protocol, environment fallback, persisted-format reader, or documentation
  exception. Because this is pre-release, V1 was corrected in place; no
  artificial V2 or compatibility shim was introduced.
- Workspace cutover: the dependency graph now resolves through canonical
  `rrd-*` engine packages and `rrflow-*` product adapters. RRFlowQL is the public
  query language. The separately branded query-executor, data-service, and KV
  identities were absorbed as responsibilities inside the RRD engine/package
  graph rather than retained as products.
- Persisted contract: WAL, record, batch, segment, index, snapshot, migration,
  native keyspace/sequence, vector, HNSW, dense-map, and TurboQuant V1 markers
  now use the frozen RRD identities recorded in
  `docs/rrflow-rename-ledger.md`. Digest/media domains and checked-in fixtures
  were regenerated against the canonical bytes.
- Fixture handling: stale expected digests surfaced independently in core
  unified-data, operator-knowledge, RRFlowQL planning, store migration, vector
  search/HNSW, and RRD LSM persistence tests. Each fixture family received a
  narrow, explicit golden-update path where one did not already exist; the
  canonical fixtures were regenerated and then verified again without update
  mode.
- Architecture enforcement: `rrd-engine/tests/workspace_architecture.rs`
  enumerates active tracked and untracked files through Git, ignores only
  non-repository/generated locations, filters deleted index entries, scans
  names and bytes case-insensitively, and rejects the retired identity plus
  obsolete composition-root names. There is no source allowlist. The same
  suite continues to expose known CLI and Connectome lower-layer dependency
  bypasses as explicit cohesion debt.
- Environment incident: the shared Cargo target volume reached capacity during
  the DataFusion build. A package-aware `cargo clean --workspace --target-dir`
  removed 38,792 rebuildable files belonging to this workspace (264.9 GiB),
  leaving other project/dependency caches in place. The workspace was rebuilt
  and all gates below completed after the cleanup.
- Successful local gates on the same current tree:
  - `cargo metadata --locked`
  - `cargo fmt --all -- --check`
  - `cargo check --workspace --all-targets --locked`
  - `cargo test -p rrd-engine --test workspace_architecture --locked` (8/8)
  - `cargo test -p rrd-lsm --locked`
  - targeted core, operator-knowledge, query, store, and vector golden suites
  - `cargo test --workspace --all-targets --locked --no-fail-fast`
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - repository identity/path scan and `git diff --check`
- Scope truth: this completes the local identity cutover, not the whole RRFlow
  product foundation. Connectome and the CLI still have recorded direct
  lower-crate dependencies; capability-surface disposition is not yet fully
  generated across engine, MCP, CLI, SDKs, and Connectome; the DataFusion plane
  still lacks a custom streaming provider, pushdown, cancellation, memory/spill
  qualification; sparse/late-interaction retrieval remains incomplete; and
  the Fjall compatibility backend remains separate backend-removal debt.
- Publication truth: this cutover is uncommitted and unpushed in the current
  worktree. Remote Linux, macOS/ARM, and Windows CI has not run and is not
  claimed.

## 2026-08-26 — lifecycle authority, capability truth, and exact-argv boundary

- Lifecycle authority: the provider-neutral lifecycle contract is now a strict
  hash-chained RRD runtime aggregate with derived session state. Invalid
  transitions, identity substitution, forged semantic events, compaction
  recovery errors, and stale projection closure fail before advancing the
  global cursor. Memory, native reopen, and Fjall compatibility replay produce
  identical state in the focused differential suite.
- Command boundary: `rrflow exec -- <exact argv>` now crosses the same durable
  reasoning, attunement, work-plan, workflow, trace, and observation gates as
  other governed mutations. It resolves and hashes the executable, binds argv
  boundaries, project-contained cwd, inherited-environment identity,
  repository/worktree identity, timeout, and output bounds, then atomically
  consumes the authorization before spawn. Captured streams are fully hashed
  while only bounded prefixes are retained. Exit, signal, timeout, repository
  state, and observation evidence close even for unsuccessful child commands.
- Work-list correction: all requested database capability families now have
  explicit acceptance criteria in `rrflow.workplan.toml`. The interim
  source-and-test matrix in `docs/rrd-engine-capability-coverage.md` labels
  incomplete or absent features as partial/missing instead of treating roadmap
  text as implementation evidence.
- Comparison evidence: fresh standard and embedding metadata-fanout fixtures
  both pass their pinned native-vs-Fjall promotion policies. These are bounded
  workload wins, not a universal claim; the earlier losing native write-p95
  read-heavy cell remains disclosed.
- Local gates: RRD engine tests, RRFlow CLI all-target tests, locked metadata,
  formatting, and targeted Clippy with `-D warnings` pass. `rrflow dev doctor`
  improves from 13/6 to 15/4. Remaining blockers are Connectome's physical
  crate bypass, the CLI's residual core/store bypass, the missing supervisor,
  and the dependency-ordered full-topology CI smoke job.

### G00-W02 canonical lifecycle acceptance checkpoint

- Contract boundary: the reviewed V1 vocabulary now has 34 canonical event
  types, strict event-specific payloads, stable session/turn/reasoning/attempt/
  tool coordinates, bounded envelopes, SHA-256 payload/event seals, trace and
  read-stamp coordinates, and a checked-in golden command fixture. Unknown
  fields, unknown event names, mismatched payloads, oversized values, invalid
  coordinates, and digest tampering fail closed.
- One-engine persistence: lifecycle schema, sealed event, and materialized
  session state commit atomically through RRD's existing `commit_runtime`
  authority. There is no lifecycle database, crate, catalogue, transaction
  coordinator, or adapter-owned state. Replay verifies runtime-change digests,
  subject/scope binding, event order, causation, previous-event digest, and
  previous-state digest before deriving the aggregate.
- State-machine enforcement: reasoning-run and tool-attempt substitution is
  denied; tool results that change project state require invalidation and a
  fresh projection; a successful outcome must agree with passing verification;
  and compaction binds the exact pre-compaction state and requires successful
  re-attunement before close or another turn.
- Persistent evidence: six lifecycle contract tests and sixteen lifecycle
  engine tests pass. The engine matrix includes canonical session-to-outcome,
  forged-event denial, stale-compaction denial, native close/reopen, native
  reopen during compaction, reference/native equality, and compatibility-
  backend reopen. The complete all-feature `rrd-contract` plus `rrd-engine`
  all-target suite passed locally, as did strict all-target Clippy with
  `-D warnings`, formatting, and the lifecycle-scoped diff check.
- W03 authority progress: `planning.authorized`, `plan.recorded`, and every
  mutating `tool.proposed` event now re-read RRD's authoritative work-plan
  aggregate before commit. Plan ID, revision, active item, plan payload,
  source-tree digest, and verification-command digest must match. The lifecycle
  aggregate retains that binding, refuses expired permits, consumes one exact
  attempt/request at `tool.started`, and refuses substituted results or a
  second start. These primitives are proven, but W03 is not crossed until the
  provider-neutral supervisor replaces direct provider-shaped authorization
  calls on every mutation surface.
- Status boundary: this satisfies G00-W02's executable acceptance criteria on
  the current Linux tree. It does not close G00. G00-W03 remains open because
  the existing provider-shaped hook/work-plan path does not yet emit and bind
  every planning or mutation decision through this canonical lifecycle. The
  remote Linux, macOS/ARM, and Windows matrix is also unclaimed.

## 2026-08-27 — Connectome crosses the public RRD boundary

- Authority cutover: the in-repository Connectome runtime no longer opens an
  RRD database or composes core, engine, store, query, vector, estate, or
  cluster crates. It authenticates through `rrd-client`, retains a bounded
  renewable session, and renders the validated public diagnostic snapshot.
- Public behavior: `/api/snapshot` returns the authoritative stamped snapshot;
  `/api/runtime/capabilities` returns RRD service and runtime-tool catalogues;
  scope-bound RRFlowQL and catalogue-resolved runtime-tool invocations use the
  supported client. API-key files must be private on Unix and secrets are not
  logged or returned.
- Honest limitation: embedded lab-only flight writes, cluster sample writes,
  trace summaries, and source-routing projections without a public RRD
  contract return explicit `501` responses. They remain D5.1/D5.3 work rather
  than surviving as a second data authority.
- Evidence: Connectome unit and all-target tests, JavaScript syntax validation,
  and strict all-target Clippy pass. The real-socket fixture boots RRD,
  authenticates, validates a seeded model/graph snapshot, executes RRFlowQL,
  and invokes `rrflow_service_status`. `rrflow dev doctor` advances from 15/4
  to 16/3 with `surface.connectome-client-only` passing.
- Capability audit: the requested twenty engine capabilities are frozen as
  `CAP-01` through `CAP-20`, each mapped to explicit work-plan closure items.
  Truth remains five core-present, fourteen partial, and one missing.
