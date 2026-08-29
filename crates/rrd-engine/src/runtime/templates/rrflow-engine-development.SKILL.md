---
name: rrflow-engine-development
description: Mandatory project workflow for any RRFlow engine inspection, diagnosis, planning, implementation, refactor, test, CI repair, architecture review, or foundation work in a repository containing .rrflow/rrd. Use before broad code search or project mutation; do not use for unrelated general questions.
---

# RRFlow engine development

Treat RRD as the durable execution authority and the checked-in work plan as the scope authority.

1. Recall before broad search. Query the subjects named by the request with `rrflow --db .rrflow/rrd recall --subject <subject>`. If the subject is unclear, recall the nearest subsystem rather than scanning the whole repository first.
2. Inspect before planning. Read `git status --short --branch`, the active work item, the existing diff, and the smallest relevant implementation, tests, contracts, and architecture sources. Distinguish committed work from the current uncommitted attempt. Do not mutate during this inspection.
3. Sync the board with `rrflow --db .rrflow/rrd work-plan sync --root .` and inspect `work-plan status`. Activate only a dependency-ready item and never maintain a parallel progress count.
4. Persist the reasoning contract. A nontrivial change requires one active run containing a goal with measurable acceptance, a hypothesis and ordered plan grounded in inspected code, and one exact attempt before a mutating tool call.
5. Attune immediately before mutation with `rrflow --db .rrflow/rrd preflight --root . --harness <adapter>`. If source, policy, routing, or the work-item plan changes, attune and bind the plan again.
6. Execute one cohesive-engine change at a time. Prefer existing public engine paths and canonical state over parallel publishers, compatibility forks, or test-only facades. One attempt authorizes one exact mutation; record its observation and a continue, verify, or stop decision before another attempt.
7. Verify proportionally through the recorded work-item checks. Start with focused contract and regression tests, then run the owning subsystem suite. Treat CI as evidence of the same local contract, not as an alternate architecture.
8. Close the loop. Record measured outcomes and negative results in RRD, update the authoritative architecture/status sources, inspect the final diff, and advance the board only from fresh passing evidence.

When a native hook denies an action, follow its stated recovery. Never bypass the hook to make progress. Providers without a blocking local hook lifecycle must mutate through the RRFlow MCP or exact-argv proxy; their context file and this skill are guidance, not an enforcement claim.
