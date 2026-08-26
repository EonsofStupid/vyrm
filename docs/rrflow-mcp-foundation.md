# RRFlow MCP foundation

**Status:** authoritative implementation map as of 2026-08-25. This document
does not promote planned tools to executable capability.

## Contract

RRFlow MCP is an adapter over RRD. It never owns storage, sessions, policy,
transactions, indexes, or audit truth. Tool discovery is generated from the
same typed registry that dispatches execution and feeds the product capability
catalogue. An advertised tool without executable dispatch is a conformance
failure; an executable tool omitted from discovery is also a conformance
failure.

MCP exposes task-level operations. RRD transport mechanics such as session
renewal and transaction lease management remain internal to the adapter. They
must still cross the ordinary RRD session, authorization, idempotency,
transaction, and audit paths; they are not bypassed or reimplemented.

## Executable now — 28

| Tool | Authority | Honest limit |
|---|---|---|
| `rrflow_audit_read` | persistent security audit journal | governed bounded page; credentialed secured-mode MCP remains an adapter-topology gate |
| `rrflow_backup_create` | authenticated logical archive authority | exact-authorized idempotent logical backup with explicit coverage report |
| `rrflow_backup_list` | authenticated backup catalogue | governed catalogue read with optional archive verification |
| `rrflow_context` | claim/runtime memory | bounded current claims, not hybrid retrieval |
| `rrflow_changefeed_follow` | retained runtime changefeed | governed bounded long-poll with explicit timeout result |
| `rrflow_changefeed_read` | retained runtime changefeed | governed validated multi-model page after an exact cursor |
| `rrflow_data_commit` | shared RRD transaction | exact-authorized atomic multi-model commit; high-level adapter owns session/lease plumbing |
| `rrflow_estate_read` | persistent estate authority | governed desired/observed/activity/operation read; no estate mutation surface |
| `rrflow_forget` | claim transaction | retires history; does not erase it |
| `rrflow_inspect` | claim history | exact subject/predicate inspection |
| `rrflow_lifecycle` | runtime lifecycle | cooperative through MCP; interception depends on host runtime |
| `rrflow_live_query_poll` | RRFlowQL live-query engine | governed bounded row additions/updates/removals after a cursor |
| `rrflow_preflight` | attunement and recall | exact claim recall plus source routing |
| `rrflow_query` | RRFlowQL execution | read-only query path |
| `rrflow_query_index_ensure` | shared query-index catalogue | exact-authorized create/rebuild with durable idempotency and projection evidence |
| `rrflow_query_index_list` | shared query-index catalogue | governed bounded catalogue read; no lease/token fields |
| `rrflow_reasoning_record` | reasoning event log | typed externally observable transitions only |
| `rrflow_reasoning_show` | reasoning event log | active or exact run inspection |
| `rrflow_recall` | claim recall | exact subjects, not lexical/vector fusion |
| `rrflow_remember` | claim transaction | one attributable bitemporal fact |
| `rrflow_restore` | authenticated logical archive authority | exact-authorized restore to a path-closed new root after explicit acknowledgement |
| `rrflow_route` | source graph | lexical/symbol routing, not semantic code search |
| `rrflow_service_status` | RRD discovery | public readiness, security state, endpoints, tools, and cross-surface capability dispositions |
| `rrflow_vector_collection_ensure` | vector collection catalogue | exact-authorized idempotent named-vector definition convergence |
| `rrflow_vector_collection_list` | vector collection catalogue | governed collection/generation/model/memory-tier inspection |
| `rrflow_vector_points_retrieve` | canonical vector log plus collection catalogue | governed bounded exact-address retrieval with explicit missing identities |
| `rrflow_vector_points_scroll` | canonical vector log plus collection catalogue | governed reference-ordered page at one read stamp and valid time |
| `rrflow_vector_search` | canonical vector log plus collection catalogue | governed bounded exact dense/sparse/multi-dense search with truthful access-path reporting |

## Existing engine capability adapter sequence — 17

These tools may be implemented without inventing a new database feature. Each
must consume generated request schemas from `rrd-contract`, call the ordinary
RRD engine/client boundary, and retain authorization/idempotency/audit evidence.

