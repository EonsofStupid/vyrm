# Prompt-flight experiment contract

Connectome prompt flights answer one narrow question: what observable effect
does Vyrm context have on a frontier model for the same prompt and repository
state?

They are an optimization instrument, not a chain-of-thought viewer. The ledger
captures only events exposed by Vyrm, the provider CLI, and tool-result
envelopes.

## Unit of comparison

The cohort identity is the SHA-256 digest of the trimmed prompt. Flights belong
to the same cohort only when their prompt bytes are identical. A valid paired
comparison also holds these variables fixed:

- provider and provider version;
- repository revision and dirty-worktree state;
- acceptance marker;
- context budget;
- runner permissions;
- machine and relevant cache state, when latency is compared.

Changing one of those variables creates a new experiment, even if the UI still
shows the flights under the same prompt digest.

## Context arms

### Fresh

A fresh flight starts a provider session with no Vyrm context. Codex uses an
ephemeral read-only invocation; Claude uses a non-persistent plan-mode
invocation. Existing claims and flight evidence are not deleted. "Fresh" is an
input condition, not a storage operation.

### Pruned

The prompt is matched against current claim subjects. Only matched claims that
fit the declared budget are rendered into the provider packet. This is the arm
that tests whether a small amount of targeted memory helps a vague request.

### Full

The provider packet receives the bounded session preflight followed by
prompt-matched recall. This measures the value and cost of broad orientation.

Routing runs as an observed stage for all arms. Ranked file names are recorded
for diagnosis but are not injected into the provider packet by the current
implementation; changing that would create a different intervention.

## Flight events

Every event has an ordinal, wall-clock instant, elapsed time, stage, kind,
label, detail, and optional provider payload.

```text
prompt → context → recall → routing → model → tools → outcome
```

Stages may be absent. Fresh has no recall injection, an observe-only flight has
no provider tool calls, and provider failures may jump directly to outcome.
Missing stages remain evidence rather than being synthesized for visual
continuity.

The UI playback clock is presentational. Recorded `elapsed_ms` is the
measurement; animation timing is never used as benchmark evidence.

## Built-in weak/strong demonstration

The workbench opens with two editable prompts. The guided weak arm has no target
or acceptance contract; the guided strong arm names the runtime stages, safety
boundary, evidence requirements, metrics, and stop condition. Running that
unchanged pair writes it through the production flight recorder as two record
revisions and sixteen immutable events in one atomic runtime commit. Repeating
the action returns the existing pair instead of polluting history with copies.

Editing either prompt changes the experiment into a custom A/B run. Both sides
use the same selected context arm, provider, token budget, and acceptance marker
and are pinned together in the interface. The live goal/scope/evidence/success/
stop indicators are lexical editing aids only. Runtime metrics begin only after
the operator runs the pair.

The timeline can be frozen at any event, scrubbed, rewound, or fast-forwarded.
Event bursts encode the approximate size of the event's visible detail and
typed payload; the expanded data modal shows the underlying keys and values.
These deterministic numbers demonstrate the visual and persistence contract.
They must never be cited as a frontier-model evaluation result.

## Metrics

- `context_tokens`: Vyrm's declared rendered-context estimate;
- `input_tokens` and `output_tokens`: provider-reported values when available;
- `tool_calls`: externally observable provider tool events;
- `latency_ms`: launch through provider exit;
- `acceptance_met`: successful process exit plus the optional literal marker.

An empty acceptance marker establishes transport success only. It cannot prove
task correctness. Optimization decisions require a non-trivial marker or a
future structured evaluator, repeated trials, and retained raw evidence.

## Reading vague-context results

A useful sequence is:

1. Begin with the shortest prompt that expresses the desired outcome.
2. Run fresh and record failure modes before adding context.
3. Run pruned without changing the prompt.
4. Run full only if pruned leaves an identifiable orientation gap.
5. Tighten the prompt independently of context and repeat as a new experiment.
6. Prefer the lowest-context arm whose repeated success and regression profile
   match the stronger arm.

One successful flight is a trace, not a conclusion. A result earns promotion
only after repeated trials across representative repositories and at least two
frontier providers.

## Retention and pruning

Flight history is stored in the instance-local authoritative flight ledger.
The current implementation does not silently age out runs. Later pruning must
first export cohort summaries and content identities, then record an explicit
retirement decision. Authoritative claims and reasoning runs are governed by
their own bi-temporal contracts and are never purged by a baseline experiment.

## Security boundary

Frontier runners are disabled by default and require `--enable-runners`.
Provider names map to fixed argument vectors; prompts are passed as process
arguments, never through a shell. The shipped runners are read-only/plan-mode.
Connectome remains loopback-only unless the operator explicitly permits a
remote unauthenticated bind.
