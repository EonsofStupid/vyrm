# RRFlow implementation journal

## 2026-08-29 — physical vector memory tiers

- Commit: `pending`. G04-W05 is active at the authoritative 30/65 board state;
  this entry does not claim board verification before the exact gate completes.
- One process-local `VectorResidencyManager` is owned by `RrdEngine`; collection
  memory-tier declarations no longer stop at catalogue metadata.
- Pinned artifacts are retained under hard admission and never silently
  evicted. Cached artifacts use deterministic byte-bounded LRU eviction and
  oversized-object transient bypass. Cold artifacts retain zero manager bytes,
  using verified read-only mmap for compact/quantized/TurboQuant codecs and a
  verified transient-owned fallback for JSON HNSW/exact or non-local stores.
- Search reconstructs active planner descriptors before reading bodies and
  loads only request-compatible artifacts. Retired generation bindings are
  removed before serving; reconciliation reclaims stale process entries.
- Corruption and substitution fail closed. Resource pressure suppresses a path
  only in the current serving view: allow-approximate can fall back to the
  canonical exact scan, while require-approximate reports ResourceExhausted.
- Engine and manager tests cover cached hits/LRU, physical tier transitions,
  cold mmap/owned behavior, bounded admission, stale reclamation, generation
  replacement, restart equality, pressure fallback, and required-ANN denial.
- Boundaries: policy is per named vector, not independently configurable for
  original/HNSW/quantized/sparse/payload structures. Automatic warming,
  distributed residency, compact mmap HNSW, generated outward controls, and
  production fixed-hardware sizing remain open.

## 2026-08-29 — unified quantization artifact lifecycle

- Commit: `3deb168`. G04-W04 is verified with digest
  `59c35e6d4139a21af11c5b458216efe7a5824351b7b17524cd1f4e5c99a74f6d`;
  no full-product-green claim is made here.
- One engine path now builds, lists, activates, retires, reopens, plans, and
  exactly reranks immutable scalar, product 4×/8×/16×/32×/64×, binary, and
  TurboQuant 4/2/1.5/1-bit artifacts. Strict public contract types are present;
  generated HTTP/MCP/CLI/SDK bindings remain G06 work.
- The historical TurboQuant `ensure_vector_index` shape now delegates to that
  lifecycle with ready-generation recovery and active-generation replay. New
  generic quantized publication is denied, old generic TurboQuant views are
  suppressed at reopen, and lifecycle restore no longer advances the unrelated
  generic vector-catalogue revision.
- Lifecycle authority is append-only and digest-chained. Build atomically binds
  its canonical record, object receipt, and build event. Ready generations do
  not serve; activation verifies bytes and selects one generation; retirement
  removes planner visibility. Restart reconstructs active generations only.
- Concurrency correction: catalogue replay is pinned to an authenticated read
  stamp and the exact same stamp supplies the commit CAS. A concurrent lifecycle
  change therefore conflicts instead of allowing a stale semantic revision to
  commit. Duplicate/missing builds, revision gaps, backward activation, and
  non-atomic record/object/event combinations fail recovery.
- Physical formats are bounded and checksummed. Scalar/product/binary share an
  aligned binary format; TurboQuant now also has true read-only mmap rather than
  copying on reopen. Runtime-dispatched AVX2 and portable scalar paths retain
  identical rankings with bounded score tolerance.
- The retained 512×64 matrix covers 11 method/ratio rows, owned+mmap reopen,
  corruption, packed/full/auxiliary/total bytes, mean absolute score error,
  Recall@10 after exact reranking 96, and build/search timing. Observed recall is
  0.90–1.00 and product packed codes reach 64×. Product codebook overhead makes
  the small-corpus total artifacts larger than raw f32, so 64× is explicitly
  not presented as universal resident-memory compression.
- Engine tests cover authentication denial, ready isolation, activation,
  exact-reranked search, process reopen, retirement, update fallback, next-
  generation rebuild, and reactivation. Durable projection traces link the
  reasoning run and committed object/lifecycle evidence.
- Remaining boundaries after G04-W05: larger release/fixed-hardware p50/p95/p99
  evidence, independent per-structure tiers and automatic warming, distributed
  placement, GPU, residual QJL, and generated outward bindings remain later
  governed items.

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

## 2026-08-27 — CLI crosses the embedded engine boundary

- Intermediate composition checkpoint: `rrflow-cli` removed its production
  `rrd-core` and `rrd-store` dependencies, but still entered through the
  temporary engine-owned `EmbeddedOperator`. G01-W02 later removed that second
  handle and moved the same bounded operations directly onto `RrdEngine`; this
  entry is retained as sequencing evidence, not current architecture.
- Preserved behavior: recall and provenance, claim CRUD/history, invocation and
  effectiveness ledgers, projection rebuild/ground/reset, hooks, preflight,
  RRFlowQL, reasoning, routing, exact-argv execution, work-plan enforcement,
  migrations, archives, backups, and format upgrades retain their prior command
  behavior and persistent fixtures.
- Evidence: 31 CLI tests pass (4 binary unit, 2 fixture, 16 operator-surface,
  and 9 runtime-experience), all 8 workspace architecture tests pass, and
  strict all-target Clippy passes for `rrd-engine` and `rrflow-cli`.
  `rrflow dev doctor` advances from 16/3 to 17/2; only supervised lifecycle and
  its full-topology CI smoke remain blocked.
- Honest limit: this closes the physical dependency boundary. Authenticated
  daemon-mode CLI selection and embedded/daemon behavior parity remain open
  D5.4/G06-W04 acceptance work.

## 2026-08-27 — Recoverable authenticated development topology

- Supervisor: `rrflow dev up|status|logs|stop` now owns companion builds,
  dedicated-instance verification, one-time `rrd-security-bootstrap`, ordered
  RRD-before-Connectome startup, HTTP readiness, bounded retained logs, atomic
  non-secret state, and paired graceful shutdown acknowledgements.
- Security: an engine-owned helper creates/reopens a private printable 256-bit
  API credential. Connectome's base grants and every engine-catalogued runtime
  tool action generate the bootstrap policy. Bootstrap is idempotent only for
  the same principal, grants, and credential; a different existing authority
  remains a hard error rather than being overwritten or bypassed.
- Evidence: a real process run proved `/v1/health/ready`, authenticated
  `/api/snapshot`, status/log recovery, and graceful completion for both
  services. CI now repeats that black-box topology. Connectome and CLI tests
  pass, and `rrflow dev doctor` reports 20 passed, zero blocked, zero warnings.
- Limit: green development topology closes D5.5/D5.6 only. D5.1/D5.3 feature
  parity, D5.4 authenticated daemon-mode CLI parity, and the CAP-01–CAP-20
  engine gaps remain open exactly as recorded in their capability/workplan
  ledgers.

## 2026-08-27 — G00-W03 canonical mutation-supervisor primitive

- Exact authority: strict adapter-neutral request, authorization, completion,
  and supervisor-context contracts now drive `tool.proposed`,
  `tool.authorized`, `tool.started`, and terminal tool events through the
  canonical lifecycle aggregate. The authorization digest binds the active
  attempt, tool-call identity, request digest, and current lifecycle state.
- Side-effect gate: work-plan revision, active item, recorded plan payload,
  source-tree identity, verification commands, and permit expiry are re-read
  both when authorization is issued and immediately before it is consumed.
  A proposal that was valid before plan drift or expiry therefore cannot start
  a mutation afterward.
- Retry and recovery: one decision can be consumed once across native reopen;
  completed tool-call identities cannot be authorized again; and an exact
  terminal completion retry is idempotent while substituted authorization or
  result coordinates fail without advancing RRD.
- Surface separation: `rrflow_lifecycle` now accepts only strict canonical
  lifecycle envelopes. Provider hook translation is explicit under
  `rrflow_hook`; generated schemas, runtime action mapping, MCP discovery, and
  process fixtures cover both surfaces. A public runtime-tool execution test
  proves canonical persistence across reopen and cross-shaped payload denial.
- Evidence: all-target `rrd-contract`, `rrd-engine`, `rrflow-mcp`, and
  `rrflow-cli` tests pass; repository formatting and strict affected-crate
  Clippy pass; and `rrflow dev doctor` remains 20 passed, zero blocked, zero
  warnings.
- Status boundary: this is reviewed executable G00-W03 progress, not G00-W03
  completion. Existing CLI and runtime mutation paths still have to replace
  their older provider-shaped authorization path with this supervisor before
  the work item can truthfully satisfy “every mutation.”

## 2026-08-26 — G00-W03 supervised AI mutation surfaces

- Provider-neutral authority: checked-in-work-plan mutations now translate to
  one strict lifecycle request and are authorized by the same RRD supervisor.
  Native blocking hooks consume before returning allow, `rrflow exec` consumes
  through the proxied boundary immediately before process spawn, and
  engine-owned MCP/runtime operations consume through the orchestrated boundary
  immediately before execution. Cooperative MCP and observe-only adapters fail
  closed for mutation instead of being mistaken for enforcement.
- Exact binding: authorization binds project, session, active turn, reasoning
  run, attempt, stable tool-call identity, tool name, request digest, work-plan
  revision, active work item, planning payload, source-tree digest, attunement
  receipt, verification command digest, and permit lifetime. The active plan
  and permit are re-read at authorization and consumption.
- Completion and recovery: authorization is one-shot across reopen. Exact
  terminal completion and projection-refresh retries are idempotent; changed
  observations or refresh evidence are rejected. A source-changing post-tool
  event records explicit invalidation and refresh before the lifecycle reaches
  `projection_ready`. Duplicate provider delivery reuses the persisted fresh
  receipt and does not advance the event chain.
- Work-item source lineage: the first attempt to use the authoritative board
  exposed that verification required both a consumed mutation and the original
  pre-mutation tree, making real edits unverifiable. The state model now keeps
  the tree and receipt at each authorization plus the observed result tree.
  Subsequent mutations must continue from that exact result, refresh failure
  leaves verification blocked but recoverable, and final verification binds to
  the last observed tree. A persistent test proves two source-changing
  mutations, stale-chain denial, refresh recovery, and verification against the
  resulting tree. No no-op command is used to manufacture completion evidence.
