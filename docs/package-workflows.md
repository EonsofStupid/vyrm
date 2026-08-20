# Canonical package workflow policy

Status: local alpha contract, 2026-08-20.

Package script names are identities, not permissions. Vyrm only enforces a
package command when the project owns a strict `.vyrm/workflows.toml` that
binds its direct argv to the instance, required projections, zero-lag source
policy, and verification rule.

```toml
format = 1

[[workflows]]
event = "package:pnpm:run:typecheck"
command = ["pnpm", "run", "typecheck"]
allow_arguments = false
scope = "my-instance-id"
required_projections = ["source-routing"]
max_source_lag_generations = 0
verification = "exit_zero"
```

`scope` must exactly equal the ID in `.vyrm/instance.toml`. Supported
verification policies are `exit_zero` (passed/failed/unverified from the
observable exit code) and `observe` (execution is evidence, not a pass claim).
The first alpha supports only `source-routing` and strict zero-lag freshness;
unknown fields, formats, projections, duplicate events, vague commands, and
scope mismatches are denied rather than ignored.

## Lifecycle

1. Session preflight validates the manifest, captures a runtime `ReadStamp` for
   every declared event scope, and injects the event, manifest SHA-256, cursor,
   and manifest identity. A JS project without policy receives a loud warning.
2. Pre-tool first applies the typed reasoning-run gate, then refreshes source
   routing, then resolves the exact package event and argv. Missing, corrupt,
   undeclared, cross-instance, shell-composed, or stale policy returns a harness
   `permissionDecision: deny`.
3. Post-tool reloads the project policy. It hashes the exact command and
   response, retains only the declared command plus argument count (not
   potentially secret arguments or output), and derives the typed status.
4. The observation becomes a temporal `package:*` status claim inside the same
   CAS commit as its runtime changes, commit outcome, schema bootstrap, and
   hash-chained audit envelope. Re-runs supersede prior status in that same
   transaction.

Hook and MCP callers share `vyrm_node::handle`, so decision and persistence
semantics do not fork by provider. The reasoning contract makes an unresolved
attempt deny the next mutation. A future distributed coordinator may add a
durable pre/post authorization lease for concurrent tool calls; this local
alpha does not claim that cross-process lease yet.

## Frozen evidence

- `crates/vyrm-node/fixtures/workflow-v1.toml` freezes the manifest contract.
- `crates/vyrm-node/fixtures/workflow-observation-v1.json` freezes the compact,
  digest-bound observation envelope.
- `workflow_lifecycle.rs` proves identical observation and audit evidence on
  Memory, Fjall, and native VyrmKV, plus denial when policy is absent.