| Order | Target tool | Existing authority consumed | Security/mutation rule |
|---:|---|---|---|
| A1 — implemented | `rrflow_service_status` | readiness, endpoint and product capability catalogues | read-only; no session secret returned |
| A2 — implemented | `rrflow_data_commit` | typed data transaction | mutation; required idempotency and exact preflight receipt; adapter owns lease plumbing |
| A3 — implemented | `rrflow_query_index_ensure` | query-index ensure | mutation; required idempotency and exact preflight receipt |
| A4 — implemented | `rrflow_query_index_list` | query-index catalogue | authorized read |
| A5 — implemented | `rrflow_live_query_poll` | live-query poll | bounded authorized read |
| A6 — implemented | `rrflow_changefeed_read` | retained changefeed | bounded authorized read |
| A7 — implemented | `rrflow_changefeed_follow` | bounded long-poll changefeed | bounded authorized read with timeout |
| A8 — implemented | `rrflow_vector_collection_ensure` | vector collection catalogue | mutation; required idempotency and preflight receipt |
| A9 — implemented | `rrflow_vector_collection_list` | vector collection catalogue | authorized read |
| A10 — implemented | `rrflow_vector_points_retrieve` | governed vector point retrieval | bounded authorized read |
| A11 — implemented | `rrflow_vector_points_scroll` | governed vector point scroll | bounded authorized read |
| A12 — implemented | `rrflow_vector_search` | dense/sparse/multi-dense search | bounded authorized read; reports actual access path/exactness |
| A13 — implemented | `rrflow_backup_create` | authenticated logical backup catalogue | exact-authorized mutation with required idempotency |
| A14 — implemented | `rrflow_backup_list` | authenticated backup catalogue | authorized read with optional verification |
| A15 — implemented | `rrflow_restore` | restore-to-new-root | exact-authorized mutation; explicit new-root acknowledgement and idempotency |
| A16 — implemented | `rrflow_estate_read` | persisted estate authority | authorized read |
| A17 — implemented | `rrflow_audit_read` | security audit journal | privileged bounded read |

The A-series adapter map now contains 28 executable tools with exact
generated-list/dispatch parity. Its complete exit gate additionally requires
real stdio calls for every domain, persistence/reopen tests for every
mutation, initialized-security denial/allowance tests, and matching capability
dispositions in Connectome. The number 28 is an output of this reviewed
operation map, not a product-quality claim.

### A2 executable evidence and boundary

`rrflow_data_commit` is generated from `rrd-contract`'s typed
`TransactionMutation` schema and maps to the existing `transaction-commit`
capability row; it does not create a duplicate MCP-only capability. It derives
session and transaction identities from the required caller idempotency key,
uses `RrdEngine::{create_session,begin_transaction,commit_transaction}`, and
never returns the lease token. One ordered commit may contain schema, claims,
records/documents, relations/native graph edges, events, dense/sparse/
multi-dense vectors, time-series samples, geospatial values, and object
references.

Execution requires a fresh project-attunement receipt bound to the exact tool
name and exact typed arguments. The engine closes the authorization with a
durable post-tool observation after success or failure. Initialized security
still denies the uncredentialed embedded adapter before data execution. The
recovery path recognizes an exact existing transaction before applying lease
liveness, so committed retry remains resolvable after session expiry and
process reopen.

Focused evidence covers all-engine transaction recovery, exact authorization,
multi-model commit/query, security denial, idempotent replay after reopen,
generated discovery parity, capability-row parity, and MCP stdio discovery.
This is one data commit adapter. The later A3-A12 slices now provide the listed
index, realtime, and exact named-vector operations; backups, estate, audit,
document ingest, full-text/hybrid search, distributed deployment, and the
managed cloud plane remain incomplete.

### A3 executable evidence and boundary

`rrflow_query_index_ensure` is generated from the public `EnsureQueryIndex`
contract and maps onto the existing `query-index-ensure` capability row. The
adapter reuses the shared internal session wrapper, then calls
`RrdEngine::ensure_query_index`; index definition validation, catalogue CAS,
build/rebuild, source read stamp, artifact identity, security action, and
durable idempotency remain owned by RRD.