- Honest adoption/qualification: a recorded item now declares `change` or
  `qualification` execution. Change mode requires the consumed mutation chain.
  Qualification mode rejects mutation and verifies only if the attuned tree is
  byte-identical to the recorded tree. This is the explicit path used to audit
  W01–W03 work that existed before the checkout's RRD board was installed; it
  cannot be used to disguise a tree change or manufacture a completion event.
- Generated runtime-tool policy: the public catalogue no longer exposes the
  ambiguous `attunement: none|exact_tool` field. Every tool now has one strict
  `lifecycle` disposition: `read_only`, `control_transition`, or
  `planned_mutation`. Contract validation rejects contradictory mutation
  metadata. Multi-model commit, query-index creation, vector-collection
  mutation, backup, and restore are planned mutations and receive generated
  lifecycle coordinates. Preflight, routing, reasoning, lifecycle/hook
  translation, and memory maintenance are explicit control transitions needed
  to establish and maintain the governing state rather than hidden bypasses.
- One catalogue: MCP discovery serializes this engine catalogue and engine
  dispatch applies its policy internally. MCP does not keep a second tool list
  or rely on a model to call a separate pre-tool authorization operation.
  The TypeScript OpenAPI surface was regenerated from the reviewed in-place V1
  contract; the canonical document digest is
  `c03843b99d3ee54d923bc44d1af2686037514e7c9b32db8e05af299a4cd2aa3e`.
- Real checkout state: `rrflow init --harness codex-cli` created the tracked
  relocatable dedicated-instance manifest and RRFlow context block. Preflight
  built routing for 372 files and 4,770 symbols, persisted an attunement
  receipt, and installed all 65 work-plan items into the checkout's RRD. The
  local database and verification output are now ignored while
  `.rrflow/instance.toml` remains trackable.
- Passing local gates on this exact source tree:
  - all-target `rrd-contract` tests;
  - all-target, all-feature `rrd-engine` tests, including 17 lifecycle-state,
    13 supervisor, provider-hook source-change/retry, planned engine dispatch,
    persistent reopen, policy, runtime catalogue, data, vector, backup, and
    restore tests;
  - all-target `rrflow-cli` and `rrflow-mcp` tests, including the real stdio MCP
    process and exact-argv command proxy;
  - strict affected-crate Clippy with `-D warnings` and formatting;
  - TypeScript generation drift, Biome, typecheck, and client tests.
- Scope boundary: this closes the implementation and local-evidence portion of
  G00-W03 only after the authoritative work-plan verifier accepts those exact
  commands. It does not claim G00-W04 verification-only completion semantics,
  G00-W05 full generated surface parity, G07 native provider hook coverage, or
  the complete RRFlow database/runtime foundation. The current Codex adapter
  declares MCP cooperation without native blocking hooks, so enforceable Codex
  mutations must use RRFlow's proxied or orchestrated boundaries until G07 is
  qualified.

## 2026-08-26 — G00-W04 exact verification evidence

- Clean-checkout correction: the pushed workspace matrix exposed a declared
  `rrd-cluster-node` Cargo target whose source existed locally but was excluded
  by the repository-wide `**/bin/` rule. The Rust source is now tracked, the
  ignore rule is limited to generated .NET SDK `bin`/`obj` directories, and a
  workspace architecture test requires every Cargo metadata target source to
  be present in Git. This turns the prior local/cloud discrepancy into a
  reproducible local failure.
- Full-workspace correction: the first RRD-owned W04 verification attempt then
  exposed `RrdRaftTimingPolicy` hidden behind the optional transport feature
  while its contract was feature-neutral. The policy now lives in the
  always-available cluster contract and the transport consumes it. Failed
  verification logs are retained under `.rrflow/verification` and their exact
  paths and stderr digest are returned, so a red gate is diagnosable without a
  blind rerun.
- Catalogue parity correction: the next full gate exposed a real-server client
  test with a duplicated hardcoded runtime-tool count. The test now compares
  the validated runtime catalogue endpoint with the diagnostic snapshot's
  catalogue, so additions or removals are governed by the authoritative RRD
  catalogue instead of a second numeric registry.
- Exact plan binding: verification is rejected unless its ordered argv vectors
  are byte-for-byte equal to the commands recorded before implementation.
  Missing, extra, reordered, or substituted checks cannot complete an item.
- Inspectable durable evidence: each verified item now retains its complete
  evidence after RRD reopen: work-item and plan identities, result source-tree
  digest, repository revision bound to that tree, platform, exact argv, pass
  result, exit code, stdout/stderr byte counts and SHA-256 identities, and a
  sealed digest over the entire check. An opaque digest without its evidence is
  invalid state.
- Fail-closed completion: false results, nonzero or absent success exit codes,
  tampered artifact metadata, stale result trees, missing mutation results, and
  qualification tree changes are rejected. Dependent items remain blocked
  until their declared prerequisites carry valid persisted verification.
- Persistent proof: engine tests close and reopen native RRD before inspecting
  exact evidence and exercise substituted-command, tampered-artifact, failed,
  missing-result, stale-tree, one-shot mutation, qualification, and dependency
  denial paths. The real CLI integration executes the recorded argv and reads
  the same exact evidence after reopen.
- Pre-release state rule: the earlier checkout-local board stored only opaque
  verification hashes. It is preserved under
  `.rrflow/dev/pre-w04-exact-evidence-20260826` and is not accepted by the
  stricter V1 reader. The authoritative local board is rebuilt from the
  checked-in 65-item plan with inspectable evidence; there is no dual reader or
  compatibility mode.
- Status boundary: targeted W04 and clean-target architecture tests pass
  locally. W04 remains active until its recorded formatting, workspace test,
  and strict workspace Clippy commands pass on the final tree and the pushed
  Linux, macOS/ARM, and Windows matrix is green.

## 2026-08-26 — W04 publication correction after Windows matrix failure

- Remote evidence, not local inference: commit `4b45294` passed the supervised
  topology and macOS/Ubuntu estate jobs but failed the Windows estate job. The
  real RRD child exited during `RrdEngine::open` with `object store: Access is
  denied. (os error 5)`. The controller's later `Prepared` observation was a
  consequence of that startup failure, not a separate reconciliation defect.
- Root cause: object, migration, and cluster paths independently copied the
  Unix `File::open(directory).sync_all()` operation instead of consuming the
  physical storage durability boundary. Rust's ordinary Windows file open does
  not obtain a directory handle; Windows publication durability was already
  implemented in `rrd-lsm` with write-through `MoveFileExW`, but those callers
  bypassed it.
- Cohesive correction: `rrd-lsm` now publicly owns the platform directory-sync
  and durable-rename primitives. `rrd-store` exposes that one boundary to its
  object and cluster consumers. Object publication, quarantine, Fjall-to-native
  migration, native format upgrade, artifact-transfer state, and OpenRaft spool
  cleanup no longer carry independent directory-sync implementations. Cross-
  directory object moves also sync the removed source entry on Unix.
- Contract proof: portable tests publish both a file and a directory through
  the same primitive. All-target `rrd-store` tests pass, including every
  migration/recovery fault boundary; all-feature `rrd-cluster` tests pass,
  including process-isolated Raft, transfer restart, snapshot, and mTLS; and the
  exact four-test local estate process matrix now passes through child startup,
  controller reopen, identity validation, graceful stop, kill fallback, and
  retained data.
- Status boundary: this corrects the locally reproduced failure, but does not
  make the platform matrix green by assertion. The recorded W04 commands must
  pass again on the final tree, then the exact pushed commit must complete both
  required GitHub workflow runs before W05 begins.

## 2026-08-26 — W04 remote closure and G00-W05 generated work-plan surfaces

- Exact W04 publication evidence: persisted verification digest
  `efee5d2debd7125253b9fbd4664b5f08873424dc91b0b73d5c379e757f0fa9f6`
  is bound to commit `56f6a850c1093b1eae9eacab72c8959cfe39aad4`. GitHub push run
  `33053901131` and PR run `33053905003` both completed green, including the
  required Linux, macOS/ARM, Windows, workspace, strict-Clippy,
  supervised-topology, and estate jobs. That evidence made G00-W05
  dependency-ready; it did not establish the complete product foundation.
- One operation identity: `rrd-contract::WorkPlanOperation` owns the exact
  `sync`, `status`, `activate`, `record`, and `verify` set plus their runtime
  tool, capability, and CLI identities. The engine generates the five public
  runtime descriptors from that set. MCP serializes the engine catalogue;
  Connectome discovers and invokes through authenticated RRD contracts; the
  capability catalogue derives all five surface dispositions from the same
  definitions; and CLI parser parity rejects missing or extra subcommands.
- One execution authority: plan preparation and exact-command verification
  moved out of `rrflow-cli` into `rrd-engine::runtime::workplan`. Adapters may
  provide reviewed plan text and request verification, but cannot provide pass
  results, output digests, repository identity, source-tree identity, or the
  verified transition.
- Privilege boundary: `status` requires `WorkPlanRead`; ordinary board changes
  require `WorkPlanControl`; and the verifier's host-process execution requires
  the distinct `WorkPlanVerifyExecute` action and
  `verification_execution` lifecycle. It is not mislabeled as an ordinary RRD
  control transition. Verification still executes only the exact argv already
  sealed into the active work-item plan and retains bounded failed-command
  artifacts.
- Persistent focused proof: all five operations execute through the generated
  engine catalogue; successful evidence and verified state survive RRD reopen;
  a failed exact command leaves the item active after reopen; source-tree drift
  cannot cross verification; MCP stdio exposes every generated identity; and
  Connectome discovers all five and invokes status through its authenticated
  backend. Product-surface parity and CLI parser parity tests pass.
- V1 contract projection: the additive security and lifecycle vocabulary is
  frozen under OpenAPI digest
  `580c4723bc9b4e3061a81361322868a3b1c913e7ed149f088208d5db4f4e17cb`.
  The TypeScript client was regenerated from `rrd-contract`; generation drift,
  Biome, typecheck, and all client tests pass. The owner-controlled release and
  protocol remain V1; no compatibility reader or parallel schema was added.
