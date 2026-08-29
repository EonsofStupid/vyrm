# RRD unified logical catalogue

## Authority

`rrd_core::RuntimeSchemaRegistry` is the one logical catalogue installed for a
runtime scope. It is a complete replacement at each positive, consecutive
revision and is published only as `RuntimeMutation::Schema` inside the normal
CAS-protected `RuntimeCommit`. There is no vector, graph, reasoning, or
lifecycle side authority.

Each registry names one namespace and database and maps a table kind to exactly
one `RuntimeLogicalModel`: document, relational, graph node, graph relation,
key-value, vector, event, time-series, geo, object, reasoning claim/record/event,
or lifecycle record/event. Record-like models share `RuntimeRecord` as their
canonical persisted identity. The catalogue model tells outward APIs which
view and operations apply without creating a second row identity.

## Strict and schemaless states

`RuntimeSchemaMode` is tagged, not inferred:

- `strict` validates the canonical value structure and its declared property
  contract. Record, relation, and event tables retain their richer uniqueness,
  endpoint, cardinality, and subject rules in their specialized schema entry.
- `schemaless` accepts any already-bounded `RuntimeProperties` bag. It cannot
  also declare strict properties or a specialized strict schema.

A table kind can occur only once in the effective catalogue. A record schema
cannot be catalogued as a relation, and a vector mutation cannot enter a geo
table. These conflicts fail before a store writes a cursor, schema, outbox, or
audit row.

## Migration boundary

Pre-G03 registries serialized only `records`, `relations`, and `events`. They
remain readable and retain identical canonical commit bytes; RRD derives strict
relational, graph-relation, and event table entries for diagnostics and
planning. The first revision with any explicit `tables` entry activates the
unified contract for that scope. From then on every non-claim mutation must
resolve to a matching entry. Claims retain their independent statically typed
bi-temporal contract.

## Transaction and recovery behavior

Schema and data share one commit. Revision continuity, table/model agreement,
strict property validation, dangling references, uniqueness, and cardinality
are checked before publication. Any failure leaves the previous schema and
cursor intact. The corrected retry uses the same normal commit path.

The registry already travels in the runtime hash chain, snapshots, logical
archives, replication entries, public transaction/changefeed contract, and
native database. Memory, Fjall compatibility, native RRD LSM, and both
persistent reopen paths return the same registry revision and table map.

G03-W02 adds one atomic mutation/read vocabulary over these identities.
Document, key-value, record, relation/native-edge, event, vector, time-series,
geo, and object values can be created, updated, retired at an explicit valid
time, and recreated without erasing their authenticated history. Event targets
use their immutable log cursor; every other target uses its typed reference.
Reasoning claims retain their separate bi-temporal retirement contract.

`RuntimeDataSnapshot` reduces all model families through one catalogue
revision, valid-time instant, authenticated transaction cursor, and read
manifest. Retirement-target validation observes the same pre-commit cursor and
the backend compare-and-swap remains the final race authority, so a missing
target or competing writer cannot partially publish a mixed-model batch.
Memory, Fjall, native RRD LSM, and persistent reopen fixtures execute the same
contract. G03-W03 and G03-W04 bind mutating RRFlowQL and Arrow/DataFusion to
this exact immutable catalogue snapshot.