The same integration fixture commits document data, creates a real ready index,
checks its two artifact rows, reopens RRD, advances beyond the adapter session's
absolute expiry, and proves exact ensure replay returns the retained receipt.
An expired session cannot create a new index: only an already-recorded exact
operation replay is resolved before liveness enforcement. A4 list/read remains
separate because it has different authorization and no idempotency semantics.

### A4 executable evidence and boundary

`rrflow_query_index_list` is generated from `ListQueryIndexes`, maps to the
existing `query-index-list` capability, and invokes
`RrdEngine::list_query_indexes`. It is a governed read and therefore does not
consume an exact mutation receipt. Internal read sessions are bounded and
reused by operation and maximum-lease time window; the tool schema and result
contain no lease, token, or artificial read-idempotency field. Focused evidence
lists the ready A3 index after RRD reopen and proves initialized security denies
the uncredentialed read.

### A5-A7 executable realtime evidence and boundary

`rrflow_live_query_poll`, `rrflow_changefeed_read`, and
`rrflow_changefeed_follow` are generated directly from `PollLiveQuery`,
`ReadChangefeed`, and `FollowChangefeed` and map to their existing RRD endpoint
capabilities. All are governed reads using the internal bounded read-session
path. The public contracts enforce row/page budgets and a maximum five-second
wait; timeout is an explicit successful result, not an unbounded blocked tool.

The cohesive integration commits the multi-model A2 transaction, then proves a
live RRFlowQL poll reports both document rows as additions. The retained
changefeed includes the nine mutations belonging to the exact A2 commit digest
plus the surrounding authoritative reasoning/lifecycle/trace events; it is not
an application-only shadow log. Page validation reports the bounded hash-chain
method. Following from the current head returns a bounded explicit timeout.
Existing server process coverage separately proves follow wakes on a concurrent
commit. Initialized security denies all three uncredentialed reads.

### A8-A12 executable vector evidence and boundary

`rrflow_vector_collection_ensure`, `rrflow_vector_collection_list`,
`rrflow_vector_points_retrieve`, `rrflow_vector_points_scroll`, and
`rrflow_vector_search` are generated from the existing typed RRD vector
contracts and map to the existing endpoint capability rows. The collection
mutation requires exact project attunement and idempotency; all point/search
operations are governed, bounded reads whose adapter-created sessions and
tokens remain internal.

The implementation audit found and corrected a persistence defect before the
point/search tools were advertised: public vector mutations validated
`collection_id` and `vector_name`, but lowering discarded both and physical
search relied on the shared field alone. Canonical `RuntimeVector` rows now
retain an optional validated collection/vector address. Legacy rows lacking the
additive address remain readable but cannot satisfy a collection-addressed
read. Compact dense and TurboQuant metadata preserve the address; artifact
formats that already serialize the full canonical row inherit it.

The cohesive integration creates `documents/title`, commits a collection-bound
point through the ordinary multi-model transaction, reopens RRD, and proves
retrieve, scroll, and exact search return only that point. An older unbound row
with the identical physical field is explicitly missing and cannot leak into
the result. Security-initialized RRD denies all five uncredentialed operations;
MCP discovery/dispatch parity and strict Clippy pass.

The advertised MCP search path is currently exact and truthfully reports
`exact_scan`. HNSW and TurboQuant engine artifacts exist, but collection-bound
approximate artifact selection is not exposed here until artifact selection is
explicitly bound to the collection/vector address and recall/latency evidence
passes. Full-text and hybrid retrieval also remain separate missing engine
capabilities.

### A13-A17 executable administration evidence and boundary

`rrflow_backup_create`, `rrflow_backup_list`, `rrflow_restore`,
`rrflow_estate_read`, and `rrflow_audit_read` map directly to the existing RRD
backup, restore, estate, and security-audit capability rows. Backup and restore
require exact project attunement plus stable idempotency. Restore additionally
requires the literal `restore_to_new_root` acknowledgement and accepts only a
canonical restore identity; the model cannot supply an arbitrary filesystem
path or overwrite the active database.

The cohesive administration integration persists a fact, creates a logical
backup, reopens RRD and resolves an exact replay, verifies the authenticated
catalogue, restores to the engine-owned isolated root, opens that restored
engine, and reads the backed-up fact. It also reads a real persistent estate
document and a real bounded audit record. Initialized security denies all five
uncredentialed tools before operation execution. The generated catalogue has
exactly 28 entries and every A13-A17 definition maps to the existing endpoint
capability and generated request schema.

