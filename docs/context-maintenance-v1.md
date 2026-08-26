# RRFlow context maintenance v1

Status: contract freeze for an executable local Connectome alpha. This is not
yet the remote RRD administration contract or a claim that a universal pruning
ratio is safe.

## Purpose and authority

A context-maintenance run derives a smaller AI working projection from one
stable RRFlow history cut. It never deletes or rewrites the authoritative claim
log, typed runtime log, source repository, or Git history.

Before a proposal can advance, RRFlow creates and verifies an F1 logical backup
catalogue entry. The archive remains the recovery source; hot and warm context
are rebuildable projections. This follows the event-sourcing rule that
snapshots and materialized views optimize access but do not replace the event
stream.

The research justifies an evaluated range, not a magic constant:

- [Selective Context](https://aclanthology.org/2023.emnlp-main.391/) reports a
  useful 50% reduction operating point.
- [LongLLMLingua](https://arxiv.org/abs/2310.06839) reports benchmark-specific
  gains around 4x compression, equivalent to a 75% token reduction.
- [Lost in the Middle](https://arxiv.org/abs/2307.03172) shows why a larger
  prompt is not automatically a better prompt.

V1 therefore admits requested targets from 50% through 75%, defaults to 65%,
and refuses application unless instance-specific replay evidence passes. If a
proposal cannot safely reach 50%, the system reports that result rather than
discarding required information.

## One source, three temperatures

- **Hot / injected:** active goals and constraints plus the smallest canonical
  decision set required on every model turn.
- **Warm / retrieved:** distilled attempts, lessons, and summaries that remain
  addressable through governed recall but are not injected by default.
- **Cold / archived:** authenticated raw events, provider envelopes, tool
  output, duplicates, superseded material, artifacts, and prior projections.

Cold means absent from the active prompt, not destroyed.

Every inventory class has a count, source byte estimate, token estimate,
derivation label, proposed disposition, selected disposition, rationale, and
optional operator note. V1 freezes these classes:

1. active goals and constraints;
2. canonical decisions;
3. completed attempts;
4. superseded plans;
5. duplicate observations;
6. failures with reusable lessons;
7. provider envelopes;
8. tool outputs.

Active goals and constraints must remain hot. Canonical decisions and failure
lessons may be hot or warm but never cold. Any operator override must retain a
note in the run state and audit history.

## Guided state machine

Only one non-terminal run may exist for an instance.

1. **Inventory** captures exact claim and runtime watermarks and a derived,
   bounded class inventory.
2. **Protect** creates and reopens an authenticated logical archive catalogue
   entry covering at least that inventory cut.
3. **Propose** records the AI-selected dispositions, target, estimated hot,
   warm, cold, and reduction totals, and a deterministic projection digest.
4. **Review** records operator overrides and notes without changing the source
   history or active projection.
5. **Validate** accumulates typed evidence. Failed or pending gates keep Apply
   closed and may be rerun; they never silently downgrade to warnings.
6. **Apply** atomically publishes a new active projection generation while
   retaining its predecessor for rollback.
7. **Observe** compares actual task, token, latency, and recall behavior. The
   operator either accepts the generation or publishes a compensating rollback
   generation; prior state is never overwritten.

Each state revision is a strict JSON document stored as a typed runtime record.
Transitions emit a typed event in the same exact-cursor commit. Reopen scans and
verifies revision order, previous-state digests, state digests, record identity,
and event identity before exposing a run.

## Mandatory validation gates

Application requires every gate to pass with a SHA-256 evidence identity:

- authenticated archive integrity and watermark coverage;
- active goal and constraint recall;
- canonical decision recall;
- deterministic projection reproduction;
- same-task replay against the baseline and candidate projection;
- bounded task-success, regression, token, and latency differential.

Structural checks can be produced locally. Task replay and regression evidence
must come from completed same-cohort prompt flights or a later benchmark runner;
the UI must show them as pending when that evidence does not exist.

## Connectome interaction contract

Connectome presents one run at a time as Inventory → Protect → Propose → Review
→ Validate → Apply → Observe. Every visual value links to the persisted run,
backup, runtime cursor, flight, or evidence digest that produced it.

The user can change a class disposition, attach a note, preview the exact JSON
and token differential, run validation, apply only after all gates pass, freeze
or replay the associated prompt flights, and publish a rollback generation.
Controls never auto-submit on dropdown or slider changes. Draft edits remain
local until the user invokes the next explicit transition.

## V1 limits

- The first inventory classifies RRFlow reasoning, prompt-flight, provider, and
  tool history. It does not rewrite source-control history or compact arbitrary
  project files.
- Logical archives include claims and typed runtime history; their existing F1
  coverage declarations remain authoritative for objects, projections,
  invocation telemetry, and snapshot leases.
- Remote administration, multi-user approval, scheduled maintenance, physical
  hot/warm/cold placement, automatic summarization quality, and enterprise
  retention policy remain later gates.