- Status boundary: G00-W05 remains active. Focused tests are not completion.
  Only the ten exact commands recorded before implementation may create its
  persisted verification transition, and the resulting commit must then pass
  the required remote platform matrix before work advances.

## 2026-08-27 — G01-W01 in-place V1 identity cutover proof corrections

- Reviewed scope: G01-W01 is bound in RRD to a plan with 13 exact commands.
  The row covers canonical workspace identities, the no-retired-identity gate,
  V1 fixture reopen, five generated SDK projections, formatting, workspace
  check/test, and warning-denying Clippy. It does not claim the G01-W02 unified
  engine-authority boundary or any later capability gate.
- Runtime enforcement: lifecycle session `codex-g01-w01` reached
  `plan.recorded` with the active work item, reasoning run, source-tree digest,
  reviewed-plan digest, and verification-plan digest. The first supervised
  mutation was denied because a later preflight made the recorded attunement
  receipt stale. The worktree remained unchanged; the same plan was rebound to
  the fresh receipt before the retry. This is invocation `167` followed by the
  successful supervised mutation at invocation `175`.
- Alias correction: read-stamp trace links no longer publish both
  `commit_cursor` and a compatibility `cursor`. The canonical field remains;
  a negative assertion prevents the compatibility field from returning.
  Canonical cursor fields belonging to runtime-cursor and snapshot link kinds
  are unchanged.
- Package identity correction: workspace metadata validation now rejects Cargo
  rename aliases for every workspace edge, including development edges, and
  requires every package manifest to live in a directory matching the package
  name. The existing case-insensitive active-tree scan remains no-allowlist.
- V1 recovery correction: the checked-in snapshot-bundle fixture must be
  byte-identical to a newly exported canonical V1 bundle. The test now installs
  and reinstalls that checked-in fixture, performs the successor write, and
  reopens the target database. Previously, the fixture was decoded and read but
  the install/reopen path used the freshly generated in-memory bundle.
- Passing focused command: `/tmp/rrflow-g00-w05-tools/cargo test -p rrd-core
  -p rrd-lsm -p rrd-engine --all-targets --locked` completed with exit code 0
  through `rrflow exec` (invocation `180`).
- Completion evidence: G01-W01 is persisted as verified under digest
  `c6a9a19a057baad305e57a73b04680a7e9de5ee661ff0948b07146186af442a2`.
  The source commit is `38f1a19bcc9ad76389cceed7eb0988452e694235`; push run
  `33075846902` and PR run `33075851923` completed green. Publication of this
  journal/ledger evidence remains a separate clean-tree step and must not
  silently absorb G01-W02 work.

## 2026-08-27 — G01-W02 canonical platform baseline slice

- One terminology authority: `docs/platform/README.md` owns the exact ordered
  list of 25 platform terms. `rrd-contract::PLATFORM_TERMS` is its
  machine-readable mirror; tests reject duplicates, order drift, missing public
  resource mappings, and public `domain`, `boundary`, `workspace`, or
  `umbrella` resources.
- Public resource baseline: `ResourceKind` now contains organization, estate,
  project, instance, tenant, namespace, database, table, collection, record,
  point, relation, alias, cluster, node, shard, replica, and segment, while
  retaining transaction, snapshot, backup, and operation. The intentional
  additive OpenAPI change is frozen under digest
  `bfc1607e18f225faaa3d5d374d25bd455ba718f1f1636df08df9800003449eb3`;
  the TypeScript schema was regenerated and its generation, lint, typecheck,
  and tests pass.
- Project-instance enforcement: executable multi-project topology was removed.
  Manifest format 1 accepts only `mode = "dedicated"` and `members = ["."]`;
  nested projects, alternate modes, foreign stores, and startup rebinding fail
  closed. The old fields remain serialized so existing manifest and
  `ProjectAuthorityBinding` digests remain byte-compatible.
- Explicit migration boundary: environment is a required canonical instance
  attribute but is not defaulted into format 1. A successor manifest and
  project-authority format must migrate it explicitly with reopen and rollback
  evidence; silently changing V1 would invalidate persisted authority.
- One documentation direction: `docs/rrflow-rrd-architecture.md` is the sole
  end-to-end implementation map; `docs/platform/research/README.md` is the one
  research index; root PLAN/STATUS and active topology, UI, trace, graph, and
  Clyffy notes now point toward estate-managed project instances instead of the
  retired topology.
- Local qualification passed: locked metadata; `rrd-contract` all targets;
  focused instance/architecture, CLI, and server targets; TypeScript `pnpm
  check`; formatting; workspace all-target check; full workspace all-target
  tests with no fail-fast; and workspace all-target Clippy with `-D warnings`.
- Status boundary: this is a tested G01-W02 baseline slice, not completion of
  the work item. G01-W02 remains active until one `RrdEngine` authority absorbs
  the remaining `EmbeddedOperator`/`runtime_store` escape and independent
  `PersistentEngine` openings and proves one catalogue, transaction
  coordinator, read stamp, security authority, event log, and audit root.

## 2026-08-27 — G01-W02 single RrdEngine authority consolidation

- One embedded handle: claim, recall, invocation, projection, runtime,
  work-plan, archive, backup, migration, and format-administration operations
  now belong to `RrdEngine`. The `EmbeddedOperator` type, public operator
  module, and `runtime_store` escape were removed without an alias or forwarding
  compatibility constructor. `rrflow` discovers and verifies the canonical
  `<project>/.rrflow/rrd` binding before opening it.
- One product-executable boundary: `rrd-security-bootstrap`,
  `rrd-estate-admin`, `rrd-estate-controller`, and `rrd-backup-controller` are
  thin outward binaries owned by `rrflow-cli`. Their production sources import
  only the engine/contract boundary. Security and estate policy, repository,
  process-driver, backup-driver, and storage construction moved inside typed
  engine operations; `rrd-security` and `rrd-estate` retain domain/physical
  libraries but no longer declare those binaries.
- Identity correction: estate commands require a distinct
  `--authority-instance`; an estate ID is never silently reused as the identity
  of the running RRD engine. Denied admin policy is still evaluated before the
  database can be created.
- Store initialization correction: project-bound and local-authority opens now
  initialize/authenticate the RRD substrate before placing `RRD.SECRET` inside
  a missing database directory. This prevents a fresh token file from causing
  the backend selector to misclassify the directory as compatibility storage.
- Persistent/process parity: security drift/idempotency, estate admin replay
  and journal identity, backup effect-gap kill/replay, physical estate state
  machines, and the real RRD child/controller start-stop crash matrix pass
  after executable ownership moves.
- Executable invariant: `workspace_architecture` now rejects outward production
  `PersistentEngine::open`, `EmbeddedOperator`, `runtime_store`, a public
  operator module, untracked Cargo targets, or physical-crate ownership of the
  four product executables.
- Cross-authority proof: `engine_authority` initializes security and estate
  state, creates an authenticated session, commits schema/record/vector/claim
  mutations, records lifecycle state, advances vector and query catalogues,
  observes the common diagnostic read stamp and changefeed head, proves allowed
  and denied audit records, closes/reopens, and replays the exact close and
  bootstrap identities through one `RrdEngine` database.
- Whole-workspace qualification exposed a process-fixture identity race: the
  estate driver had authenticated Cargo's mutable profile-root `rrd-server`
  path. The fixture now hard-links that artifact into a test-lifetime snapshot
  before hashing or spawning it, so an artifact replacement cannot invalidate
  the trusted executable during the crash/reopen matrix.
- Verification boundary: all 12 exact evidence-tree local commands pass: focused
  engine authority and architecture tests; CLI, security, estate, server, and
  MCP all-target tests; formatting; workspace all-target check; the complete
  workspace all-target test suite with no fail-fast; warning-denying workspace
  Clippy; and diff whitespace validation. A recovered in-flight `work-plan
  verify` process persisted G01-W02 as verified with digest
  `f8efbee2b5bd20cdb29e2746450c83266720411c672429aedf4b57d8f566f9b6`,
  bringing the executable board to 7/65 with no active item. The executable
  board does not itself enforce the reviewed plan's publication/remote-CI
  condition, so Linux/macOS/Windows evidence on a published commit remains an
  explicit release gap rather than a claim silently manufactured here. G03
  still owns multi-model catalogue breadth and Arrow/DataFusion execution; G04
  owns the complete Qdrant-shaped collection/TurboQuant memory policy; G05 owns
  compiled RBAC/privilege performance and full security-service qualification.

## 2026-08-27 — G01-W04 authoritative product capability catalogue

- Board closure: G01-W04 is persisted as verified under digest
  `19c16b4442e5a46f9d820ef9e2edcc6616462dc4a5184375001998902b23f8d2`.
  The exact source commit is
  `b21e8408c9fd2170f40383da7b86f9dd868fc160`; GitHub push run
  `33126648937` and PR run `33126651843` both completed successfully.
- Contract: the RRD V1 product surface state is now exactly `available`,
  `planned`, `denied`, or `unavailable`. Available bindings carry one or more
  sorted, unique canonical entrypoints and no reason. Non-executable bindings
  carry no entrypoint and require an explicit bounded reason.
- One catalogue: `rrd-engine::product_capability_catalogue` derives executable
  rows from the public endpoint catalogue and the engine-owned runtime-tool
  catalogue, then appends visible planned foundation gaps. One row may expose
  multiple real entrypoints on the same surface, including a direct RRD HTTP
  operation and the generic governed runtime-tool invocation route.
- Surface binding: every runtime tool is engine-executable, invokable through
  `POST /v1/runtime/tools/invoke`, advertised by MCP, and invokable through
  Connectome's client-only runtime route. Generated work-plan tools also expose
  their canonical RRFlow CLI commands; missing CLI/operation mappings are
  explicitly unavailable rather than implied or silently omitted.
- Negotiation: `ServiceCapabilities` now embeds and validates the complete
  product catalogue. RRD server construction uses the engine source directly;
  Rust client and Connectome process tests assert exact equality with the
  engine catalogue. The checked-in TypeScript V1 OpenAPI types were regenerated
  from the reviewed Rust schema without introducing a V2 or compatibility shim.