The interactive process-level stdio suite now calls every implemented domain,
including dependent backup-to-restore execution using the returned content
digest. Connectome's capability view consumes the exact runtime-tool and
product-surface catalogues, and its parity test passes with the 28-tool map.
The remaining A-series gates are credentialed secured-daemon allowance and the
cohesive workspace/remote platform matrix.

## Missing engine capability — do not advertise yet

| Target tool | Missing engine work |
|---|---|
| `rrflow_document_ingest` | governed chunking/parsing/object closure plus atomic source and projection commit |
| `rrflow_document_retire` | recoverable source retirement and projection/index cleanup |
| `rrflow_hybrid_recall` | lexical, dense, sparse, provenance, recency, and graph fusion planner |
| `rrflow_reflect` | attributable candidate-learning workflow with approval/retirement semantics |
| `rrflow_semantic_code_search` | structural code model, embedding freshness, and bounded retrieval |
| `rrflow_schema_admin` | explicit public schema inspection/migration administration beyond raw transaction input |
| `rrflow_graph_admin` | explicit public graph traversal and administration beyond record/relation transaction input |
| `rrflow_vector_collection_delete` | audited recoverable collection/artifact retirement |
| `rrflow_embed` | model registry, artifact provenance, resource policy, batching, and governed commit |
| `rrflow_object_upload` | authenticated content upload/staging and object-complete backup closure |
| `rrflow_discover` | discovery/recommendation contract and evaluation evidence |
| `rrflow_security_admin` | principal, credential-reference, policy, and grant administration contract |

These rows remain planned in the shared capability catalogue until their engine
contract and executable evidence exist. They are not placeholders in MCP
`tools/list`.

## Adapter architecture gate

The current MCP binary embeds `RrdEngine` and therefore cannot join the
canonical multi-process development topology beside `rrd-server`. The A1-A17
tool map is executable in embedded mode; daemon transport and credentialed
allowance are a separate, still-blocked topology gate. MCP must support two
explicit, mutually exclusive modes:

- **embedded:** requires `embedded --db` and `--root`; MCP is the sole RRD authority and
  opens the bound database itself;
- **daemon:** requires `--url`, an instance identity, and an operator-owned
  credential reference; MCP uses `rrd-client` and must not open a database.

The daemon is project-bound. Its configured canonical project root, instance
identity, database binding, and attunement-source fingerprint are authoritative
for every runtime call. A model-provided path cannot select or rebind a project.
The server exposes typed runtime catalogue and invocation operations through
the same authenticated session, policy, idempotency, transaction, audit, and
event paths as its data APIs. A generic arbitrary-tool or arbitrary-shell
endpoint is forbidden.

An authenticated caller session is propagated through runtime execution.
High-level tool adapters may manage internal transaction leases, but they may
not replace the caller with an anonymous embedded session. Each runtime tool
maps to a granular policy action; one blanket `runtime.invoke` grant is not
sufficient authorization for data mutation, restore, audit, or security work.

Security initialization must make uncredentialed MCP calls fail closed. Local
development credentials come from an operator-owned credential reference or
file configured at process start, never from model-authored tool arguments and
never from a checked-in environment file.

The full contract and dependency-ordered implementation gates are frozen in
[`rrflow-mcp-daemon-mode.md`](rrflow-mcp-daemon-mode.md).

D0 is now implemented locally: the public contract contains a bounded,
versioned runtime-tool catalogue, exact arguments/result digest types, and ten
dedicated memory/lifecycle/project/reasoning security actions. The engine's 28
definitions each carry an explicit action and generate the validated transport
catalogue. Exhaustiveness tests freeze every name-to-action mapping. No daemon
route or authenticated daemon execution is claimed by that contract slice.

## Local and remote setup

No additional toolchain installation is currently required on the development
host. Rust/Cargo 1.98.0, Git, Tailscale, and `cloudflared` are present.
Tailscale is online and already serves Connectome port 4387. A browser must be
signed into the same tailnet to use the stable private URL. A stable public URL
requires a user-owned DNS zone plus one-time Cloudflare Tunnel authorization;
the temporary quick tunnel is not a release or development-environment
dependency.
