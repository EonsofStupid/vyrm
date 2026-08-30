# RRD online filtered HNSW v2

G04-W02 keeps HNSW inside the RRD engine rather than introducing a vector
sidecar or a second write authority. Canonical point versions remain in the
shared authenticated runtime log. The collection catalogue fixes the named
dense vector's field, dimensions, metric, model binding, and payload indexes.
HNSW is a replaceable, content-addressed projection over those facts.

## Immediate visibility

An active HNSW generation has an immutable source cursor. Vector puts, updates,
and retirements after that cursor form a bounded authoritative delta. Planning
may select the HNSW base only when that exact delta is available through the
request's required source cursor. The plan retains both cursors and the delta
candidate count.

Traversal proposes candidates from the base graph while applying the requested
payload expression at layer zero. Final exact evaluation receives those graph
candidates plus every relevant delta version. One latest-version reducer then
applies transaction visibility, valid time, retirement, payload filtering,
metric scoring, and deterministic ordering. A newly inserted nearest neighbor
is therefore visible immediately, and a retirement suppresses the old graph
node immediately, before maintenance publishes another generation.

`AllowApproximate` can choose an exact scan when graph navigation plus the delta
costs more. `RequireApproximate` requires an identity-compatible HNSW base and
uses the exact delta to close freshness; it never serves the stale base alone.

## Non-blocking incremental maintenance

For an unchanged HNSW configuration, maintenance reads the active verified
artifact and inserts only vector versions whose source cursor is newer than the
active cursor. It does not reconstruct existing nodes. The resulting immutable
generation records:

- `maintenance = incremental`;
- the immediately previous generation;
- the number of inserted vector versions;
- the new authoritative source cursor and total node count.

The old generation remains available while insertion and object staging run.
Publication is one existing object-plus-catalogue CAS; canonical point commits
never wait for HNSW work. Repeating maintenance with no new vector version is an
idempotent replay. Changing dimensions, metric, model binding, graph parameters,
seed, or indexed filter properties deliberately creates a full-build generation.

Retirement is represented as a new cursor-bearing vector version instead of an
in-place rewrite. That preserves reads before the retirement commit, makes
delete freshness measurable, and lets incremental maintenance fold the exact
tombstone into the graph history.

## Filter authority

HNSW and TurboQuant filter properties must name active typed collection payload
indexes. A payload index referenced by an active artifact cannot be deleted.
During HNSW traversal, non-matching nodes remain available for navigation but
cannot enter the eligible result heap. Final exact reranking reapplies the same
expression. This avoids both pre-filter disconnects and post-filter leakage.

The executable filter corpus covers equals, not-equals, membership, numeric
range, existence, all, any, and not. Fixed recall cells cover 100%, 50%, 10%,
and 1% selectivity.

## Metrics and kernels

Cosine, dot product, Euclidean distance, and Manhattan distance use scalar
construction for portable deterministic artifact bytes. Query traversal uses a
runtime-dispatched AVX2 scorer on supported x86_64 hosts and the scalar oracle
elsewhere. Exact reranking remains the final authority. A fixed 512-vector,
16-dimensional corpus proves scalar/automatic result identity and at least
0.95 mean Recall@10 for every metric/selectivity cell at `ef=128`.

## Failure boundaries

- a missing/corrupt artifact, wrong collection identity, metric, dimensions,
  model, or filter coverage fails closed;
- an overlay is valid only for HNSW, must advance the base cursor, and must
  contain at least one authoritative candidate version;
- incremental generation numbers must be consecutive and source cursors must
  advance;
- duplicate or out-of-coverage delta versions, node/generation overflow, and
  graph corruption fail before publication;
- format v1 graph bytes are not silently interpreted as v2. V2 uses magic
  `RRDHNS02` and format version `2` because maintenance provenance changes the
  authenticated body contract.

Compact graph/payload-bitmap storage, ACORN-style payload-derived edges,
automatic merge thresholds, distributed placement, and GPU construction remain
later gates. They may optimize this authority; they may not replace it.
