# Connectome and RRFlow diagnostics recovery plan

Status: authoritative recovery plan; no implementation-complete claim

Date: 2026-08-25

## Decision

Connectome is RRFlow's cross-platform desktop operator and developer client.
RRD, the Reason Ready Daemon, is the only authority for runtime events,
persistence, replay, security, capability discovery, and diagnostic
projections.

The React/Tauri application currently checked out at
`../connectome/apps/connectome` is the product foundation. The Rust crate at
`crates/connectome-ui` is a temporary diagnostics lab and migration source. It
must not survive as a second Connectome product or a second authority.

Purgato is RRFlow's intended developer-tooling source at the canonical
repository path `purgato/`. Its deep diagnostic workflows and presentation
ideas will be absorbed behind the same RRD diagnostic contract and Connectome
shell; Purgato will not become another runtime, store, event log, policy
engine, or operator client.

## Recovered evidence

### Actual Connectome application

- Local application: `../connectome/apps/connectome`
- Current remote: `https://github.com/EonsofStupid/surrealist.git`
- Current branch: `agent/connectome-rrflow-panel`
- Desktop stack: Tauri 2 Rust shell, React, TypeScript, Vite, Mantine, ArkType,
  Biome, TanStack Query, Sigma, Graphology, XYFlow, CodeMirror, and native
  updater/deep-link/filesystem/process plugins.
- Declared bundles: Windows NSIS, macOS app/DMG, Linux AppImage/DEB/RPM.
- Existing RRFlow work: native diagnostics transport, ArkType response
  validation, prompt-flight launch, timeline playback, reverse playback,
  freeze/seek controls, capability display, and RRFlow connection creation.
- Relevant local commits: `3ad6cafa`, `d481fb88`, `94e608a5`, `822cf789`, and
  `63f5d92d`.

The surrounding `../connectome` checkout is a separate, dirty SurrealDB/QORTEX
experiment. Its `apps/connectome` entry is a Git submodule. RRFlow itself has no
Connectome submodule today. The public repository currently named `connectome`
is explicitly marked as a retired wrong split, so repository naming must be
resolved before changing GitHub remotes.

### Temporary RRFlow diagnostics lab

`crates/connectome-ui` is a Rust `tiny_http` server with embedded vanilla
HTML/CSS/JavaScript. It contains useful tested projections and interaction
ideas:

- instance overview and readiness;
- persistent connection profiles;
- estates, tables, logical models, and capability surfaces;
- prompt-flight launch and weak/strong demonstrations;
- freeze, rewind, forward, speed, and event inspection;
- temporal stream and graph-at-cursor views;
- causal trace trees and bottleneck evidence;
- reasoning runs, claims, routes, query plans, vector artifacts, and cluster
  history.

It is not the desktop product. Its direct dependencies on physical RRD crates,
its embedded HTTP server, and its separately authored response types are
migration debt.

### Purgato discovery state

The canonical destination is now decided: `purgato/` at the RRFlow repository
root. During the 2026-08-25 inventory that directory was absent from the local
checkout, current Git objects, and the GitHub `rrflow` repository. The source
content or exact recovery commit still must be restored before a responsible
keep/adapt/retire audit. Do not scaffold a guessed replacement and do not
substitute an unrelated observability project.

## Non-negotiable ownership boundary

```text
AI runtimes and project runners
          |
          | provider-neutral adapters
          v
RRD lifecycle authority
  - canonical runtime event state machine
  - durable hash-chained event journal
  - policy and authorization decisions
  - replay, snapshots, comparisons, and read stamps
  - generated capability catalogue
          |
          | authenticated RRD client protocol
          v
Connectome desktop application
  - local and remote instance connections
  - operator and developer visualizations
  - replay controls and comparisons
  - guided explanations and remediation
  - no independent runtime truth
```

Connectome and Purgato-derived code may cache immutable responses for offline
viewing, but may not mint authoritative runtime events, infer authorization,
or silently repair missing history.

## Runtime event prerequisite

The current generic `RuntimeEvent`, durable trace events, prompt-flight events,
and Claude-shaped `HookEvent` do not constitute the required lifecycle.
Connectome migration depends on the provider-neutral lifecycle v1 already
specified in `docs/rrd-engine-foundation-cheat-sheet.md`:

