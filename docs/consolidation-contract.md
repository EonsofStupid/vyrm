# Consolidation contract

This repository does **not** need a rewrite. It needs one enforced execution
path and an explicit extraction boundary. Existing storage, routing, reasoning,
policy, and observability code are useful substrate; none may become another
orchestrator.

## One engine

The only product flow is:

```text
prompt submitted
  -> derive and verify project shape
  -> attune + recall + route complete source files
  -> create/update goal and plan
  -> select declared routine from current context
  -> authorize one attempt
  -> execute and capture observation
  -> verify
  -> decide and persist outcome
  -> publish the same events to Connectome
```

`vyrm-node` is the engine for this flow. CLI commands, hooks, MCP, and any UI
adapter are ports into it; they must not implement alternate lifecycle logic.
Storage and graph crates are engine dependencies, not competing engines.

## Shape before planning

Run the deterministic project-shape generator before asking a model to plan:

```bash
python3 scripts/project_shape.py . --output .vyrm/project-shape.json
```

The output is a sorted file inventory with content identities and recognized
manifest/instruction roles. Generated state and dependency/build directories
are excluded. A model may summarize this artifact and propose routes, but must
not invent the inventory. The tree digest makes stale context detectable.

This is intentionally deterministic code rather than an ML-generated tree.
The filesystem is authoritative; ML is appropriate for interpreting the shape,
not for deciding what exists.

## Routines and templates

Project-specific behavior must be generated from versioned, generic contracts:

1. **Template** declares supported stack, required evidence, inputs, mutation
   policy, verification, and event triggers.
2. **Instance configuration** binds a template version to an exact project
   identity and supplies only project-specific values.
3. **Generated runtime configuration** is validated, content-addressed, and
   reviewable before activation.
4. **Routine selection** is a decision inside the engine, based on persisted
   context and current project shape. A routine cannot launch a parallel agent
   loop or bypass reasoning and verification.

Until that generator exists, hand-authored workflow manifests are transitional
inputs, not the intended authoring surface.

## Connectome extraction

Connectome must become a separate repository and process. The extraction is a
boundary change, not a second engine:

- Vyrm owns the engine, persistence, typed events, and a versioned read/control
  protocol.
- Connectome owns visualization, authoring, replay, hop inspection, and
  template/configuration UX.
- Connectome observes the exact events produced by the engine. It does not
  reconstruct state from UI-local ledgers or implement prompt orchestration.
- During extraction, `crates/connectome-ui` is a compatibility client. New
  engine behavior must not be added there.

## Ordered recovery

1. Make project shape a persisted preflight input and reject stale shapes.
2. Collapse prompt submission into one engine entry point returning context,
   route, goal/plan, chosen routine, and observable hop identifiers.
3. Define and generate routine templates and per-project bindings.
4. Expose the engine entry point and event stream through a versioned protocol.
5. Move the compatibility UI to the Connectome repository and consume only
   that protocol.
6. Delete duplicate orchestration only after differential tests prove adapters
   produce the same engine events.

Rewriting first would discard tested persistence and policy behavior while
leaving the missing boundary undefined. Consolidating in this order makes each
removal provable and keeps the system deployable while it is corrected.
