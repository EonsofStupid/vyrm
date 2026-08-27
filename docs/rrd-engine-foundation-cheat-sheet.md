# RRD engine and RRFlow runtime foundation cheat sheet

Status: authoritative pre-release architecture and implementation guardrail.
This document freezes the provider-neutral runtime boundary that implementation
work must satisfy. It is not a claim that the current repository already
satisfies the contract.

This decision supersedes older repository text that presented the retired
pre-release identity as a permanent product or independently branded storage
subsystem. Active source must use the RRFlow/RRD contract below.

Canonical platform resource and topology terms are defined only in
[`platform/README.md`](platform/README.md). This document describes engine and
lifecycle architecture and must not create alternate meanings for those terms.

The complete product/data boundary is authoritative in
[`rrflow-rrd-architecture.md`](rrflow-rrd-architecture.md). The in-place V1
identity cutover and durable-format names are tracked in
[`rrflow-rename-ledger.md`](rrflow-rename-ledger.md).

## Product boundary and naming

**RRFlow means Reason Ready Flow and is the whole product. RRD means Reason
Ready Daemon and is RRFlow's one native engine/runtime.** RRD is the native
engine and actual Fjall competitor. Storage, query,
indexing, reasoning state, lifecycle enforcement, and diagnostics compose
through that engine; they are not separate products or independently persisted
verticals.

RRFlow remains one cohesive system:

```text
RRFlow
├─ RRD: Reason Ready Daemon
│  ├─ storage: WAL, MVCC, LSM, columnar, snapshots, recovery
│  ├─ query: RRFlowQL, Arrow batches, DataFusion planning/execution
│  ├─ indexes: record, relation, graph, scalar, vector, temporal, geo
│  ├─ knowledge: project, operator, provenance, recall, reasoning state
│  ├─ lifecycle: attunement, preflight, authorization, observation, verification
│  ├─ service: embedded, local-daemon, remote, and administrative faces
│  └─ observability: authoritative replay plus derived traces and diagnostics
├─ rrflow: project-facing CLI and command boundary
├─ adapters and SDKs: provider/runtime/language integration surfaces
└─ Connectome: local and enterprise operator client
```

This is the required architecture, not a blanket completion claim. The current
query path converts one stamped authoritative snapshot to typed Arrow batches
and runs bounded relational operations through DataFusion 55. It still uses a
materialized `MemTable`; custom streaming providers, pushdown, spill governance,
and broader physical optimization remain open. Reference row semantics remain
the differential oracle.

Use `RRFlow` in all new product, protocol, CLI, configuration, documentation,
and module names. Use `RRD` only for the Reason Ready Daemon role and its public
service contract. Do not use RRD as a second product brand.

Active packages and public surfaces use RRFlow/RRD names. Because this is a
`0.1.0` alpha identity cutover, the retired identity has no alias, forwarding crate,
dual reader, deprecated command, environment fallback, or source allowlist.
Checked-in V1 fixtures use canonical RRD bytes. Future compatibility begins
only after a released version creates a real contract to preserve.

### Cohesion invariant

RRD must expose one logical engine contract over all supported models. It has:

- one authoritative catalogue for schemas, collections/tables, relations,
  indexes, vectors, temporal/geo models, reasoning state, and lifecycle data;
- one transaction coordinator and one snapshot/read-stamp model for operations
  that span those capabilities;
- one query and execution plane, with specialized physical operators behind
  the shared logical contract rather than separate user-facing engines;
- one event/outbox boundary for durable mutations, live-query publication,
  projection invalidation, index maintenance, audit, and reasoning events;
- one security authority and policy-decision path;
- identical logical behavior through embedded, local-daemon, and remote faces;
- one Connectome control/diagnostic model derived from authoritative RRD state.

Specialized LSM, columnar, graph, vector, and inference components may have
their own physical data structures. They may not create competing catalogues,
transaction truth, identity systems, authorization rules, or product
lifecycle state. An adapter, SDK, UI, or provider hook never owns authoritative
RRFlow state.

### Pre-release rename rule

Because the product has not reached a stable release, the RRFlow `0.1.0` alpha
identity is corrected in place. The repository must reject every active occurrence of the retired
identity, including serialized markers and fixtures. Git history is the only
rollback mechanism for the rename. No compatibility path survives in runtime
code.

`rrflow-mcp` is a correctly RRFlow-owned interoperability surface. It remains
an adapter projection of RRD's capability catalogue and never becomes a second
runtime or authority.

## Non-negotiable architecture

Provider hooks are adapters, not the foundation.

RRFlow starts without assuming Claude, Codex, Gemini, Copilot, MCP, or a local
model. It discovers the project, derives and persists an attunement profile,
materializes a fresh project topology, opens one exact runtime read state, and
produces one preflight receipt. It then requires a structured architecture
assessment and planning permit before it accepts a plan or authorizes a
mutation. Every provider adapter translates its native lifecycle into the same
canonical events and decisions. Bun, pnpm, npm, Cargo, Python, Go, and future
runners are project capabilities discovered during attunement; they are not
separate RRFlow implementations.

