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

## One-run learning workflow

The workbench opens with one editable prompt. Weak and strong examples are
starting points, not a synthetic scorecard. Every click launches exactly one
persisted flight. Repeating identical prompt bytes at another effort or context
arm adds a run to the same digest cohort; changing the prompt starts a new
cohort. The live goal/scope/evidence/success/stop indicators are lexical editing
aids only. Runtime metrics begin only after the operator launches the flight.

The four UI profiles map to exact provider arguments:

| Vyrm profile | Requested provider effort |
|---|---|
| Default | `medium` |
| High | `high` |
| Extreme | `xhigh` |
| Ultra | `max` |

These names are experiment controls. Ultra is Vyrm's quality-first label for
provider `max`, not Codex's separate multi-agent ultra mode. The mapping follows
the current [OpenAI model guidance](https://developers.openai.com/api/docs/guides/latest-model)
and is recorded on the flight and its provider-spawn event.

The timeline can be frozen at any event, scrubbed, rewound, resumed, or
fast-forwarded. Its aligned lanes distinguish context/routing, provider model
envelopes, tool activity, and outcome evidence. Packet height encodes only the
captured visible detail plus raw payload byte count. The expanded micro-event
shows the complete observable envelope. It does not infer or reconstruct hidden
chain-of-thought.

## Metrics

- `context_tokens`: Vyrm's declared rendered-context estimate;
- `input_tokens` and `output_tokens`: provider-reported values when available;
- `cached_input_tokens`: provider-reported cached input when available;
- `reasoning_tokens`: provider-reported reasoning-token count when available;
- `provider_events`: count of raw provider envelopes retained by the flight;
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
