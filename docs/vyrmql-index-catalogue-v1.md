# VyrmQL index catalogue foundation

Status: authoritative persistent catalogue, content-addressed exact scalar
artifacts, verified selection/execution, and fail-closed planner evidence are
implemented. Public RRD administration routes, uniqueness enforcement,
incremental maintenance, and broader index families remain open.

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
its configuration still hashes identically, its source cursor exactly equals
the requested known cursor, and its built valid-time equals the query
valid-time. Exact equality is required because a newer artifact could contain
knowledge unavailable at an older bi-temporal read. Publication cannot name
cursor zero, a cursor beyond that scope's head, the wrong generation, or a
non-building entry.

VyrmMX builds an artifact by executing the exact all-fields reference query at
one captured head and explicit valid-time. Canonical rows are durably published
under a content-addressed projection name before the catalogue marks that
generation ready. The executor revalidates the read stamp, artifact digest,
scope, definition, generation, source cursor, schema revision, valid-time, and
row count before using the bytes. Filters and projection are reapplied by the
same reference evaluator.

A query whose filters match the leading fields of an exact ready artifact
selects it in `EXPLAIN CONTRACT`. A stale cursor, different valid-time,
building/quarantined state, or absent coverage keeps the authoritative log
selected. Multiple eligible artifacts choose the longest matching prefix and
then stable index identity. This is a usable exact snapshot index, not yet an
incrementally maintained or universally temporal index.

Tests build and query real rows identically on memory, Fjall compatibility, and
native VyrmKV, prove stale fallback after a write, fence a stale generation,
reject invalid definitions before control mutation, verify the journal chain,
reopen the native artifact, and fail closed on corrupted artifact bytes. The
full workspace test suite passes.