**Clyffy is the enforced agent behavior and harness identity; RRFlow/RRD is its
durable runtime authority.** Clyffy does not introduce a second event log,
project model, policy engine, or tool dispatcher. A Claude, Codex, Grok,
Gemini, Copilot, or local-model adapter becomes Clyffy by proving that it binds
the provider to the RRFlow lifecycle below. Provider and harness are separate
axes: for example, an xAI model can be fully orchestrated by a Clyffy-owned
tool loop even when a particular Grok CLI has no trustworthy blocking hooks.

```text
project evidence
  → materialized project topology and attunement profile
  → exact preflight receipt
  → task classification and architecture assessment
  → golden-pattern selection
  → planning permit and bounded context packet
  → recorded plan
  → reasoning attempt
  → one-shot tool authorization
  → execution observation
  → invalidation/refresh
  → verification
  → outcome
```

No provider adapter owns policy, persistence, recall, or verification. It only
normalizes input, calls the shared RRFlow runtime, and translates the shared
decision back into the provider's native response.

## Canonical lifecycle

Events are durable state transitions at the AI/application boundary. They are
not arbitrary logs and are not OpenTelemetry spans.

```text
session.opened
  → project.attunement.requested
  → project.attunement.completed | project.attunement.failed
  → turn.opened
  → prompt.received
  → preflight.started
  → preflight.completed | preflight.denied
  → task.classification.completed
  → architecture.assessment.requested
  → architecture.assessment.completed
      | architecture.decision.required
      | architecture.assessment.denied
  → pattern.selection.completed
  → planning.authorized | planning.denied
  → context.assembled
  → plan.recorded
  → tool.proposed
  → tool.authorized | tool.denied
  → tool.started
  → tool.completed | tool.failed
  → project.state.invalidated
  → projection.refresh.completed | projection.refresh.failed
  → verification.completed
  → reasoning.outcome.recorded
  → turn.closed
  → session.compaction.started
  → session.compaction.completed
  → session.closed
```

Use past-tense event names for facts that have happened. Requests and commands
are separate typed inputs; they do not masquerade as completed events.

This lifecycle governs observable agent behavior, not private model
chain-of-thought. RRFlow never asks a provider to expose hidden reasoning. It
proves that the model received the current project evidence before its first
planning response, that a structured architecture assessment was accepted,
and that no persistent plan or mutating tool crossed the boundary without the
resulting permit.

### Required event envelope

Every authoritative event carries:

```text
spec_version             event_id
event_type               occurred_at
recorded_at              instance_id
project_id               member_id?
session_id               turn_id?
reasoning_run_id?        attempt_id?
tool_call_id?            correlation_id
causation_id?            actor
adapter_kind             adapter_version
enforcement_level        scope
payload_digest           previous_state_digest?
read_stamp?              trace_id
span_id                  parent_span_id?
```

Payloads are strict, versioned, size-bounded, and deny unknown fields at the
control boundary. Unknown adapter payloads, event revisions, commands, or
mutation classifications fail closed. Observational readers may preserve an
unknown event as opaque evidence, but may not authorize work from it.

### Authoritative events versus telemetry

