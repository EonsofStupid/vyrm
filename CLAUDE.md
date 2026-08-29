# Claude Code

<!-- rrflow:begin -->
## RRFlow project context

This project has provider-neutral RRFlow runtime state at `.rrflow/rrd`.
RRD persists bi-temporal claims with provenance, project routing,
reasoning lifecycle evidence, and exact tool authorization.

- Load the `rrflow-engine-development` skill from `.agents/skills/rrflow-engine-development/SKILL.md` for project work.
- Recall before broad search, then inspect the current code, tests, worktree diff, and enforced board.
- Sync and activate only dependency-ready work: `rrflow --db .rrflow/rrd work-plan sync --root .`
then `rrflow --db .rrflow/rrd work-plan activate --item <id>`.
- Persist a reviewed goal and plan, then attune with `rrflow --db .rrflow/rrd preflight --root .`.
- Record one exact attempt before each mutation; observe and decide before the next attempt.
- Record what you decide: `rrflow --db .rrflow/rrd assert --subject <s> --predicate <p> --object <text>`
- If an RRFlow gate denies a tool call, follow the reported recovery
action. For projection divergence, run `rrflow --db .rrflow/rrd ground`
and review its evidence before `rrflow --db .rrflow/rrd reset-projection`.
<!-- rrflow:end -->
