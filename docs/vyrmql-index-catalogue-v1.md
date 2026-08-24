# VyrmQL index catalogue foundation

Status: authoritative persistent catalogue and fail-closed planner evidence are
implemented. Materialized scalar artifacts, verified artifact readers, public
RRD administration routes, and index-selected execution remain open.

An index definition binds a stable projection ID to one non-recursive VyrmQL
source, an ordered list of one to sixteen fields, and an optional uniqueness
requirement. Creation binds every field against the captured schema before any
control state changes. Duplicate fields, unknown fields, cross-scope catalogues,
and recursive traversal sources fail closed.

Each entry carries the shared projection stamp:

- generation fences stale builders;
- source cursor states exactly how much authoritative runtime history it covers;
- configuration digest binds the definition;
- artifact digest binds produced bytes;
- state is `building`, `ready`, `quarantined`, or `retiring`.

The catalogue is authoritative control state, not a buffered projection blob.
Every create, rebuild, publish, quarantine, and retire uses compare-and-swap and
appends to the authenticated control journal. A ready entry is usable only when
its configuration still hashes identically and its source cursor covers the
requested known cursor. Publication cannot name cursor zero, a cursor beyond
that scope's head, the wrong generation, or a non-building entry.

VyrmMX captures the catalogue with the schema. A query whose filters match the
leading fields of an index receives a named candidate with generation, state,
freshness, and prefix evidence in `EXPLAIN CONTRACT`. The candidate remains
unselected and non-exact until a content-verified artifact reader is connected;
the authoritative log path continues to answer the query. This makes current
planning behavior inspectable without claiming acceleration before it exists.

Tests run the lifecycle identically on memory, Fjall compatibility, and native
VyrmKV, fence a stale build generation, reject an invalid definition without a
control mutation, verify the journal chain, prove native reopen, and assert the
planner's explicit rejection evidence. The full workspace test suite passes.