- Drift gates: architecture tests require MCP `tools/list` to serialize its
  authority catalogue, require Connectome to fetch the runtime catalogue, reject
  hardcoded runtime-tool definitions in either outward source, and require both
  CI matrices to build `rrd-estate-controller` from its actual `rrflow-cli`
  product owner rather than the retired physical crate.
- Closing evidence: capability catalogue and workspace architecture tests,
  all RRD contract targets, TypeScript generation/lint/type/test checks, the
  server/client/MCP/Connectome target suites, full workspace, strict Clippy,
  exact work-plan verification, publication, and both remote matrices passed
  for the recorded source identity.

## 2026-08-27 — G02-W01 native WAL/MVCC/LSM recovery foundation

- Board closure: G02-W01 is verified under digest
  `1e17ab3441eef467442984df3389e2206b253f287b2f93b51fad54a31f1025bd`.
  Its eleven-command verifier covers `rrd-lsm`, `rrd-store`, `rrd-engine`,
  `rrd-cluster`, the estate controller, `rrd-server`, formatting, complete
  workspace check/test, warning-denying Clippy, and staged diff validation.
  Candidate run `33142404999` passed Linux, macOS, Windows, topology, and the
  stable `ci-gate` for commit `e017e33a6c7cad85597189d50918bc2884b373c3`.
- One native writer: the manifest lock is held for the database lifetime and
  acquired with a non-blocking exclusive lock. A second in-process or
  cross-process writer fails promptly with an explicit active-writer error;
  reopening succeeds after ordinary drop or SIGKILL owner death. There is no
  second engine, lock authority, or outward storage-opening surface.
- Ordinary WAL boundary contract: borrowed and owned atomic batches expose
  deterministic `before_wal_append` and `wal_synced` failure points under
  crash and storage-full modes. Pre-WAL failure publishes none and remains
  usable; post-sync failure forces reopen, then recovers the complete batch.
  Existing torn-header, torn-payload, checksum-corruption, flush, compaction,
  snapshot, and reopen matrices remain authoritative for their boundaries.
- Exact stamped audit: `DataTransaction.read` now reaches the backend's
  atomic commit primitive. Memory, Fjall compatibility, and native storage
  persist the exact read stamp in the same batch as changes, outbox work,
  receipt, cursor, and audit; unstamped `commit_runtime` remains explicitly
  unstamped. Idempotent replay retains the first accepted audit unchanged.
- Multi-family recovery proof: one schema + record + event + claim transaction
  is injected at all four crash/storage-full and pre-WAL/WAL-synced
  combinations. Reopen proves all-or-none schema, record, event log, claim,
  cursor, outbox, outcome, commit ID, and stamped audit state, then accepts a
  successor write.
- Process and engine proof: the existing test-only durability child now commits
  one nine-mutation transaction spanning schema, records, relation, event,
  vector, time-series, geospatial, and claim families, reports readiness, and
  is terminated with SIGKILL. Reopen recovers all nine changes as one commit,
  one claim, the exact actor/read/audit identity, and the commit outcome.
  A separate child holds the native writer lock to prove prompt contention
  denial and owner-death recovery. An internal `RrdEngine` regression proves
  the session read stamp, canonical commit ID, audit request ID, outcome cursor,
  idempotent replay, and reopen identity all agree through the sole engine
  authority.
- Focused evidence passed: `rrd-store` all-target compilation; the injected
  native transaction matrix; all seven real process durability tests; the
  focused `RrdEngine` transaction-stamp test; snapshot and unified-data
  stamped-audit tests; and rustfmt. The final exact eleven-command tree,
  publication SHA, remote platform matrix, and work-plan verifier then passed
  before G02-W02 was activated.

## 2026-08-27 — G02-W02 bounded resumable logical archive candidate

- Runtime-enforced scope: RRD has G02-W02 active with the reviewed plan and an
  exact 13-command verifier. The board remains 10/65 verified; this entry does
  not mark G02-W02 complete.
- Proven defect: the pre-existing restore regenerated transaction audits via
  unstamped `commit_runtime`, changing `AuditEnvelope.read` from the original
  `ReadStamp` to `None` and therefore changing the audit digest. A regression
  test failed on that exact difference before implementation.
- Atomic correction: each V1 runtime action now carries its original audit
  envelope. A crate-private native restore path runs ordinary commit planning,
  validates the archived envelope against commit identity, cursor, previous
  audit head, and historical read stamp, and writes it in the same atomic batch.
  No public Engine/HTTP/MCP/SDK mutation bypass was added.
- Bounded stream: `rrd-store::archive` is split into facade, export, format,
  receipt, and restore modules. Export uses two fixed-page passes, retains one
  page plus one bounded action, and never builds whole claim/change/commit/
  action vectors. Object and catalogue manifests have explicit V1 count/byte
  bounds; object bytes already move through verified streaming IO.
- Resume contract: deterministic private partial/staging paths and
  content-authenticated receipts retain fixed watermarks, action ordinal,
  claim/runtime coordinates, audit head, prefix bytes, and prefix digest.
  Export and restore both reconcile a durable action whose receipt was not yet
  published. Source advancement invalidates an unfinished export; receipt,
  partial, archive, staging, object, and catalogue divergence fail closed.
- Focused evidence: the new resilience target passes 12/12 tests. Together
  with 4/4 logical archive and 5/5 backup catalogue tests, it covers multi-page
  claims, split commits, all typed families, original transaction audits, both
  action/receipt boundaries plus the finalized-footer/publication boundary,
  receipt tampering, source drift, corruption, truncation, object/catalogue/
  audit closure, and reopen.
- Local candidate evidence: all 13 reviewed commands pass, including focused
  archive/catalogue suites, all `rrd-store`, `rrd-engine`, CLI, and server
  targets, formatting, complete workspace all-target check and test, warning-
  denying workspace Clippy, and diff validation. This is local candidate
  evidence only; it does not cross off G02-W02.
- Pending before board verification: commit/push this exact implementation,
  require Linux, macOS ARM, and Windows success for that commit, journal the
  source identity and remote runs, execute the RRD verifier on the unchanged
  published tree, and only then allow RRD to cross off G02-W02.
- First remote candidate evidence: commit `54ab71111942345486a6c4646b2384bc40d6457e`
  produced run `33151716794`. macOS, Windows, and supervised RRD + Connectome
  topology passed. Linux workspace verification failed before tests ran because
  two concurrent `rust-lld` processes crashed with `SIGBUS` inside LLVM
  `parallelFor` while linking separate `rrd-engine` test binaries. The Linux-
  heavy job now serializes Cargo build jobs and bounds rust-lld to one thread;
  an architecture test freezes both resource limits. This failed attempt is
  retained as evidence and G02-W02 remains active pending a complete green run.
- Second remote candidate evidence: correction commit
  `d1cba992b0b75555f1414427e0fa0260fffb721b` produced run `33153581325`.
  macOS, Windows, and supervised RRD + Connectome topology passed again. The
  Linux verifier honored serialized Cargo jobs and the one-thread linker flag,
  then reported only 49 MB of free runner disk immediately before `rust-lld`
  received `SIGBUS` while linking `rrflow-mcp`'s `stdio_daemon` test. This was
  runner disk exhaustion, not a test assertion or an unbounded linker. Global
  `RUSTFLAGS` had also invalidated the restored Rust artifact set and forced a
  second full workspace build. The next correction therefore retains one Cargo
  build job, disables unused test debuginfo through the Cargo profile variable,
  uses a fresh matching cache namespace, refuses to cache failed verifier
  artifacts, and records disk capacity around the unchanged full-workspace test.
  G02-W02 remains active; neither failed remote run is completion evidence.
- Local correction evidence: with `CARGO_BUILD_JOBS=1` and
  `CARGO_PROFILE_TEST_DEBUG=0`, the 16-test workspace architecture gate passed
  from a cold profile build, then the exact previously failing all-features
  `rrflow-mcp` `stdio_daemon` integration target compiled, linked, and passed.
  This proves the corrected settings are accepted by Cargo and preserve the
  failed target's behavior; only a complete remote matrix can qualify the
  candidate.
- Third remote candidate evidence: correction commit
  `adb6440c8bfafdf9efa184b7ba32229e23a602d1` produced run `33156754813`.
  macOS, Windows, and supervised RRD + Connectome topology passed. Linux passed
  version enforcement, the 16-test architecture gate, its controller build,
  and the pre-test capacity probe. The unchanged all-features workspace test
  then remained in its cold build when GitHub cancelled the verifier at the
  configured 30-minute budget; no test failure or disk warning was reported.
  Failed artifacts were intentionally not cached, so another candidate must
  admit a cold build. The Linux-heavy gate remains bounded but now allows 60
  minutes for the measured cold test-and-Clippy path. G02-W02 remains active.
- Fourth remote candidate evidence: correction commit
  `74704e04b34fd631a6f77d59c4cbdb0c736346ff` produced run `33159783047`.
  macOS, Windows, and supervised RRD + Connectome topology passed. Linux again
  passed version enforcement, architecture, controller build, and the pre-test
  capacity probe, but whole-build `CARGO_BUILD_JOBS=1` left the cold workspace
  test compiling until GitHub cancelled the job at its 60-minute bound. This
  proves serializing all Rust compilation is not an admissible fallback. The
  next correction uses both hosted compile cores while bounding each
  `rust-lld` invocation to one thread, retains stripped test debuginfo, and
  moves to a fresh cache namespace matching that exact compiler/linker profile.
  Failed artifacts remain unpublishable and G02-W02 remains active.
- Local targeted-concurrency evidence: with two Cargo jobs, one thread per
  `rust-lld`, and stripped test debuginfo, the cold architecture target built in
  3m58s and passed 16/16; the exact all-features `rrflow-mcp` `stdio_daemon`
  target then built in 2m59s, linked without a signal, and passed. This retains
  the resource guard while removing the proven whole-build serialization
  bottleneck. Remote qualification remains required.
