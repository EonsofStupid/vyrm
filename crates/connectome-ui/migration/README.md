# Connectome migration input

This directory preserves behavior from the earlier embedded Connectome client
without compiling it into the current client-only product. RRD remains the
only data authority. Files here are inputs to G09, not a second application or
a permitted physical-store boundary.

| Input | Preserved behavior | Current bypass debt | G09 absorption target |
|---|---|---|---|
| `embedded-authority/flight.rs` | prompt flights, replay, context cohorts, trace capture | opens and mutates the physical RRD store | authenticated RRD diagnostic/runtime-tool APIs plus the Connectome replay viewport |
| `embedded-authority/cluster.rs` | cluster samples, alerts, historical telemetry | reads physical cluster/store state | public cluster diagnostic contracts through `rrd-client` |
| `embedded-authority/connections.rs` | saved connection profiles and negotiation receipts | persists profiles in the physical control store | client-owned preferences plus authenticated RRD capability negotiation |

Rules:

- Do not import these files from `src/` or add their physical dependencies to
  `connectome-ui` production dependencies.
- Do not delete or replace their behavior with placeholder UI.
- Absorb one behavior at a time behind generated `rrd-contract`/`rrd-client`
  operations, with replay and visual parity tests, then remove that migration
  input in the same reviewed G09 slice.
- Update this inventory whenever an input is absorbed or its disposition
  changes.
