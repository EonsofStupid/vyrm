# RRFlow runtime foundation cheat sheet

Status: architecture and implementation guardrail. This document freezes the
provider-neutral runtime boundary that implementation work must satisfy. It is
not a claim that the current repository already satisfies the contract.

## Product boundary and naming

RRFlow is one cohesive product and runtime. Its internal layers are modules of
one system, not separately governed applications:

```text
RRFlow
├─ RRD: authoritative data and event runtime
├─ storage: WAL, MVCC, LSM, snapshots, recovery
├─ query: RRFlowQL, Arrow batches, DataFusion planning/execution
├─ indexes: record, relation, graph, scalar, vector, temporal, geo
├─ AI runtime: attunement, preflight, recall, authorization, observation
├─ rrflowd: embedded/remote transport and administration
└─ Connectome: local and enterprise operator client
```

Use `RRFlow` in all new product, protocol, CLI, configuration, documentation,
and module names. Existing `vyrm-*` package names and persisted identifiers are
legacy migration inputs. Do not perform a blind text replacement: crate names,
CLI/configuration compatibility, environment variables, protocol fields, and
on-disk format identifiers require an explicit migration matrix. A legacy
identifier may remain only where changing it would make existing data or
clients unreadable.

## Non-negotiable architecture

Provider hooks are adapters, not the foundation.

RRFlow starts without assuming Claude, Codex, Gemini, Copilot, MCP, or a local
model. It discovers the project, derives and persists an attunement profile,
opens one exact runtime read state, and produces one preflight receipt. Every
provider adapter translates its native lifecycle into the same canonical
events and decisions. Bun, pnpm, npm, Cargo, Python, Go, and future runners are
project capabilities discovered during attunement; they are not separate
RRFlow implementations.

```text
project evidence
  → attunement profile
  → exact preflight receipt
  → context packet
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
  → context.assembled
  → tool.proposed
  → tool.authorized | tool.denied
  → tool.started
  → tool.completed | tool.failed
  → project.state_invalidated
  → projection.refresh_completed | projection.refresh_failed
  → verification.completed
  → reasoning.outcome_recorded
  → turn.closed
  → session.compaction_started
  → session.compaction.completed
  → session.closed
```

Use past-tense event names for facts that have happened. Requests and commands
are separate typed inputs; they do not masquerade as completed events.

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
project-owned lifecycle scripts. Dependency installation and build scripts are
side effects, not discovery mechanisms.

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
- Mixed repositories retain every capability. A Bun lockfile does not erase
  Node, pnpm, Cargo, Python, Go, or nested workspace evidence.

## Preflight contract

Preflight is a deterministic, persisted operation, not a text-printing helper.

1. Resolve the instance, project, member, session, and turn.
2. Load the persisted `ProjectProfile` and verify its source fingerprints.
3. Re-attune or deny when required evidence is stale or unavailable.
4. Establish source-graph and index freshness.
5. Open one exact RRFlow read stamp/snapshot.
6. Classify the task and select relevant project/profile graph nodes.
7. Route through source, decision, failure, claim, vector, and external
   knowledge edges under explicit budgets.
8. Assemble a bounded context packet with provenance and exclusion evidence.
9. Persist a `PreflightReceipt` binding the profile digest, repository state,
   read stamp, projection generations, selected evidence, token budget, and
   expiration.
10. Return provider-neutral structured context plus a rendered adapter view.

Preflight recall is task-specific. It must not inject every known subject by
default. Empty, truncated, stale, or fallback results are explicit outcomes.

## Command and tool boundary

The cross-platform RRFlow binary owns one command path:

```text
rrflow attune
rrflow preflight
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
- OpenAI Responses exposes typed function/MCP/shell calls, constrained tool
  selection, and streaming tool events; an RRFlow-owned orchestrator performs
  authorization before returning tool output. See the
  [official OpenAI Responses reference](https://developers.openai.com/api/reference/cli/resources/responses/methods/create).
- MCP exposes the shared cooperative tools and structured results but does not
  claim interception of host-owned tools.
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

- `crates/vyrm-node/src/hook.rs` follows Claude-shaped fields and lets unknown
  shapes degrade to no action.
- `crates/vyrm-node/src/init.rs` writes only `.claude/settings.json` and its
  post-tool coverage is incomplete.
- `crates/vyrm-node/src/stack.rs` relies on marker/prefix heuristics and lets
  Bun replace Node evidence.
- `crates/vyrm-node/src/workflow.rs` governs npm-compatible commands while
  Cargo and other runners are only observed.
- `crates/vyrm-node/src/preflight.rs` recalls all subjects rather than a
  task-specific project capability/knowledge graph cut.
- `crates/vyrmd` asks the model to call lifecycle tools voluntarily and must
  therefore report cooperative, not intercepting, enforcement.
- No canonical provider-neutral event envelope, `ProjectProfile`,
  `PreflightReceipt`, adapter capability declaration, or command proxy is yet
  complete.

## Target internal organization

These are module boundaries inside one RRFlow runtime, not services:

```text
crates/rrflow-runtime/src/
├─ attunement/
│  ├─ profile.rs
│  ├─ discovery.rs
│  ├─ fingerprint.rs
│  ├─ cargo.rs
│  ├─ javascript.rs
│  └─ ci.rs
├─ lifecycle/
│  ├─ event.rs
│  ├─ envelope.rs
│  ├─ state_machine.rs
│  └─ decision.rs
├─ runtime/
│  ├─ preflight.rs
│  ├─ authorization.rs
│  ├─ execution.rs
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
extract one responsibility at a time, keep compatibility shims temporary and
explicit, and remove a shim only after its callers have migrated.

## Conformance and release gates

The runtime foundation is not complete until all of these pass:

1. Identical canonical event fixtures for every provider adapter.
2. Unknown or malformed mutating inputs fail closed.
3. Cargo, Bun, pnpm, npm, and mixed-workspace attunement fixtures produce
   deterministic profiles and retain every capability.
4. Manifest/lockfile/CI/policy drift invalidates preflight and authorization.
5. One preflight receipt plus one reasoning attempt authorizes exactly one
   matching mutation.
6. Retry/restart cannot double-execute or reuse authorization.
7. Pre/post/failure events survive process reopen and preserve causal order.
8. Cooperative MCP/local adapters never report enforced mutation coverage.
9. Unix and Windows argv/environment/working-directory behavior is proven.
10. One end-to-end fixture runs a real Cargo command and one real JavaScript
    package command through the same command proxy and canonical lifecycle.
11. Embedded and remote RRFlow paths persist identical logical events.
12. Formatting, unit/integration/property tests, clippy with warnings denied,
    and required Linux, macOS/ARM, and Windows CI jobs all pass.

No branch or feature is described as verified until the required remote matrix
is green. Local results are fast feedback for the host platform, not proof of
other targets.

## Implementation order

1. Freeze unrelated feature work and current public claims.
2. Define the neutral event envelope, decisions, state machine, and fixtures.
3. Implement persisted `ProjectProfile` attunement and freshness.
4. Implement `PreflightReceipt` and task-specific context assembly.
5. Implement exact-argv authorization and the cross-platform command proxy.
6. Extract the existing Claude behavior into an adapter.
7. Add Gemini, Copilot, OpenAI-orchestrator, MCP, and local-runtime adapters.
8. Prove adapter conformance and enforcement-level reporting.
9. Bind the lifecycle to the unified RRFlow transaction, graph, query, and
   knowledge layers.
10. Add controlled prompt/evaluation traces only after the enforcement path is
    real and restart-safe.

No new adapter-specific feature may precede the neutral contract it consumes.
