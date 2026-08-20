# Anytype-inspired connectome workbench

Research date: 2026-08-18. This is an interaction study, not a proposal to
copy Anytype's branding or source.

## Patterns worth adopting

| Anytype pattern | Connectome translation |
|---|---|
| Objects accumulate properties and relationships | Claims, runs, evidence, files, and invocations are inspectable objects with stable identities |
| A Channel/Space owns its objects and sidebar | A vyrm instance owns its runtime state and navigation; an umbrella must still name members explicitly |
| Sidebar widgets provide persistent lenses | Overview, Graph, Runs, Claims, Routes, and Activity are stable developer lenses |
| Types, Queries, and Collections can render through different Views | The same runtime snapshot can render as graph, timeline, table, route result, or inspector |
| Global and local graph navigation | Global instance map exists, but local selection-centered graphs are the default |
| Desktop frontend is separated from local middleware | The workbench is a browser UI over a local read-only Rust API; the store remains authoritative |
| Graph rendering moves expensive simulation off the main UI path | Start with bounded SVG for runtime-sized snapshots; retain a worker/WebGL migration boundary when measurements require it |
| Selecting one object recenters its local relationships and properties | A frozen micro-event becomes the focused object while typed context, model, tool, and outcome lanes preserve its surrounding sequence |
| Composable blocks keep dense object data explorable | The flight stage, transport, event envelope, baseline, and inspector remain independently inspectable surfaces over one authoritative record |

Primary references:

- [Anytype objects](https://doc.anytype.io/anytype/create/objects)
- [Anytype types](https://doc.anytype.io/anytype/organize/types)
- [Anytype views](https://doc.anytype.io/anytype/organize/views)
- [Anytype graph](https://doc.anytype.io/anytype/features/graph)
- [Anytype sidebar](https://doc.anytype.io/anytype/basics/sidebar)
- [Anytype channels/spaces](https://doc.anytype.io/anytype/basics/channels)
- [Anytype desktop and graph architecture](https://github.com/anyproto/anytype-ts/blob/develop/CLAUDE.md)
- [Anytype browser-mode middleware boundary](https://github.com/anyproto/anytype-ts/blob/develop/docs/src/ts/lib/web/README.md)

## Corrections for a frontier-runtime tool

An undifferentiated knowledge graph becomes decorative at scale. Connectome's
default graph therefore centers on the selected run, claim, evidence item, or
file. Edge labels remain visible and filters operate on runtime semantics, not
just colors. The global view is an optional orientation mode.

The workbench is initially read-only. Reasoning and mutation gates remain on
the existing typed lifecycle surfaces; a visual button must not become a path
around policy. Later controls should call the same commands and expose their
contract differential before execution.

## First information architecture

```text
instance sidebar          active lens                         inspector
├─ Overview               health + active run                selected object
├─ Temporal stream        scoped mutation lanes + clock      mutation + audit
├─ Graph                  local/global runtime map            identity
├─ Runs                   typed event timeline                provenance
├─ Claims                 current claim table                 validity
├─ Routes                 query + ranked files                justification
└─ Activity               invocation stream                   outcome
```

The top command field searches across all loaded objects and doubles as the
route query in the Routes lens. Keyboard navigation is first-class: `/` focuses
search; `t`, `g`, `r`, `c`, and `a` open the main lenses.

## Implemented workbench

The `connectome` binary serves embedded HTML, CSS, and JavaScript from the same
local Rust process as its instance-bound API. It currently provides:

- runtime health and freshness overview;
- selection-centered and global object graphs with semantic filters;
- claims, reasoning transitions, and evidence as first-class graph objects;
- current-claim inspection including validity, producer, and SHA-256 identity;
- reasoning-run timelines;
- ranked full-file source routes with visible justification;
- invocation activity and outcomes;
- stable hash routes and keyboard navigation.
- controlled prompt flights across fresh, pruned, and full context arms;
- live event playback with pause, step, scrub, stage jumps, and raw inspection;
- a one-run reasoning lab with exact `medium`/`high`/`xhigh`/`max` effort
  controls and explicit observable-only boundaries;
- aligned event-mass lanes for context, model envelopes, tools, and outcomes,
  with click-to-freeze packets and complete captured-envelope expansion;
- a bounded global temporal stream over persisted reasoning, routing, workflow,
  model/flight, search, storage, and data mutations, with first/rewind/play/
  forward/latest controls and mutation-plus-audit inspection;
- same-prompt baseline comparison for context, provider tokens, tools, latency,
  and acceptance.

The transport verifies the instance/store pairing and accepts writes only for
the explicit prompt-flight endpoint. Frontier runners are disabled by default,
run with read-only/plan-mode permissions when enabled, and never receive a
shell-interpolated prompt. The server binds to loopback by default and requires
an explicit warning-bearing override for remote binding.

## Evolution path

1. Replace five-second full snapshots with sequence-cursor runtime deltas. The
   global temporal stream is already cursor-ordered but is delivered as the
   newest bounded snapshot; flight events use a separate 750 ms lightweight
   feed.
2. Add saved, instance-specific lenses without turning those preferences into
   authoritative runtime truth.
3. Move graph layout/rendering to a worker and WebGL only after node-count and
   frame-time measurements justify it.
4. Keep provider execution read-only until operator actions can cross the
   existing lifecycle/policy contract and show the differential before
   confirmation.
5. Enable umbrella instances only after every query and visualization carries
   an explicit member scope.

Browser acceptance is reproducible against a running workbench:

```bash
CONNECTOME_URL=http://127.0.0.1:4387 \
  bunx playwright test crates/connectome-ui/tests/workbench.spec.js --workers=1
```