```text
session.opened
project.attunement.requested
project.attunement.completed | project.attunement.failed
turn.opened
prompt.received
preflight.started
preflight.completed | preflight.denied
task.classification.completed
architecture.assessment.requested
architecture.assessment.completed | architecture.decision.required | architecture.assessment.denied
pattern.selection.completed
planning.authorized | planning.denied
context.assembled
plan.recorded
tool.proposed
tool.authorized | tool.denied
tool.started
tool.completed | tool.failed
project.state.invalidated
projection.refresh.completed | projection.refresh.failed
verification.completed
reasoning.outcome.recorded
turn.closed
session.compaction.started
session.compaction.completed
session.closed
```

RRD must validate transitions, persist the canonical envelope through its
existing control journal, reject unknown mutation events, and reproduce the
same aggregate after restart. OpenTelemetry remains a derived export, not the
state or replay source.

## Diagnostic contract v1

Connectome consumes one generated, authenticated RRD client contract. The
contract must expose:

1. readiness, instance identity, protocol version, and capability catalogue;
2. bounded diagnostic snapshots carrying one verified read stamp;
3. materialized project topology, topology differences, task classification,
   architecture answers, pattern selection, and planning-permit evidence;
4. run/session/turn indexes with explicit completeness and retention state;
5. ordered canonical event replay by cursor and by reasoning run;
6. live committed-event subscription with resumable cursors;
7. exact event inspection, causal ancestors/descendants, and payload evidence;
8. graph, schema, vector, query, storage, routing, reasoning, audit, estate,
   and cluster projections linked to the same event/read coordinates;
9. cursor-to-cursor state and graph differences;
10. persisted replay leases and honest truncation/retention disclosures;
11. explicit commands for controlled prompt experiments, never hidden UI-side
    mutations.

All schemas remain version 1 during pre-release unless the product owner
explicitly changes the version. RRFlow-named protocol strings and types are
renamed in place; no RRFlow compatibility protocol is retained by default.

## Connectome information architecture

Every complex diagnostic view has three progressive-disclosure layers over the
same source event. Switching layers never changes the selected cursor.

### Understand

Default layman view. It answers:

- What was the AI asked to do?
- What context did it find and omit?
- What decision did it make?
- What tools changed the project?
- Did verification pass?
- Where was time or context wasted?
- What action is recommended next?

Terms are translated without falsifying them. For example, `read_stamp`
becomes “the exact project state the AI saw,” while the raw value remains one
click away.

### Operate

Operator view adds instance health, estates, capabilities, permissions,
retention, projections, indexes, storage pressure, cluster state, live-query
health, and guided administrative actions.

### Inspect

Developer view preserves the full event envelope, payload, causation, hashes,
read stamps, query/physical plans, spans, index decisions, resource metrics,
raw audit evidence, and export controls.

## Schema-driven teaching viewport