- Fifth remote candidate evidence: correction commit
  `92e7317c7b21b05f19ecac230623b8ef56659d7b` produced run `33164751681`.
  macOS and supervised topology passed. Linux honored two compile jobs,
  one-thread linkers, and stripped test debuginfo, but the globally unified
  all-features workspace still filled the target filesystem: `rustc` reported
  `No space left on device` and concurrent test outputs then ended in a linker
  bus error. Windows independently exposed nondeterministic fixture readiness:
  one process test exceeded its 15-second startup-log wait and the effect-gap
  stop used its bounded kill fallback before the server's request-file watcher
  was proven ready. These are not archive assertion failures. The correction is
  structural: test the complete default-feature workspace once, qualify each
  package's optional features in isolated target directories, isolate the
  pgvector service to its feature job, and make real-process fixtures observe
  server readiness before asserting graceful shutdown. G02-W02 remains active.
- Local structural-correction evidence: Cargo metadata identifies exactly five
  workspace packages with non-default features. The reusable CI contract now
  tests the complete default-feature workspace once, qualifies `rrd-cluster`,
  `rrd-engine`, `rrd-inference`, and `rrd-vector` in isolated all-feature
  matrix jobs, and qualifies `rrd-operator-knowledge` with its own disposable
  pgvector service. The architecture gate derives that set from metadata and
  rejects an omitted package, a global all-features workspace test, a shared
  heavy cache, an unbounded job, or a gate that ignores either new partition.
- Exact local evidence for that partition passes: default workspace test and
  warning-denying Clippy; all-feature test and warning-denying Clippy for each
  of the five optional-feature packages; 16/16 workspace architecture tests;
  and the concurrent four-test local estate-driver target after the process
  fixture was changed to observe the real RRD HTTP-ready diagnostic before
  requesting graceful shutdown. The first local pgvector attempt correctly
  failed closed because its database name was outside the required
  `rrd_operator_knowledge_test*` disposable namespace; the rerun used an
  isolated contract-compliant database and passed every live, golden, sync,
  unit, and Clippy check. This remains candidate evidence until the exact
  committed tree passes the complete remote platform matrix.
- Sixth remote candidate evidence: correction commit
  `e7e447571b5ec427cf78a54da0a3e3daebf17cc3` produced run `33170028933`.
  macOS, Windows, supervised RRD + Connectome topology, pgvector, and all four
  other isolated optional-feature jobs passed. The default Linux verifier had
  no restored cache and reported 7.8 GB free before its workspace test, then
  launched the `rrd-graph` `route_vs_scan` example and `entities` integration
  test linkers concurrently; both one-thread `rust-lld` processes terminated
  with `SIGBUS`. No Rust test assertion or optional-feature partition failed.
  The structural split is retained, while the complete default-workspace
  verifier now admits only one live Cargo compile/link job and uses another
  fresh cache identity. G02-W02 remains active.
- Seventh remote candidate evidence: correction commit
  `339bbc20adb7c83875006a0a5292f759f2503066` produced run `33171814626`.
  Every non-Linux-default partition passed, including Windows process recovery.
  Linux used one Cargo job and one linker thread with no restored cache, again
  reported 7.8 GB free before the default-workspace test, and then a single
  `rrd-graph` `route_persisted` example linker terminated with `SIGBUS`. This
  disproves concurrency as the remaining cause and identifies cumulative
  whole-workspace target size as the boundary. Default-feature qualification
  is therefore split into one isolated test/Clippy matrix job per Cargo package.
  The architecture gate derives the complete expected matrix from Cargo
  metadata, so a new or omitted package fails the policy job. The policy and
  evidence verifier no longer emits the workspace's test binaries. G02-W02
  remains active.
- Eighth remote candidate evidence: correction commit
  `a044738f6ae0f49ec0e1936200a1c2254bc63a38` produced run `33174362335`.
  Thirty of thirty-one substantive jobs passed: policy/evidence, topology,
  macOS, Windows, all optional features, pgvector, and 21 isolated default-
  feature packages. The `rrd-server` package test alone failed because its
  real-process fixture requires the product-owned `rrd-estate-controller`
  binary, which the monolithic workspace target had supplied through implicit
  build order. The isolated server matrix now declares and builds that exact
  cross-package fixture before testing. An architecture invariant binds the
  conditional fixture to `rrd-server`; no physical-crate executable or hidden
  workspace ordering is reintroduced. G02-W02 remains active.
- Ninth remote candidate evidence: correction commit
  `2eceb038fa850e248a609cab75c324364d4b3dc6` produced run `33176084700`.
  All thirty-one substantive partitions and the stable `pipeline / ci-gate`
  passed on the same commit, including the formerly failing real-process server
  fixture. This closes the remote CI failure sequence as infrastructure/test
  ownership work rather than an archive defect.
- CI cohesion correction: default-feature qualification is now expressed as
  five product-subsystem suites (`kernel-storage`, `query-retrieval`,
  `engine-authority`, `service-protocol`, and `product-operations`) instead of
  twenty-two package-named checks. Every workspace package is still tested and
  strictly linted exactly once, while the bounded compiler/linker profile and
  isolated caches retain the hosted-runner disk boundary. CI-specific policy
  moved from the Rust engine architecture test into
  `scripts/ci/check_workflow.py`; engine architecture now enforces only product
  composition, dependency direction, executable ownership, storage opening,
  and outward-surface invariants.
- Exact local evidence for the five-suite candidate passes under the same
  bounded profile used remotely: all five suite test commands, all five strict
  Clippy commands, 14/14 engine architecture tests, formatting, workflow YAML
  parsing, CI policy coverage for 22 default-feature and five optional-feature
  packages, and `git diff --check`. An initial unbounded local query build filled
  the pre-existing shared Cargo cache and failed during linking; removing only
  rebuildable failed-link artifacts and rerunning with `CARGO_INCREMENTAL=0`,
  stripped test debuginfo, two build jobs, and one linker thread passed the
  complete query/retrieval lane.
- The first remote five-suite candidate at `acdf647` preserved the substantive
  gates and exposed a real lifecycle race in the unchanged `rrd-cluster`
  optional-feature lane: OpenRaft joined its core task but its detached
  state-machine worker could still own `MANIFEST.LOCK` when the consensus test
  immediately reopened canonical runtime state. The cluster adapter now
  exposes non-owning release evidence for both physical databases. Test
  shutdown consumes every Raft handle and waits for that evidence; production
  node shutdown aborts and joins transport, drops known storage owners, and
  does not return until both writer locks are released or a bounded error is
  reported. The native engine continues to fail closed on genuine concurrent
  writers.
- Exact local correction evidence passes the complete
  `rrd-cluster --all-features` test lane, strict all-target Clippy, formatting,
  and diff validation. The real two-test consensus shutdown/reopen target also
  passed thirty consecutive executions (sixty consensus tests) without a
  writer-lock race.
- The replacement candidate at `05bb55aff0186bca257eff4ac4160a5959e96734`
  produced run `33182150466`. Every Linux subsystem and optional-feature lane,
  macOS estate process qualification, topology, and verification passed. The
  Windows estate process lane exposed a separate lifecycle gap: the process
  driver persisted an applied start after PID/executable stability while the
  RRD listener could still be initializing, and the fixture eventually timed
  out waiting for the incidental stderr startup line. The aggregate gate
  correctly failed; this run is not completion evidence.
- Process start completion now has an explicit catalogue-owned readiness
  contract. The RRD deployment passes a direct `RRD.READY` path; `rrd-server`
  durably publishes its bound URL only after listener creation; and the driver
  removes stale evidence, monitors early exit, enforces the declared deadline,
  kills on failure, and persists the process record only after readiness. The
  estate suite and the exact four-test real child/controller recovery target
  pass locally with the corrected boundary. Remote Windows and aggregate
  qualification remain required.

## 2026-08-28 — G02-W03 estate-owned retention and verified recovery candidate

- Added one estate recovery authority: revisioned per-instance RPO/RTO,
  minimum-point and retention policy; exact policy snapshots in backup request
  identity; authenticated recovery points; expiring policy pins; explicit
  holds; deterministic retention decisions; bounded restore evidence; and
  path-free public snapshots. Legacy estate/backup JSON remains decodable.
- Backup completion and recovery-point/policy-pin creation share one estate
  CAS transition. Estate backup creation now retains the application-complete
  archive, immutable-object closure, and catalogue closure rather than
  promoting a logical-only archive into managed recovery posture.
- Added authenticated catalogue prune with exact digest and complete disjoint
  retained/candidate partition checks. Its successor carries bounded replay
  evidence; publication precedes reclamation; shared archives, manifests, and
  objects survive; stale, unknown, corrupt, overlapping, or resurrected
  identities fail closed.
- `RrdEngine` persists an estate prune intent before the physical effect. The
  intent fences backup completion, policy, hold, and restore changes; exact
  replay completes the storage effect and then records pruned history without
  deleting recovery evidence. No second catalogue or transaction authority was
  added.
- Estate restore accepts identities only, places data under the fixed
  `restores/<instance>/<restore-id>` hierarchy, refuses symlink/divergent
  targets, holds the selected point during work, verifies archive/object/
  catalogue closure and reopened watermarks, records measured RPO/RTO, and
  releases the hold. The active instance root is never renamed or overwritten.
- Extended the existing authorized estate admin and added the thin
  `rrd-recovery-controller` adapter. Focused estate, store, engine, CLI, reopen,
  shared-artifact, stale/corrupt partition, fencing, replay, and RPO/RTO tests
  pass locally. Commit `b079151164d87873230aa24c7ff0a25d3515d96e` passed all
  fourteen remote partitions and `pipeline / ci-gate` in run `33217251152`;
  RRFlow verified G02-W03 with evidence
  `73bce44ed49f581b40227a2f8db789f87bd4ac564938aeb0f05dd89705d82956`.

## 2026-08-28 — G02-W04 exact-successor and reverse-recovery candidate

- Froze the supported native application-format edge to TextV1 → TagV2. The
  matrix is public and bounded; it does not advertise a generic numeric or
  skipped-version upgrader.
- Added fail-closed native-format rollback after cutover. Rollback verifies the
  visible TagV2 successor and retained TextV1 predecessor against the
  authenticated archive and recorded cutover manifest, retains the successor,
  and republishes the predecessor through durable sibling renames.
