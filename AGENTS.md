# OpenAI Codex CLI

<!-- rrflow:begin -->
## RRFlow project context

This project has provider-neutral RRFlow runtime state at `.rrflow/rrd`.
RRD persists bi-temporal claims with provenance, project routing,
reasoning lifecycle evidence, and exact tool authorization.

- Inspect the enforced board: `rrflow --db .rrflow/rrd work-plan status`
- Sync and activate only dependency-ready work: `rrflow --db .rrflow/rrd work-plan sync --root .`
then `rrflow --db .rrflow/rrd work-plan activate --item <id>`
- Attune before mutation: `rrflow --db .rrflow/rrd preflight --root .`
- Recall before searching: `rrflow --db .rrflow/rrd recall --subject <s>`
- Record what you decide: `rrflow --db .rrflow/rrd assert --subject <s> --predicate <p> --object <text>`
- If an RRFlow gate denies a tool call, follow the reported recovery
action. For projection divergence, run `rrflow --db .rrflow/rrd ground`
and review its evidence before `rrflow --db .rrflow/rrd reset-projection`.
<!-- rrflow:end -->