Connectome uses an A2UI-inspired declarative surface protocol: RRD returns
strict, versioned diagnostic data and allowed action descriptors; the desktop
client maps them to a trusted local component catalogue. RRD never sends
arbitrary JavaScript, executable UI code, or styling authority. Connectome
retains accessibility, platform behavior, visual identity, permissions, and
action confirmation. This follows the useful security boundary in
[A2UI](https://a2ui.org/introduction/what-is-a2ui/) without making A2UI a
second RRFlow protocol or pinning RRFlow's version to an external candidate.

Every viewport element can carry canonical teaching metadata:

- industry term, plain-language label, concise explanation, and deeper
  reference;
- exact source event/schema field and evidence completeness;
- why the control is visible, what it affects, and its risk/permission class;
- progressive `Understand`, `Operate`, and `Inspect` renderings;
- deterministic tooltip, focus, keyboard, screen-reader, and error behavior.

Focus, hover, selection, and explanation requests first resolve from a local
canonical glossary and pattern registry with no model call. Small deterministic
rules or local ML may rank which explanation to surface and suppress repeated
hints. An LLM is reserved for genuinely ambiguous classification or a requested
custom explanation, must return a schema-constrained result, and is cached by
evidence digest. Teaching signals are local/consented diagnostic data and never
become authorization truth.

The viewport therefore teaches platform-aligned terminology passively while
remaining faithful: “read stamp” may render as “the exact project state the AI
saw,” but the canonical term, raw value, source, and completeness remain one
interaction away.

## Core visual system

One synchronized time cursor drives every panel:

- play, pause, single-step, rewind, fast-forward, jump-to-boundary, and speed;
- freeze one micro-event and expand its complete causal neighborhood;
- lane view for context, routing, reasoning/model, tools, storage/indexes,
  verification, and outcome;
- causal graph with parent, link, invalidation, and projection-refresh edges;
- state-before/state-after and graph-before/graph-after differences;
- latency, token, retry, cache, retrieval, and IO overlays;
- weak-versus-strong and baseline-versus-candidate prompt comparisons;
- synchronized plain-language explanation and raw diagnostic inspector;
- explicit gaps when RRD did not record sufficient evidence—never invented
  animations or inferred success.

Animations are projections of committed event timestamps and relationships.
They are not decorative simulations.

### NodeTrace projection

NodeTrace is the later Connectome lens that follows one prompt across provider,
Clyffy adapter, RRD preflight, project topology, knowledge routing, policy,
tool/command, external integration, observation, and verification boundaries.
For a request such as “access Infisical,” it shows only recorded hops: which
adapter received the prompt, what capability/policy resolved the request, what
credential boundary was consulted, what tool was authorized, what remote call
occurred, and how the result was verified. Every node links to a canonical
event or a derived span and states whether evidence is authoritative,
projected, sampled, redacted, or missing. NodeTrace never invents a traversal
to make the animation look complete.

## Keep, adapt, retire

| Source | Keep | Adapt | Retire after parity |
|---|---|---|---|
| Tauri Connectome | Desktop shell, packaging, updater, connection UX, tables, query editor, graph libraries, component system | Replace Surreal/RRFlow protocol assumptions with generated RRD v1 client and product terminology | Surreal cloud/product-specific flows that have no RRFlow role |
| `crates/connectome-ui` | Tested diagnostic semantics, projection coverage, replay behavior, prompt demos, UI acceptance fixtures | Move response construction behind RRD; port views into Tauri components | `tiny_http` product shell, direct physical-crate access, handwritten capability truth |
| RRD engine/contract/client | Diagnostic snapshot, read stamps, capability catalogue, authorization, audit, changefeed | Add canonical lifecycle events, run replay, live subscriptions, and missing diagnostic sections | Duplicate HookEvent/flight-specific authorities once adapters migrate |
| Purgato | Developer-tooling source once `purgato/` is recovered | Deep diagnostics enter RRD projections and Connectome Inspect/Operate layers | Any duplicate store, event authority, agent policy, or standalone product shell |

## Dependency-ordered execution

### C0 — Recover repository identity

- Preserve the dirty outer Connectome/Surreal/QORTEX checkout.
- Record the exact nested Tauri branch and commit.
- Decide the canonical GitHub home for the Tauri app before changing remotes.
- Mount that application at `apps/connectome` in RRFlow as a pinned submodule.
- Add a repository topology check that rejects an absent, dirty, or unexpected
  Connectome gitlink in release jobs.

Exit: RRFlow points to one reviewed Connectome application commit and no
retired engine repository is mistaken for the UI.

### C1 — Establish canonical runtime events

- Implement strict lifecycle envelope v1 and payloads in the RRD contract.
- Implement the transition state machine and enforcement-level rules.
- Materialize the project topology and persist the task, architecture,
  pattern-selection, preflight, and planning-permit evidence that Connectome
  will later explain.
- Persist transitions through the existing RRD control journal with CAS,
  digest chaining, idempotency, and reopen recovery.
- Refactor Claude and current MCP lifecycle handling into adapters.
- Fail closed for unknown mutation events.

Exit: conformance fixtures prove valid transitions, invalid-order denial,
mutation enforcement, exact replay, tamper detection, and restart recovery.

### C2 — Complete the diagnostic read model

- Extend `/v1/diagnostics/read` with lifecycle runs, routing, reasoning, causal
  traces, vector artifacts, storage/index decisions, and cluster sections.
- Add cursor-based run replay, point event reads, differences, and resumable
  committed-event streaming.
- Generate client schemas from `rrd-contract`; do not duplicate them in UI.

Exit: one read stamp binds every returned section, and section metadata states
whether coverage is authoritative/projection and complete/bounded.

### C3 — Attach the real Connectome client

- Stabilize the existing `rrd-diagnostics` TypeScript/Rust transport as the RRD
  diagnostics V1 contract in place.
- Replace temporary `/api/*` workbench endpoints with authenticated RRD client
  operations.
- Support local daemon discovery, explicit local endpoints, and authenticated
  remote TLS endpoints.
- Replace two-second whole-snapshot polling with resumable live events plus
  bounded reconciliation reads.

Exit: Connectome uses no physical storage/query/vector crate and cannot open an
RRD database directory directly.

### C4 — Absorb the diagnostics lab

- Port each tested workbench lens into the Tauri application.
- Preserve prompt flights, weak/strong demos, freeze/rewind/forward, temporal
  graph differences, causal traces, estates, models, tables, routes, query
  plans, vector artifacts, and cluster history.
- Delete no workbench behavior until a parity test passes against the same RRD
  fixture.

Exit: a generated parity manifest accounts for every workbench route and
interaction as `ported`, `replaced`, or explicitly `retired` with rationale.

### C5 — Audit and absorb Purgato

- Recover the exact repository and freeze its commit before edits.
- Inventory diagnostic collectors, data models, workflows, visual components,
  alerts, exports, and tests.
- Classify each item as engine evidence, diagnostic projection, Connectome
  presentation, or out of scope.
- Reimplement or move selected capability behind RRD's contract; port selected
  presentation into Connectome's Operate/Inspect layers.

Exit: no Purgato capability introduces a second journal, policy engine,
capability list, or desktop shell.

### C6 — Layman/operator translation

- Add an explanation model that maps canonical event types and evidence to
  controlled plain-language templates.
- Keep source identifiers, certainty, authority, and gaps visible.
- Add guided actions with previews, permission checks, progress, cancellation,
  receipts, and undo/restore boundaries where the engine supports them.
- Test expert and non-expert comprehension against the same traces.

Exit: every summary links to the exact evidence used, and no summary claims
more completeness or causality than RRD recorded.

### C7 — Retire the temporary shell and qualify the product

- Remove `crates/connectome-ui` only after parity and route-consumer searches
  prove all retained behavior is present in Tauri Connectome.
- Run Rust, TypeScript, ArkType, Biome, desktop, protocol, persistence, and UI
  tests on one commit.
- Build required Windows, macOS ARM, Linux x64, and Linux ARM artifacts.
- Run the remote CI matrix to completion before calling the client verified.

Exit: one RRD authority plus one Connectome desktop client passes the full
matrix; the browser lab is gone and no RRFlow public vocabulary remains.

## Minimum persistent replay scenarios

Completion requires 100% success for at least these twelve scenarios:

1. weak prompt baseline;
2. strong prompt baseline;
3. custom prompt with no tool mutation;
4. authorized mutating tool and verification success;
5. denied mutating tool;
6. tool failure followed by a corrected attempt;
7. stale preflight/read-stamp denial and refresh;
8. context compaction and continuation;
9. projection refresh failure and later recovery;
10. daemon restart during an incomplete run;
11. concurrent sessions with independent cursors;
12. retention boundary with an honestly reported unavailable history range.

For every scenario, the live view and post-restart replay must produce the same
ordered event identities, state transitions, causation graph, and terminal
outcome. UI snapshots alone are insufficient.

## Development and release gates

- `rrflow dev up` owns one RRD daemon; Connectome never opens the same store.
- `rrflow dev doctor` verifies protocol, credentials, capabilities, event
  freshness, submodule identity, and supported build tools.
- Capability and diagnostic schemas are generated from engine-owned contracts.
- UI tests run against a real disposable RRD daemon, not only mocked JSON.
- Replay tests close and reopen RRD between capture and inspection.
- Tauri transport tests cover loopback, authenticated remote TLS, response
  bounds, redirect denial, credential handling, and reconnect cursors.
- Cross-platform builds use the same pinned Connectome and RRFlow commits.
- Every executable slice updates `docs/implementation-journal.md` with commands,
  outcomes, known gaps, and the exact commit/CI run when pushed.

## Immediate next action

Execute the foundation cheat sheet's forced-planning slice first, beginning C1:
establish the canonical provider-neutral runtime-event contract, durable state
machine, materialized project topology, architecture assessment, pattern
selection, and planning permit in RRD. In feature terms nothing else is
authorized: Connectome cannot faithfully visualize, rewind, compare, or
explain lifecycle data that RRD does not yet record canonically.

Repository mutation for C0 waits only on the canonical GitHub identity for the
Tauri application. Purgato absorption waits on restoration of the intended
`purgato/` source content; neither uncertainty permits another UI
implementation to be started.