- Added authenticated `rollback_target_moved` and `rolled_back` phases plus
  reconciliation at both reverse rename windows. Retry is idempotent, forward
  reuse after rollback is denied, and no phase deletes the predecessor,
  successor, archive, or ledger.
- Added the engine-owned `storage format-rollback` operator path. Focused tests
  cover every forward and reverse durability boundary, successor divergence,
  exact-edge refusal, direct TextV1 migration, logical TextV1 archive recovery
  into a fresh TagV2 root, CLI status, and reopening both retained roots.

## 2026-08-29 — G02-W05 bitemporal reads and forward rollback candidate

- Corrected RRFlowQL historical planning so a literal `KNOWN` cursor bounds
  source watermarks and exact-index eligibility. A newer artifact can no
  longer be selected for an older read; authoritative replay remains the
  fallback.
- Corrected claim query replay to exclude changes after the known cursor and
  resolve same-valid-time corrections newest-first by transaction cursor.
  Cross-engine tests prove identical historical/current results on memory,
  Fjall, native, and native reopen.
- Added a transport-neutral forward rollback request, deterministic plan,
  structural counts, and receipt. `RrdEngine` reconstructs current and target
  record/relation snapshots from the authenticated log, emits restored or
  closing versions at the explicit later effective time, and binds the target
  snapshot by SHA-256.
- Added the governed `rrflow_data_rollback` runtime tool under the existing
  transaction-commit security action. Compensation plus a
  `historical-rollback` evidence claim uses the normal read-stamp CAS,
  idempotency ledger, runtime hash chain, projection outbox, and atomic accepted
  audit envelope. Exact retry after native reopen returns the stored commit.
- The end-to-end fixture proves changed-state restoration, later-record and
  later-relation retirement, unchanged pre-rollback `KNOWN` reads, evidence
  queryability, invalid target denial, capability/discovery parity, and reopen
  replay. Append-only events and other non-structural families are deliberately
  excluded pending their own restoration semantics.

## 2026-08-29 — G02-W06 tiered persistence candidate

- Replaced the whole-object S3 compatibility shim with capability-gated,
  authenticated multipart semantics. Admitted transports must provide signed
  payloads, conditional PUT/completion, SHA-256, complete pagination,
  resumable multipart/list-parts, and version-bound ranged reads.
- Added fixed part/range and attempt policies. Restart discovers the existing
  upload and reuses only exact part-number/length/digest matches; transient
  transport failures retry within the fixed bound, while corruption and
  metadata divergence fail immediately.
- Changed the immutable object reader contract to permit a borrowed bounded
  remote range reader. Logical archives and cluster artifacts can therefore
  use the same verified streaming path without allocating the whole remote
  object.
- Added one native immutable-segment I/O policy across mmap, actual Linux
  io_uring after setup/opcode probing, and portable positional fallback. Every
  backend feeds the existing block checksum, decompression, and shared cache;
  operation count, bytes, peak request, selected backend, and fallback evidence
  are exposed separately from decoded cache residency.
- Fault tests cover anonymous capability refusal, interrupted multipart resume,
  bounded transient retry, actual logical-archive upload, local loss/ranged
  recovery, remote corruption, mmap/bounded parity, and io_uring execution or
  explicit fallback on the running kernel.

## 2026-08-29 — G03-W01 unified logical catalogue candidate

- Extended the existing `RuntimeSchemaRegistry`; no side catalogue was added.
  One registry revision now carries namespace/database coordinates and table
  definitions for document, relational, graph, key-value, vector, event,
  time-series, geo, object, reasoning claim/record/event, and lifecycle
  identities.
- Added explicit tagged strict/schemaless mode. Record/relation/event strict
  entries retain their specialized constraints; structurally typed vector,
  series, geo, and object entries use the same table property contract.
  Schemaless entries cannot simultaneously claim a strict schema, and table
  kinds cannot cross mutation families.
- Retained exact pre-G03 migration compatibility: old record/relation/event
  registries derive strict catalogue rows and keep their historical canonical
  commit bytes. Once any explicit table is published, all non-claim mutations
  require a matching logical-model entry.
- Namespace/database/table definitions round-trip through the public contract
  and participate in new commit identity. A failed schema-plus-vector revision
  advances neither cursor nor schema; a corrected retry publishes both.
- Protocol review: the additive catalogue identity and schema-mode vocabulary
  intentionally advances the generated OpenAPI digest to
  `3d3848f74205c56c4fb3dff00070d3c2f4df5a92addd0b7e83a9b122594231f5`.
- Differential tests cover one mixed catalogue, strict and schemaless records,
  graph relation, events, vector, series, geo, object, reasoning/lifecycle
  identities, atomic migration failure/retry, and exact Fjall/native reopen.

## 2026-08-29 — G03-W02 unified multi-model CRUD candidate

- Added `RuntimeRetirement`, a model-tagged, valid-time mutation that closes
  document, key-value, record, relation/native-edge, event, vector, series,
  geo, and object identities without deleting their authenticated history.
  Event corrections use immutable cursor targets; reasoning claims continue to
  use the existing bi-temporal claim contract.
- Added one `RuntimeDataSnapshot` reducer across every model family. It returns
  the exact catalogue revision, valid time, known transaction cursor, and read
  manifest used by the operation. The authenticated public engine read exposes
  the same transaction-mutation vocabulary and retains named-vector collection
  identity losslessly. Schema selection is replayed and revision-checked from
  the same authenticated prefix, so a concurrent catalogue commit cannot tear
  the snapshot.
- Retirement targets are validated against the observed pre-commit snapshot;
  the backend cursor compare-and-swap remains the final concurrency authority.
  A missing target or racing writer therefore cannot partially publish a
  mixed-model commit. Physical current-state rows are removed atomically while
  immutable log replay retains historical reads and supports recreation.
- Differential fixtures exercise create, read, update, retirement, recreation,
  event correction, failed-batch atomicity, and Memory/Fjall/native parity.
  Persistent backends reopen to an identical exact-stamp snapshot and two
  same-cursor writers yield exactly one successful commit.
- Protocol review: typed `DataTarget` and `RetireData` intentionally advance
  the generated transaction schema in the OpenAPI digest to
  `394d4bb27669512b6eecafdede793f43c5b7f70ce3a84bd9f97de4601af8dbc0`.

## 2026-08-29 — G03-W03 RRFlowQL transactions and joins candidate

- Added one-stamp equi-joins to the RRFlowQL AST, canonical parser, catalogue
  binder, logical/physical plans, and executor. Both sides reduce from one
  authenticated log page; output fields are explicitly left/right-qualified.
- Join evaluation has a hard intermediate row bound. It fails closed instead
  of returning an inexact partial join, while accepted results retain the
  existing deterministic bounded-batch contract.
- Added a strict bounded `BEGIN; MUTATE $binding; ...; COMMIT|CANCEL` program.
  Bindings reuse the full transport-neutral `TransactionMutation` vocabulary;
  no parallel mutation model or transaction coordinator was introduced.
- `RrdEngine::execute_query_transaction` validates before opening state and
  delegates to the existing begin/commit/abort authority. Derived operation
  identities bind the program and mutation payload for exact idempotent replay.
- Differential join fixtures cover memory, Fjall, native, and native reopen.
  Transaction fixtures cover commit, cancellation, missing bindings, failed
  mutation atomicity, authorization denial, retry, and native reopen replay.
- Public HTTP/MCP routes, generated SDK adapters, richer mutation expressions,
  outer/costed joins, and the final pushdown/spill execution plane remain their
  explicit later gates.

## 2026-08-29 — G03-W04 shared Arrow/DataFusion execution plane candidate

- Replaced the generic DataFusion `MemTable` boundary with an immutable
  `RrdSnapshotTableProvider`. One schema is derived over the complete captured
  snapshot, every physical batch obeys the request row bound, and schema
  metadata binds the read manifest, scope, valid/known time, source cursor, and
  catalogue revision back to RRD authority.
- DataFusion now executes through `SendableRecordBatchStream`. Provider
  projection and limit hints are applied only when exact; filters are declared
  unsupported and remain above the scan, with the disposition reported as
  stable RRD evidence rather than a serialized DataFusion plan.
- Query contracts now carry memory, temporary-spill, and elapsed-time limits.
  Arrow snapshot residency and DataFusion operator reservations share the
  memory cap, the temporary disk manager enforces the spill cap, and dropping
  the physical stream cancels an elapsed query. Resource exhaustion is surfaced
  as a budget denial.
- Added `EXPLAIN ANALYZE` as a distinct canonical RRFlowQL mode. Its optional
  public evidence reports provider scans, bounded input/output batches, input
  and peak governed memory, pushdown dispositions, physical operator count,
  spill totals, and elapsed time. Ordinary query response bytes remain
  backward compatible; the intentional schema addition advances the generated
  OpenAPI digest to
  `882e5896e390a8f208941beb85ef3577deab5984ac03ddda441b3e46549d6f5c`.
- Differential tests prove reference ordering while a constrained external
  sort actually spills, filter/projection/limit semantics against authoritative
  rows, memory/spill/time denial, stamp corruption rejection, and execution
  from both synchronous and already-async engine callers.

## 2026-08-29 — G03-W05 unified index, full-text, and analytics candidate

- Extended the existing query-index catalogue rather than adding specialized
  side databases. Scalar/compound, unique, count, geo, BM25, filtered
  materialized-view, and grouped-count definitions share one content-addressed
  artifact lifecycle and one exact planner boundary.
- Unique scalar and compound keys are validated across overlapping validity
  windows during build and again against the prospective transaction. Public
  index creation and authoritative commit share an engine-owned serialization
  gate; stale unique artifacts stop accelerating reads but do not stop being
  constraints.
- BM25 artifact v2 adds configurable Unicode-alphanumeric/whitespace
  tokenization, lowercase/case-sensitive analysis, ASCII folding, normalized
  stop words, token bounds, deterministic English stemming, and configurable
  `k1`/`b`. Postings persist token positions and UTF-8 byte offsets; query rows
  expose stable score, matched terms, offsets, and highlighted text.