RRFlow events are hash-chained, replayable product state. OpenTelemetry is a
derived export for diagnostics and third-party visualization. OTel spans model
timed operations with parentage, links, attributes, events, and status; they
must never become the source of authorization or recovery truth. See the
[OpenTelemetry tracing API](https://opentelemetry.io/docs/specs/otel/trace/api/).

Logical decisions, denials, state transitions, read stamps, and verification
receipts are never sampled. High-volume physical diagnostics may be sampled or
aggregated without changing product truth.

RRFlow deliberately keeps three event planes separate:

1. **Authoritative lifecycle events** are durable, unsampled, hash-chained RRD
   state used for authorization, replay, audit, and recovery.
2. **Diagnostic telemetry** is a derived OpenTelemetry/metrics projection used
   for latency, resource, index, token, and bottleneck analysis. It may be
   sampled and rebuilt.
3. **Teaching signals** are privacy-bounded local UI observations such as a
   request for an explanation, a tooltip expansion, or a dismissed hint. They
   may tune deterministic progressive disclosure, but never change policy or
   become evidence that a task was authorized or verified.

## Mandatory pre-planning gate

The gate runs for every code-change prompt before provider reasoning begins
when the adapter has a blocking prompt hook or RRFlow owns orchestration. Its
command form is also manually available so an operator or AI can refresh and
inspect the same evidence without starting a model turn:

```text
rrflow attune --refresh
rrflow topology show --format tree|json
rrflow topology explain <path-or-member>
rrflow plan assess --input <architecture-assessment.json>
```

The first three commands are deterministic reads/materialization. `plan
assess` validates and persists the architecture decision; it does not create
files or scaffold code.

### Required architecture assessment

Before a plan is accepted, the agent answers these evidence-backed questions:

1. Which existing capability and boundary own this behavior today?
2. Should the change extend that boundary or create a new one, and what
   repository evidence makes that necessary?
3. What is the public contract and permitted dependency direction?
4. Which golden pattern or composed pattern facets apply, and which candidate
   patterns were rejected?
5. What persistence, transaction, security, audit, realtime, recovery, and
   observability implications cross the boundary?
6. Which existing files/modules should change, what new paths are justified,
   and how will oversized or cyclic ownership be prevented?
7. What exact verification commands and platform matrix prove the change?

The assessment has a typed answer for every question. `unknown` is valid only
with the missing evidence and the bounded discovery action needed to resolve
it. Empty prose, copied boilerplate, and unsupported certainty are denied.

### `PlanningPermit`

RRD emits a short-lived, single-turn planning permit only after the topology,
preflight, architecture assessment, and pattern selection agree. It binds:

- project, workspace member, session, turn, and reasoning-run identities;
- prompt/task digest and task/risk classification;
- project-topology, project-profile, source-tree, routing, policy, and RRD read
  stamps/digests;
- selected golden-pattern IDs, revisions, evidence sources, and composed
  facets;
- architecture-answer digest, chosen owner/boundary, dependency direction,
  expected paths, and validation matrix;
- adapter identity plus separately proven pre-planning and mutation coverage;
- issuance, expiration, one-shot scope, and permit digest.

A changed prompt, tree, manifest, lockfile, workspace definition, CI file,
policy, route generation, pattern revision, or read stamp invalidates the
permit. File writes, patches, shell mutations, generated-code changes, and
external mutations fail closed until the permit is refreshed. A provider with
no pre-prompt interception can still prove mutation enforcement through the
command proxy, but it must report `planning_enforced = false`; it cannot claim
full Clyffy readiness.

## Enforcement levels

Every installed adapter declares and proves one enforcement level:

| Level | Guarantee | Mutation authority |
|---|---|---|
| `intercepting` | Runtime exposes a blocking pre-tool decision point | Yes |
| `orchestrated` | RRFlow-owned application dispatches every tool | Yes |
| `proxied` | Commands execute through `rrflow exec` | Yes |
| `cooperative` | Model may voluntarily invoke RRFlow/MCP tools | No |
| `observe_only` | Runtime activity can be recorded but not blocked | No |

An installation may combine adapters, but at least one `intercepting`,
`orchestrated`, or `proxied` path must cover every mutating capability before
RRFlow reports enforcement as ready. Hookless local runtimes must use the
command proxy or remain cooperative/observe-only.

The level is accompanied by a tested coverage vector, not one optimistic
boolean: session start/resume, pre-prompt, pre-tool by tool class, post-tool,
failure, compaction, stop/session end, subagent, hosted-tool, and external
mutation coverage; plus timeout, crash, invalid-output, and trust failure modes.
RRFlow reports `planning_enforced` and `mutation_enforced` separately and lists
every uncovered path. A provider/version change expires that evidence until its
conformance fixture runs again.

MCP exposes model-controlled `tools/call`; tool annotations are hints and are
not trustworthy authorization evidence. MCP therefore supplies interoperability
and cooperative context/tool access, not universal interception. See the
[MCP tool schema](https://modelcontextprotocol.io/specification/2025-06-18/schema).

## Attunement contract

Attunement is evidence-based project discovery performed before provider
binding and before preflight.

### Inputs

- repository and workspace roots;
- manifests and lockfiles;
- workspace membership and dependency topology;
- declared scripts, aliases, build targets, and task-runner configuration;
- CI workflows, required platforms, and verification commands;
- RRFlow instance/member manifest and policy;
- source revision, dirty-state digest, and relevant environment/tool versions;
- installed provider/local runtime capabilities.

Attunement parses files and machine-readable metadata without executing
project-owned lifecycle scripts. Safe metadata commands run with explicit
argv, time, output, and environment bounds; their results are content-addressed
evidence. Dependency installation, build scripts, generated project code, and
arbitrary configure hooks are side effects, not discovery mechanisms.

### Output: `ProjectTopology`

The latest file tree is a materialized, queryable project model, not a wall of
text pasted into the model context. Every topology node records its canonical
relative path, kind, language, owning workspace/member, package/module/target,
generated/vendor status, content or metadata digest, and relevant policy
labels. Edges describe membership, containment, dependency, generation,
ownership, test coverage, CI execution, deployment, and public-contract
relationships.

The topology also records:

- repository/workspace roots and nested members;
- packages, modules, libraries, binaries, applications, services, plugins,
  tests, examples, benches, generated outputs, and deployment units;
- scripts/tasks and their exact runner, working directory, dependencies,
  inputs, outputs, mutation class, and platform constraints;
- CI jobs, target operating systems/architectures, required gates, and release
  artifact relationships;
- current architecture boundaries inferred from explicit manifests and
  dependency graphs, with confidence and provenance rather than silent guesses;
- excluded, ignored, generated, vendored, binary, and oversized paths so the
  agent knows what it did not inspect.

`rrflow attune --refresh` and the session/prompt hooks invoke the same
materializer. Rendering as a tree is a view over this model; no separate
manually maintained file-tree document becomes authority. Incremental refresh
is allowed only when its digest equals a clean full rebuild in differential
tests.

### Output: `ProjectProfile`

The persisted, versioned profile contains:

- all repository/workspace members and their canonical identities;
- every discovered ecosystem and runner without destructive precedence;
- exact executable plus argv templates, working directories, and argument
  policies;
- task classes such as format, lint, check, test, build, run, package, deploy;
- dependency and affected-target graph;
- mutation/read-only classification;
- required projections, maximum allowed lag, and verification rules;
- CI platform matrix and runner versions;
- source fingerprints and one deterministic profile digest.

Profile freshness is checked at session start, prompt preflight, and immediately
before a mutating authorization. A changed manifest, lockfile, workspace file,
CI workflow, policy, relevant tool version, or repository membership invalidates
the profile and requires re-attunement.

### Ecosystem discovery rules

- Cargo uses `cargo metadata --format-version=1` for machine-readable packages,
  members, targets, and dependencies. The format admits additive fields, so
  parsers must tolerate unknown observational fields. See
  [Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html).
- pnpm parses `pnpm-workspace.yaml`, `package.json`, scripts, catalogues, and
  filters. Recursive, sequential, parallel, pre/post, and execution-summary
  behavior must remain explicit. See [pnpm run](https://pnpm.io/cli/run) and
  [pnpm workspaces](https://pnpm.io/workspaces).
- Bun parses `package.json`, workspaces, `bun.lock`, `bunfig.toml`, lifecycle
  configuration, and `trustedDependencies`. Discovery never enables blocked
  dependency scripts. See [Bun lifecycle scripts](https://bun.sh/docs/pm/lifecycle)
  and [Bun workspaces](https://bun.sh/docs/pm/workspaces).
- npm parses `package.json` and models `pre<script>`, `<script>`, and
  `post<script>` as one composite workflow with separate child observations.
  See [npm scripts](https://docs.npmjs.com/cli/using-npm/scripts/).
- Python/uv parses `pyproject.toml`, build-system metadata, package layout,
  dependency groups, and `uv.lock`; uv workspace members remain independent
  projects joined by one lockfile. See
  [uv workspaces](https://docs.astral.sh/uv/concepts/projects/workspaces/) and
  [uv project layouts](https://docs.astral.sh/uv/concepts/projects/init/).
- Go parses every `go.mod`/`go.sum` and any applicable `go.work` without
  assuming the workspace file belongs in version control. See
  [Go modules and workspaces](https://go.dev/ref/mod).
- Gradle and Maven retain projects/modules, included build logic, task graphs,
  and reactor order. Natural project boundaries and shared convention plugins
  are explicit rather than inferred from directory names. See
  [Gradle multi-project builds](https://docs.gradle.org/current/userguide/multi_project_builds.html),
  [Gradle build-structure practices](https://docs.gradle.org/current/userguide/best_practices_structuring_builds.html),
  and the [Maven reactor guide](https://maven.apache.org/guides/mini/guide-multiple-modules).
- .NET parses solution/project references, targets, frameworks, test projects,
  and installed template metadata. Item, project, and solution templates stay
  distinct. See [.NET templates](https://learn.microsoft.com/en-us/dotnet/core/tools/custom-templates).
- CMake consumes the versioned file API codemodel for projects, directories,
  targets, sources, toolchains, and dependency edges. It does not parse
  generated build files as prose. See the
  [CMake file API](https://cmake.org/cmake/help/latest/manual/cmake-file-api.7.html).
- Mixed repositories retain every capability. A Bun lockfile does not erase
  Node, pnpm, Cargo, Python, Go, or nested workspace evidence.

## Golden pattern registry

Golden patterns are versioned, source-linked structural contracts. They are
not copied repositories, prompt snippets, or hardcoded conditionals in an
adapter. RRD owns one registry; Clyffy selects from it using the task,
`ProjectTopology`, deployment target, risk, and existing dependency direction.

Every supported stack family offers exactly three first-class structural
choices:

1. **Bounded component/package** — extend an existing owner with the smallest
   public surface and colocated tests.
2. **Modular application/service** — create or extend one independently
   testable application/service boundary with an explicit contract and private
   implementation.
3. **Workspace/platform/plugin** — introduce an independently governed member,
   plugin, build-logic unit, deployment unit, or cross-project capability only
   when lifecycle and ownership evidence justify it.

Those are decision alternatives, not a preference for more directories. The
default is the smallest existing boundary that can own the behavior without a
dependency cycle or responsibility violation. Creating a new crate, package,
service, plugin, or repository requires an explicit `new_boundary` decision.

### Required pattern data

Each pattern revision contains:

- stack/runtime/framework applicability and incompatible combinations;
- one canonical topology constraint set and permitted dependency directions;
- naming, public API, internal-module, configuration, generated-code, and
  ownership rules;
- minimum unit/integration/contract/e2e tests and affected-target selection;
- security, secrets, audit, observability, persistence, recovery, CI,
  packaging, deployment, and documentation requirements when applicable;
- safe creation/update actions, dry-run output, rollback boundary, and
  expected file diff;
- primary sources, reviewed reference implementations, supported tool
  versions, last-audited date, confidence, and deprecation/replacement data;
- counterexamples that explain when the pattern must not be selected.

Pattern execution follows a choose → configure → review → dry-run → apply →
verify flow. Backstage's schema-driven Software Templates, review step, task
history, and dry-run support are useful interaction references, but RRFlow
keeps selection and authorization in RRD rather than adopting Backstage as an
authority. See [Backstage Software Templates](https://backstage.io/docs/features/software-templates/)
and [dry-run testing](https://backstage.io/docs/features/software-templates/dry-run-testing/).

### Initial stack packs

The first registry qualification covers the stacks already present in the
operator estate, then expands through the same plugin contract:

| Stack family | Bounded component/package | Modular application/service | Workspace/platform/plugin |
|---|---|---|---|
| Rust/Cargo | `rust/component`: library or internal module | `rust/application`: binary/service with library-owned logic | `rust/workspace`: workspace member, Tauri/embedded boundary, or shared platform crate |
| TypeScript/JavaScript | `typescript/component`: package, feature, or UI component | `typescript/application`: Vite/TanStack client, server, worker, or full-stack app | `typescript/workspace`: Bun/pnpm/npm workspace package, plugin, or shared build/config unit |
| Python/uv | `python/component`: `src` package/module | `python/application`: API, worker, CLI, or ML/data application | `python/workspace`: uv workspace member, shared library, or pipeline plugin |
| Go | `go/component`: package within one module | `go/application`: service, CLI, or edge agent | `go/workspace`: separate module/workspace member or generated client boundary |
| JVM/Gradle/Maven | `jvm/component`: library/module | `jvm/application`: service, worker, or application | `jvm/workspace`: multi-project member, convention plugin, or reactor/platform module |
| .NET | `dotnet/component`: library or feature project | `dotnet/application`: ASP.NET/worker/desktop application | `dotnet/workspace`: solution member, shared build package, or extensibility module |
| C/C++/CMake | `cpp/component`: library/target | `cpp/application`: executable, daemon, or embedded application | `cpp/workspace`: multi-target project, toolchain/platform layer, or plugin boundary |

Polyglot projects compose facets instead of multiplying templates. A
Rust/Tauri + TypeScript/React desktop client, for example, combines one desktop
application boundary, one Rust native-command boundary, and one frontend
feature/package boundary under a single architecture assessment. Cross-language
FFI/IPC/protocol ownership is an explicit edge in the topology.

No pattern is labelled “top,” “modern,” or “enterprise” from popularity alone.
Qualification requires current primary documentation, at least one maintained
reference implementation, deterministic fixtures, a dry-run diff, and a
scheduled drift audit. Official examples such as Cargo's package targets,
TanStack Start's project structures, and native `.NET` templates are evidence
inputs, not code to absorb wholesale. See [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html),
[TanStack Start examples](https://tanstack.com/start/latest/docs/framework/react/getting-started),
and [.NET project templates](https://learn.microsoft.com/en-us/dotnet/core/tutorials/cli-templates-create-project-template).

## Preflight contract

Preflight is a deterministic, persisted operation, not a text-printing helper.

1. Resolve the instance, project, member, session, and turn.
2. Load the persisted `ProjectTopology` and `ProjectProfile` and verify every
   source fingerprint.
3. Re-attune or deny when required evidence is stale or unavailable.
4. Establish source-graph and index freshness.
5. Open one exact RRFlow read stamp/snapshot.
6. Classify the task and select relevant topology/profile graph nodes.
7. Select and validate candidate golden patterns and required architecture
   questions.
8. Route through source, decision, failure, claim, vector, and external
   knowledge edges under explicit budgets.
9. Assemble a bounded context packet with provenance and exclusion evidence.
10. Persist a `PreflightReceipt` binding topology/profile/pattern digests,
    repository state, read stamp, projection generations, selected evidence,
    token budget, and expiration.
11. Return provider-neutral structured context plus a rendered adapter view;
    the later accepted architecture assessment produces `PlanningPermit`.

Preflight recall is task-specific. It must not inject every known subject by
default. Empty, truncated, stale, or fallback results are explicit outcomes.

## Command and tool boundary

The cross-platform RRFlow binary owns one command path:

```text
rrflow attune
rrflow topology show|explain
rrflow preflight
rrflow plan assess
rrflow authorize
rrflow exec -- <exact argv>
rrflow observe
rrflow verify
rrflow adapter install <provider>
rrflow adapter audit <provider>
```

`rrflow exec` accepts an argv vector, not an unparsed shell program. Shell
composition, quoting, redirection, expansion, pipelines, and platform-specific
wrappers require an explicitly declared execution policy rather than string
splitting.

Before spawn, authorization binds:

- one active reasoning attempt;
- one unexpired planning permit;
- one unexpired preflight receipt;
- exact executable, argv, working directory, and selected environment digest;
- instance/project/member and repository revision;
- profile, workflow, read-stamp, and projection identities;
- mutation class, required approval, timeout, and verification policy.

Authorization is one-shot and idempotently consumed. After execution, RRFlow
records exit/signal status, duration, bounded stdout/stderr identities, changed
paths, resource evidence, and adapter evidence; invalidates affected knowledge
and indexes; refreshes required projections; runs verification; and advances
the reasoning state. Another mutation cannot reuse the same attempt or receipt.

## Provider adapters

Adapters implement one internal interface:

```text
identify runtime/version/capabilities
normalize native input → canonical request/event
call shared RRFlow lifecycle operation
translate canonical decision → native response
emit adapter audit/conformance evidence
```

They contain no independent recall, policy, workflow, or persistence logic.

- Claude supports session, prompt, pre/post-tool, failure, file-change,
  compaction, and session-end hooks; `PreToolUse` can block. See
  [Claude Code hooks](https://code.claude.com/docs/en/hooks).
- Gemini exposes agent/model/tool-selection/tool/session/compaction hooks and
  blocking decisions. See [Gemini hooks](https://geminicli.com/docs/hooks/reference/).
- Copilot exposes pre/post-tool, permission, stop, and error hooks. See
  [Copilot hooks](https://docs.github.com/en/copilot/reference/hooks-reference).
- Codex now exposes stable `SessionStart`, `UserPromptSubmit`, `PreToolUse`,
  `PostToolUse`, compaction, stop, and session lifecycle hooks.
  `UserPromptSubmit` can inject context or block before the prompt is processed;
  `PreToolUse` can block or rewrite Bash, `apply_patch`, MCP, and most local
  function calls. Its own documentation says specialized and hosted paths may
  opt out, so the adapter must prove actual coverage instead of treating hook
  availability as universal enforcement. See the
  [official Codex hooks reference](https://developers.openai.com/codex/hooks).
- OpenAI Responses exposes typed function/MCP/shell calls, constrained tool
  selection, and streaming tool events; an RRFlow-owned orchestrator performs
  authorization before returning tool output. See the
  [official OpenAI Responses reference](https://developers.openai.com/api/reference/cli/resources/responses/methods/create).
- MCP exposes the shared cooperative tools and structured results but does not
  claim interception of host-owned tools.
- Grok/xAI and other provider APIs can be `orchestrated` when Clyffy owns their
  entire tool loop. A standalone CLI with no verified prompt/tool interception
  remains `proxied`, `cooperative`, or `observe_only` regardless of the model
  provider behind it.
- Local runtimes without a trustworthy hook or orchestrator use the command
  proxy and receive only the enforcement level actually proven.

## Reasoning and authorization invariant

The canonical reasoning state is:

```text
goal → plan → attempt → observation → decision → verification → outcome
```

One declared attempt authorizes at most one mutating tool execution. A real
tool result produces the observation. Verification is separately typed and
cannot be fabricated from a declared intention. Failed or missing verification
cannot transition to a successful outcome. Adapter shutdown, compaction,
restart, or retry does not bypass these transitions.

## Current implementation gaps

The following are defects to refactor, not target behavior:

- `crates/rrd-engine/src/runtime/attunement.rs` has a real persisted,
  content-addressed `ProjectAttunementReceipt` and one-outstanding-tool binding,
  but it does not materialize the required `ProjectTopology`, architecture
  assessment, pattern selection, or `PlanningPermit`.
- `crates/rrd-engine/src/runtime/hook.rs` follows Claude-shaped fields and lets
  unknown shapes degrade to no action.
- `crates/rrd-engine/src/runtime/init.rs` writes only `.claude/settings.json` and
  its post-tool coverage is incomplete.
- `crates/rrd-engine/src/runtime/stack.rs` relies on marker/prefix heuristics,
  silently ignores unreadable manifests, and lets Bun replace Node evidence.
- `crates/rrd-engine/src/runtime/workflow.rs` governs npm-compatible commands
  while Cargo and other runners are only observed.
- `crates/rrd-engine/src/runtime/preflight.rs` recalls all subjects rather than
  a task-specific project capability/knowledge graph cut.
- `crates/rrd-engine/src/runtime/registry/codex-cli.toml` incorrectly declares
  current Codex hook support absent, and `gemini-cli.toml` incorrectly declares
  the current Gemini CLI retired. Registry claims require a source/versioned
  drift audit before they can authorize installation.
- `crates/rrflow-mcp` asks the model to call lifecycle tools voluntarily and must
  therefore report cooperative, not intercepting, enforcement.
- No canonical provider-neutral lifecycle state machine, complete
  `ProjectTopology`/`ProjectProfile`, `PreflightReceipt`, `PlanningPermit`,
  pattern registry, adapter conformance declaration, or command proxy is yet
  complete.

## Target internal organization

These are module boundaries inside one RRFlow runtime, not services:

```text
crates/rrd-engine/src/runtime/
├─ attunement/
│  ├─ profile.rs
│  ├─ topology.rs
│  ├─ discovery.rs
│  ├─ fingerprint.rs
│  ├─ cargo.rs
│  ├─ javascript.rs
│  ├─ python.rs
│  ├─ go.rs
│  ├─ jvm.rs
│  ├─ dotnet.rs
│  ├─ cmake.rs
│  └─ ci.rs
├─ lifecycle/
│  ├─ event.rs
│  ├─ envelope.rs
│  ├─ state_machine.rs
│  └─ decision.rs
├─ planning/
│  ├─ task.rs
│  ├─ assessment.rs
│  ├─ permit.rs
│  ├─ pattern.rs
│  └─ registry.rs
├─ execution/
│  ├─ preflight.rs
│  ├─ authorization.rs
│  ├─ command_proxy.rs
│  ├─ observation.rs
│  └─ verification.rs
└─ adapters/
   ├─ claude.rs
   ├─ gemini.rs
   ├─ copilot.rs
   ├─ openai.rs
   ├─ mcp.rs
   └─ command_proxy.rs
```

Do not create this tree through a single bulk move. Freeze contracts and tests,
extract one responsibility at a time inside the existing RRD engine, and do
not create a new runtime crate, store, catalogue, or event authority. Any
temporary forwarding path is explicit and removed after its callers migrate.

## Conformance and release gates

The runtime foundation is not complete until all of these pass:

1. Identical canonical event fixtures for every provider adapter.
2. Unknown or malformed lifecycle, assessment, pattern, and mutating inputs
   fail closed.
3. Cargo, Bun, pnpm, npm, uv, Go, JVM, .NET, CMake, and mixed-workspace
   fixtures produce deterministic topology/profile output and retain every
   capability.
4. A clean full topology build and every valid incremental refresh produce the
   same digest and graph.
5. Manifest/lockfile/workspace/CI/policy/pattern drift invalidates preflight,
   planning permit, and authorization.
6. Architecture assessment deterministically selects or rejects a golden
   pattern and cannot silently invent a new boundary.
7. No plan persistence or mutating tool is accepted without one matching,
   unexpired planning permit.
8. One planning permit, preflight receipt, and reasoning attempt authorize
   exactly one matching mutation.
9. Retry/restart cannot double-execute or reuse authorization.
10. Pre/post/failure events survive process reopen and preserve causal order.
11. Cooperative MCP/local adapters never report enforced planning or mutation
    coverage.
12. Native-hook, orchestrated, and proxied fixtures prove their actual tool
    coverage and expose uncovered paths.
13. Unix and Windows argv/environment/working-directory behavior is proven.
14. One end-to-end fixture runs a real Cargo command and one real JavaScript
    package command through the same command proxy and canonical lifecycle.
15. Embedded and remote RRFlow paths persist identical logical events.
16. Formatting, unit/integration/property tests, clippy with warnings denied,
    and required Linux, macOS/ARM, and Windows CI jobs all pass.

No branch or feature is described as verified until the required remote matrix
is green. Local results are fast feedback for the host platform, not proof of
other targets.

### Minimum persistent foundation scenarios

All scenarios close and reopen RRD before their final assertion and must pass
100%; in-memory-only success is not completion:

1. new session materializes topology before prompt processing;
2. resumed and post-compaction sessions refresh stale topology;
3. manual topology refresh matches the automatic hook result;
4. mixed Rust/Bun/pnpm workspace retains every member and runner;
5. unreadable or contradictory manifest evidence denies planning;
6. existing-boundary assessment selects a bounded-component pattern;
7. unjustified new-boundary assessment is denied with evidence;
8. justified polyglot boundary composes two pattern facets without creating a
   second authority;
9. file/manifest/CI drift invalidates an issued planning permit;
10. patch or shell mutation without a planning permit is denied;
11. exact permitted mutation records observation, invalidation, verification,
    and terminal outcome once;
12. retry, daemon restart, or provider reconnection cannot reuse the permit or
    tool authorization;
13. Claude, Codex, and one Clyffy-owned orchestrator emit equivalent canonical
    events for the same fixture;
14. MCP-only and uncovered native tool paths report incomplete enforcement and
    cannot claim Clyffy-ready status.

## First executable slice: forced planning boundary

This is the first code slice after this research freeze. It stays inside the
existing `rrd-contract` plus `rrd-engine` contract/authority and does not add a
new crate, database, daemon, UI, provider policy, or compatibility version.

### Contract types

- `LifecycleEventEnvelopeV1` plus strict payload enums for the pre-planning
  sequence;
- `ProjectTopologyV1` and `ProjectProfileV1`;
- `TaskClassificationV1`, `ArchitectureAssessmentV1`, and
  `GoldenPatternSelectionV1`;
- `PreflightReceiptV1`, `PlanningPermitV1`, and `AdapterCoverageV1`;
- command/decision types for refresh, assess, allow, deny, and stale evidence.

These names describe the first product contract revision; they do not create a
`v2` compatibility path. During pre-release, corrections update the reviewed
V1 contract and its golden fixtures unless the product owner explicitly
authorizes another public version.

### Engine behavior

1. Persist `session.opened`, attunement, prompt, preflight, classification,
   architecture, pattern, planning, and context/plan events through RRD's
   existing transaction/control journal.
2. Materialize Rust/Cargo plus JavaScript/Bun/pnpm/npm topology from a
   disposable mixed-workspace fixture using machine-readable evidence.
3. Render the same topology through automatic session/prompt preflight and
   manual `rrflow topology show` without maintaining two implementations.
4. Validate the seven architecture answers and deterministically select one or
   more pattern facets.
5. Emit a sealed planning permit only from the accepted state.
6. Deny `apply_patch`, shell mutation, and a representative MCP mutation when
   the permit is absent, stale, mismatched, or already consumed.
7. Close and reopen RRD, replay the sequence, and recover the identical
   aggregate state and terminal decision.

### Slice acceptance

- golden JSON fixtures reject unknown fields and invalid transitions;
- the first ten persistent scenarios above pass at 100%, with scenarios 11–14
  landing as the command proxy and adapters are attached;
- full-vs-incremental topology differential passes;
- no source file, plan, or mutation changes before the permit in the end-to-end
  fixture;
- no `rrflow-*` public name or parallel runtime/store authority is introduced;
- format, targeted tests, workspace tests, clippy with warnings denied, and the
  required remote platform matrix pass before the slice is called complete.

## Implementation order

1. Freeze unrelated feature work and current public claims.
2. Define the neutral event envelope, decisions, state machine, and persistent
   reopen fixtures using the existing RRD transaction/control-journal authority.
3. Replace marker heuristics with persisted `ProjectTopology` and
   `ProjectProfile` materialization, starting with Rust plus JavaScript and
   mixed-workspace fixtures, then the other qualified stack adapters.
4. Implement the versioned golden-pattern registry, task classification,
   architecture assessment, and deterministic selection fixtures.
5. Implement `PreflightReceipt`, task-specific context assembly, and
   `PlanningPermit`.
6. Implement exact-argv authorization and the cross-platform command proxy;
   deny every mutation without the permit chain.
7. Extract existing Claude-shaped handling into the neutral adapter interface,
   then correct and prove Codex and Gemini from current source/version evidence.
8. Add Copilot, OpenAI/xAI orchestrators, MCP, and local-runtime adapters only
   against the same contract.
9. Prove adapter conformance and separate planning/mutation coverage reporting.
10. Bind the lifecycle to the unified RRFlow transaction, graph, query, and
    knowledge layers.
11. Add controlled prompt/evaluation traces only after the enforcement path is
    real and restart-safe.

No new adapter-specific feature may precede the neutral contract it consumes.

## Capability and planning-source invariants

- RRD owns one typed capability catalogue. Engine operations, RRD HTTP
  endpoints, MCP tools, CLI commands, SDK methods, and Connectome controls are
  projections of that catalogue, never independently handwritten inventories.
- Every surface disposition is explicit: available, experimental, planned, or
  not applicable. `Available` requires a real entrypoint and executable test.
- Conformance fails in both directions: an advertised entrypoint without an
  implementation fails, and an implemented capability without a declared
  surface disposition fails.
- Before code planning, RRFlow attunes against the actual repository tree,
  manifests, lockfiles, workspaces, CI, policy, and RRD read stamp. It persists
  a content-addressed receipt naming every source fingerprint used.
- A mutating authorization binds to that receipt, project root, routing
  generation, exact argv/tool input, working directory, environment digest,
  reasoning attempt, and read stamp. Tree or manifest drift makes the receipt
  stale and mutation fails closed until re-attunement.

## Pre-compaction continuation brief

### Objective

Build RRFlow as one cohesive, provider-neutral AI data/runtime system. RRD, the
Reason Ready Daemon, is the durable runtime authority within RRFlow. Connectome
visualizes and operates that authority;
it does not reimplement it. Provider hooks, MCP, SDKs, command runners, and
local/frontier models all bind to the same lifecycle and data contracts.

### Current truth

- The repository contains substantial working capabilities, but their current
  crate names and vertical organization do not prove that the cohesive target
  contract is complete.
- RRFlow is the product name; it is not permission to create a second engine
  beneath RRD.
- A partial persisted `ProjectAttunementReceipt` exists, but the
  provider-neutral state machine, materialized `ProjectTopology`, complete
  `ProjectProfile`, `PreflightReceipt`, `PlanningPermit`, exact-argv proxy,
  golden-pattern registry, and adapter conformance suite are not complete.
- Older statements that conflict with the RRFlow product/RRD engine split are
  historical only; this document is authoritative when those statements conflict.
- Maintenance/pruning work is postponed. It must not displace the cohesive
  engine and runtime foundation.

### Next implementation session

1. Audit the worktree and preserve unrelated or concurrent changes.
2. Enforce this document's naming and cohesion rules through the product-identity
   scan and dependency map.
3. Freeze canonical lifecycle types, state transitions, decisions, and reopen
   fixtures in RRD's existing contract/engine boundary.
4. Materialize deterministic `ProjectTopology`/`ProjectProfile` from actual
   machine-readable workspace evidence and prove the manual refresh/tree view.
5. Implement task classification, golden-pattern selection, the architecture
   assessment, `PreflightReceipt`, and `PlanningPermit`.
6. Deny all persistent planning and mutation until the permit chain is fresh;
   then implement the exact-argv one-shot command proxy.
7. Extract existing Claude-shaped behavior behind the shared adapter contract,
   correct the stale Codex/Gemini registry records, and add other adapters only
   against that contract.
8. Join lifecycle state to RRD's shared catalogue, transaction, snapshot,
   graph, query, index, audit, and recovery authority; do not create another
   sidecar store or parallel engine.
9. Prove embedded/local/remote equivalence, restart safety, fail-closed
   behavior, and the full required CI platform matrix before claiming the
   foundation verified.

### Explicit prohibitions

- Do not treat a provider, package manager, MCP server, UI, or physical storage
  backend as RRFlow's foundation.
- Do not build provider-specific policy or persistence.
- Do not split documents, graph, vector, reasoning, lifecycle, and audit into
  independently authoritative products.
- Do not use local CI success as proof of the remote platform matrix.
- Do not optimize blanket benchmarks before the shared contract and recovery
  invariants are real.
- Do not claim SurrealDB-, Qdrant-, Fjall-, or frontier-runtime superiority
  without controlled, reproducible, capability-appropriate evidence.

### Resume instruction

At the beginning of the next implementation session, read this document in
full, inspect the current worktree and recent history, and restate the first
dependency-critical slice before editing code. Treat the product/naming,
cohesion, lifecycle, enforcement, and release-gate sections as acceptance
criteria. Update this document when a decision changes; update the implementation
journal only when executable evidence lands.