- Count and grouped-count artifacts persist analytics beside exact filtered
  materialized views. Rebuilds remain outside the commit path, compare the
  prior complete artifact with the new snapshot, record inserted/updated/
  removed evidence, and atomically publish only the completed generation.
  Stale artifacts route to authoritative execution and corrupt selected bytes
  fail closed.
- Public index contracts expose the new kinds, full-text configuration,
  maintenance evidence, and bounded analytics summaries. The reviewed additive
  schema advances the generated OpenAPI digest to
  `327c4eb616053138feb5231c133b1f2db8496e23c6b1cc4f84030f7b0e2b7260`.
- Fixed corpora cover scoring, normalization, deterministic ties, offsets, and
  highlighting. Lifecycle tests cover Memory/Fjall/native/reopen, all index
  families, generation reconciliation, stale authoritative fallback,
  corruption denial, and public transaction rejection without cursor advance.

## 2026-08-29 — G03-W06 durable WebSocket subscriptions candidate

- Added one engine-owned durable subscription record for changefeeds and
  semantic live RRFlowQL queries. Immutable stream definition, owner session,
  durable ACK cursor, retained resume window, lease, fencing generation,
  delivery sequence, and bounded outstanding batches are committed through the
  existing CAS control journal.
- The runtime commit cursor remains the sole ordering coordinate. Deliveries
  are recorded before send; ACKs identify exact outstanding deliveries and are
  cumulative. Reconnect increments a fencing generation, abandons only the
  unacknowledged window, and replays from the durable ACK across process reopen.
- Added authenticated subscription open/close HTTP operations and a dedicated
  Axum WebSocket upgrade route outside the body-consuming fallback. Typed
  opened/changefeed/live-query/heartbeat/acknowledged/error/closed frames are
  shared with the supported Rust client, including custom Rustls mTLS reuse for
  `wss://`.
- Backpressure stops new delivery at the negotiated in-flight bound. Heartbeat
  and ACK frames renew the lease. Resume below `head - retention_window`, stale
  generations, wrong owners, expired leases, malformed frames, and future
  cursors fail closed.
- Subscription operations have distinct policy actions and also require the
  underlying changefeed-follow or live-query-poll grant. Tests cover policy
  denial, ownership, retention, fencing, cumulative ACK, semantic deltas,
  interleaved-writer global ordering, native reopen, real WebSocket push, and
  reconnect replay.
- The additive HTTP/WebSocket catalogue and frame schemas advance the generated
  OpenAPI digest to
  `89ea3a3fe1abd5f672475bf0b964773c10f7f8091ba8f3ce2ff4dd2f0e24331f`.

## 2026-08-29 — G05-W01 cohesive deployment-mode engine candidate

- Added a true in-memory `RrdEngine` composition by type-erasing only the
  storage and immutable-object ports. Memory and native embedded modes continue
  through the same session, transaction, query, policy, audit, and runtime
  commit authorities; no second engine or test-only coordinator was created.
- Replaced the ambiguous `local_server` vocabulary with the closed v1 set:
  `memory`, `embedded`, `local_daemon`, `edge`, `remote`, and `distributed`.
  Loopback servers report `local_daemon`, mutual-TLS servers report `remote`,
  and distributed remains vocabulary rather than an availability claim.
- Added one strict checked-in deployment corpus. The full engine executes it in
  memory and embedded modes; a real child daemon and Rust loopback client and
  the remote mTLS Rust client execute the same transaction/query expectation;
  the offline edge executable builds and mmap-opens the same corpus under its
  intentionally read-only retrieval contract.
- Standalone-process evidence proves readiness, exclusive native-root writer
  ownership, bounded graceful shutdown, and exact reopen cursor. Memory mode
  denies backup, restore, project binding, and physical-root operations before
  preparing durable operator state.
- The additive deployment corpus schemas and corrected public enum advance the
  reviewed generated OpenAPI digest to
  `99c00f57d53c3f17e38590dc849c9e067a4788b8febd898d04401c95c5bc38a6`.

## 2026-08-29 — G05-W02 durable client-transaction foundation candidate

- Reclassified transaction preview as the durable prepare mutation it actually
  is. It now requires and persists an idempotency key, freezes the exact
  write-set digest and runtime time, rejects key or payload substitution, and
  does not append another lifecycle transition on exact replay.
- Added complete prospective read-your-writes reduction over the transaction's
  captured read stamp for schema, record/document/key-value, relation, event,
  vector, series, geo, object-reference, and claim-bearing commits. Preparing
  never advances the authoritative runtime cursor; commit lowers the same
  frozen runtime identity.
- Lifecycle tests now physically reopen the engine across lease renewal/token
  rotation, timeout, abort/replay, commit-intent recovery, runtime acceptance,
  and terminal receipt recovery. Persisted lifecycle records and journals
  continue to contain token hashes only.
- Real-socket evidence prepares the eleven-change all-model transaction,
  restarts and replays the identical prospective snapshot, then commits it. A
  separate cancellation row sends a typed data commit, discards the response,
  stops the server, reopens the same root, and proves retry convergence with
  exactly three runtime changes and no claim duplication.
- Regenerated the TypeScript OpenAPI/types and the TypeScript, Python, Go,
  Java, and .NET endpoint catalogues from the reviewed contract. Transaction
  prepare is consistently marked as a mutation. The OpenAPI digest advances to
  `5e2374350f3a152f825013ed6610db42319868e268e7405174b536801ea2aa91`.
- The first cold board qualification exposed a two-second WebSocket integration
  timeout that could expire under verifier load before the opened frame. The
  real-client fixture now uses one named ten-second I/O bound for connection and
  frame waits; three consecutive secured-server runs pass, including a cold run
  that legitimately took more than the retired two-second allowance.
- The next board qualification exposed resource amplification inside the HTTP
  process tests: every parallel fixture created a full multi-thread Tokio
  runtime. Fixtures now use one async runtime thread, admit at most four live
  servers, and use a named bounded process/socket deadline. Changefeed and live
  waits remain within their public five-second limit. The complete 17-test
  process suite passed three consecutive normal-parallelism runs after the fix.

## 2026-08-29 — G05-W03 identity and authorization foundation candidate

- Expanded the one durable security authority with credential revisions,
  bounded acyclic inherited roles, exact third-party issuer/subject/audience
  bindings, RRD JWT issuer metadata, constrained grants, and one revisioned CAS
  replacement path. Exact replacement replay is a no-op; stale revisions,
  inheritance cycles, ambiguous equally specific policies, and unknown
  references deny.
- Added bounded HS256 JWT issue/verify with issuer, audience, key, principal,
  token, validity-window, policy-revision, and credential-revision binding.
  Only key/credential hashes persist. API-key, JWT, and externally verified
  identities create the same durable principal-bound session.
- Bound every authenticated session to the principal credential revision and
  fixed the shared engine invocation boundary to check that revision before
  authorization. Rotation or revocation now invalidates old JWTs and already
  issued leases across reopen for HTTP, SDK, and tool-adapter callers.
- Added typed tenant and row equality predicates plus allowed-field policy.
  RRFlowQL injects these restrictions before bind/plan, rejects explicit
  forbidden fields, and publishes the compiled policy revision and
  authorization digest. Callers without a policy injector deny constrained
  grants rather than widening them.
- Added owner-only mounted JWT key support to the server, JWT bearer session
  exchange, honest capability negotiation, and the existing rule that every
  non-loopback listener requires initialized security plus TLS 1.3 mTLS.
- Native and real-socket evidence covers role inheritance, exact external
  binding, token tamper/expiry, issuer and credential rotation, principal
  revocation, authority replay/reopen, cross-tenant and row exclusion,
  field projection/denial, durable denied audit, restart replay, old-token and
  old-lease rejection, and recursive exclusion of raw JWT/key material.
- Regenerated the TypeScript OpenAPI schema. The two additive query-plan
  security evidence fields advance the reviewed digest to
  `78716ccb2720fbee0a0beffba06b4efc605f073a8e72ccca4c72b1ed4ccd7fe5`.

## 2026-08-29 — G05-W04 comprehensive structured-audit candidate

- Added one atomic distinct-key control-batch port across memory, Fjall
  compatibility, native RRD LSM, persistent dispatch, and the type-erased
  engine boundary. Every member is compare-and-swap validated before the batch
  publishes, its journal entries receive consecutive sequence/digest links,
  and any conflict rolls the complete batch back.
- Split unsealed audit input from immutable records. Each record commits its
  previous audit digest and semantic SHA-256; the record and compare-and-swap
  audit head publish in one control batch. Bounded contention retry creates one
  linear chain rather than last-writer-wins forks. Page validation retains an
  exact anchor/head across filtered control-journal coordinates.
- Security initialization and replacement now publish state plus a completed
  `security_admin` record atomically. Authorized estate administration,
  reconciliation, backup reconciliation, retention pruning, and restore
  recovery emit `estate_admin` authorization/completion records. Secured direct
  embedded calls outside a supported invocation lifecycle leave a durable
  authorization reservation or terminal denial.
- Added protected `POST /v1/audit/export` and Rust-client support. Its canonical
  newline-delimited JSON carries an exact count, media type, content digest,
  chain anchor/head, and individually verifiable records. Oversized HTTP bodies
  and handler-join failures now cross the same rejection audit boundary instead
  of returning unaudited transport failures.
- Added storage parity/rollback, eight-writer no-fork concurrency, restart,
  direct embedded denial, estate administration, real server, export redaction,
  digest, and chain-continuity evidence. Regenerated TypeScript, Python, Go,
  Java, and .NET route maps at 34 HTTP operations. The reviewed OpenAPI digest
  advances to
  `2a02019b9d89b06714b62d27477413c6216fbb8bff79c371ac24f3b18653ebc3`.
- Terminal structured-audit completion is intentionally still appended after
  an application operation; the pre-effect reservation makes a crash visible.
  Coupling every terminal record to the application data transaction, retention
  policy, and external sink delivery remain later hardening rather than hidden
  claims in this foundation slice.

## 2026-08-29 — G03-W07 governed functions and triggers candidate

- Added one versioned automation contract for digested JavaScript ES2020 and
  portable WebAssembly JSON-v1 definitions, explicit execution limits,
  capabilities, typed trigger selectors/effects, and single-attempt semantics.
  The shared JSON domain also bounds depth/item amplification and rejects
  integers outside the exact cross-runtime range instead of losing precision.
- Added an immutable per-instance automation catalogue. Revision record and
  compare-and-swap head publish in one control batch, validate their SHA-256 on
  reopen, and preserve old revisions for transaction recovery.
- Added fresh-context QuickJS execution with heap, stack, interrupt, input, and
  output bounds. Clock, locale, dynamic code, random, module, network, and
  filesystem authority are absent. Added no-import/no-WASI Wasmi execution
  with memory, value-stack, call-stack, instance, output, and deterministic
  fuel limits.
- Integrated synchronous triggers into the existing transaction coordinator.
  The commit intent pins the automation revision and derived runtime digest;
  retry reexecutes that revision at the frozen transaction time. Triggers only
  inspect original typed mutations and can require boolean true or append one
  capability-gated event to the same authoritative commit.
- Added separate catalogue-read, catalogue-write, and function-execute security
  actions plus trigger authorization and terminal audit evidence. Runtime,
  contract, permission, missing-revision, and resource-limit failures remain
  distinct and cannot advance the data cursor.
- Added deterministic JavaScript/WebAssembly parity, restart, heap, stack,
  interrupt, fuel, import, randomness, atomic derived-event, rejecting-trigger,
  and audit tests. The engine-only capability is published honestly; generated
  outward surfaces remain owned by G06. The reviewed OpenAPI digest advances to
  `3d8a057d9c89a82ab190bf2fe1f5839945dd3943006cbf297c03264312616be2`.

## 2026-08-29 — G04-W01 cohesive vector administration candidate

- Retained one data authority: `CommitTransaction` batches collection-bound
  dense, sparse, and multi-dense point upserts alongside every other model;
  `retire_data` batches point deletion as valid-time facts. Retrieve, scroll,
  search, and deletion preflight reduce the same authenticated runtime history.
- Extended the CAS-protected per-scope collection catalogue with typed boolean,
  integer, unsigned, decimal, keyword, and digest payload indexes. Ensure and
  delete advance exact generations/revisions, retain configuration digests and
  timestamps, enter the authenticated control journal, and converge on durable
  idempotency receipts after conflict or reopen. Present indexed payload values
  are type-checked before any transaction mutation commits.
- Added recoverable collection deletion receipts and an engine deletion gate.
  Metadata cannot be removed while a latest point version is live or
  future-effective at the deletion coordinate, while the declared scan bound
  cannot prove completeness, or while any named vector retains an active HNSW
  or TurboQuant artifact. Historical point mutations remain in the runtime
  log; collection deletion does not rewrite history.
- Added distinct deny-by-default collection-delete and payload-index
  ensure/list/delete actions. HNSW/TurboQuant filter properties now require an
  active collection payload index instead of accepting an ungoverned label.
- Added native catalogue lifecycle/replay/collision evidence and one cohesive
  engine test covering all three vector shapes, two payload indexes, atomic
  batch upsert, indexed-type rejection, restart retrieve, live-delete denial,
  atomic batch retirement, payload-index deletion, collection deletion, and
  post-delete reopen/replay. Generated HTTP/MCP/CLI/SDK administration bindings
  remain G06 ownership; online filtered HNSW is G04-W02. The additive
  collection snapshot and security-action schema advance the reviewed OpenAPI
  digest to
  `bbf2f27f80e47091a56df67b3383b965cbd0aabdcf5a60af1a4f66fb4aa753ed`.

## 2026-08-29 — G04-W02 online filtered HNSW candidate

- Corrected canonical vector reduction so retirement appends a new version at
  the retirement commit cursor instead of rewriting the original put. Reads
  before the retirement cursor remain reproducible, delete freshness is now
  measurable, and online projections receive an exact tombstone delta.
- Added HNSW artifact format v2 (`RRDHNS02`). Unchanged configurations advance
  by inserting only post-generation vector versions into a successor immutable
  graph. Descriptor/body evidence records full-build versus incremental mode,
  previous generation, delta count, total nodes, and source cursor; generation,
  cursor, identity, coverage, duplicate, size, and corruption checks fail closed.
- Added authoritative delta overlay planning and execution. Plans retain the
  immutable graph cursor plus exact overlay cursor/count. HNSW candidates and
  all post-generation puts, updates, and retirements pass through one final
  latest-version/valid-time/filter/exact-score reducer, so writes are searchable
  immediately and deletes suppress old graph nodes before maintenance runs.
- Kept authoritative commits and readers independent of graph work. The active
  immutable generation serves while a successor is built and staged; one
  object/catalogue CAS publishes it. A repeated no-delta ensure is idempotent;
  configuration change deliberately performs a full build.
- Required HNSW/TurboQuant filter fields to remain active typed collection
  payload indexes. Deletion is refused while a live artifact depends on the
  field. Layer-zero eligible admission and exact reranking share the same
  bounded equals/not-equals/in/range/exists/all/any/not evaluator.
- Added runtime-dispatched AVX2 HNSW traversal with portable scalar construction
  and exact reranking. A fixed 512-vector, 16-dimensional corpus covers cosine,
  dot, Euclidean, and Manhattan at 100/50/10/1% selectivity; scalar/automatic
  results are identical and every cell reaches at least 0.95 mean Recall@10 at
  `ef=128`.
- Added nested-filter, immediate insert/retirement, incremental byte reopen,
  eight-generation update/delete soak, collection-bound filtered engine,
  payload-index protection, and post-restart tests. The public OpenAPI digest
  remains `bbf2f27f80e47091a56df67b3383b965cbd0aabdcf5a60af1a4f66fb4aa753ed`
  because index administration is still an engine-only G06 binding; the Rust
  snapshot now exposes maintenance provenance.

## 2026-08-29 — G04-W03 unified multimodal retrieval candidate

- Added one strict recursive `ExecuteRetrievalQuery` contract with bounded
  depth, nodes, prefetch branches, examples, rerank stages, candidates, scanned
  changes, and total candidate-stage work. Unknown fields and dimensional,
  vector-shape, weight, factor, or amplification drift fail before execution.
- Added one `RrdEngine::execute_retrieval_query` authority. It requires both
  query and vector permissions, captures one immutable read stamp and one
  collection catalogue, reads canonical changes once, and requires every
  filter, boost, group, and facet property to be an active typed payload index.
- Added nearest dense/sparse/multi-dense, keyword BM25, average/best-score
  recommendation, target discovery, and context-only sources. Stored examples
  resolve exact named vector references; duplicate active vectors or conflicting
  multimodal payloads fail closed.
- Added bounded nested prefetch, fixed-point weighted reciprocal-rank fusion,
  payload-conditioned add/multiply boost, exact named-vector reranking,
  embedding-model-digest-pinned multi-dense MaxSim reranking, and deterministic
  greedy MMR. Every stage publishes ordinal, input/output count, exactness, plan
  digest, and per-hit rank/score contributions.
- Added point, typed-payload group, typed-payload facet, and directed similarity
  matrix results. Groups and facets deliberately describe the bounded final
  candidate universe; matrices remain directed because MaxSim is asymmetric.
- Added one persistent three-subject/four-vector corpus covering
  keyword+dense+sparse RRF, exact image reranking, model-pinned ColBERT-style
  MaxSim, boost, MMR, both recommendation strategies, discovery, context,
  groups, facets, six directed matrix cells, and exact result equality after
  database reopen. Permission evidence separately denies a principal missing
  query execution and one missing vector search.
- Focused contract, engine scenario, permission, and capability-catalogue gates
  pass. Formatting and strict all-feature Clippy pass across contract, vector,
  security, store, and engine. The same five-crate all-target locked test matrix
  passes serially, including 63 engine tests, 28 public-contract tests, every
  store target, and all vector exact/HNSW/reopen gates. TypeScript generation
  drift, lint, typecheck, and tests pass; the CI workflow policy reports six
  substantive jobs, five cohesive engine suites, 22 default-feature packages,
  and five optional-feature packages.
- The public OpenAPI digest remains
  `bbf2f27f80e47091a56df67b3383b965cbd0aabdcf5a60af1a4f66fb4aa753ed`
  because generated HTTP/MCP/CLI/SDK exposure is G06 work. RRFlow publication
  follows this candidate entry.

## 2026-08-29 — G04-W06 native embedding and governed inference candidate

- Extended `rrd-inference` with a revisioned exact-descriptor backend registry,
  explicit `local_offline` and `remote_provider` trust boundaries, and hard
  per-input, batch-count, batch-byte, and output-value limits. Network, source
  digest, output shape, and normalization violations fail before admission;
  provider/network and resource denials occur before backend dispatch.
- Added a native batch method to the provider-neutral backend contract. The
  default remains compatible with synchronous adapters, while FastEmbed sends
  the validated text batch through one native model invocation. Executable
  sessions and credentials remain process-local and non-serializable.
- Added strict transport-neutral contracts for model catalogue inspection,
  bounded batch generation, and collection-addressed embed-and-search. Public
  results carry read-manifest/cursor, registry revision, backend execution,
  trust/resource evidence, source SHA-256, exact model SHA-256, normalization,
  and generated values.
- Composed the registry into every `RrdEngine` mode with distinct
  `embedding_model_list`, `embedding_generate`, and `embedding_search`
  authorization actions. `embed_and_search_vectors` captures one read stamp,
  validates the backend against the named-vector model contract, and reuses
  `search_vectors_at`; it never commits a temporary query vector.
- Added focused registry/provider/resource/digest tests plus an end-to-end
  engine scenario that batch-generates two model-bound vectors, commits them
  through the unified transaction authority, embeds a query, returns the exact
  matching document, and proves the runtime cursor does not advance during the
  query operation.
- The reviewed public security schema advances the OpenAPI digest to
  `98873117404c51a862e5811b51a372f99636bbfbf2cde2ee1db51bab31ca45d3`.
  Generated outward HTTP/MCP/CLI/SDK bindings remain owned by G06.
