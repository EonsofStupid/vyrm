# RRD vector collection and point contract v1

RRD's vector subsystem is one branch of the authoritative runtime, not a
sidecar vector database. Collection administration lives in the revisioned
per-scope vector catalogue. Point values, payloads, and retirements live in the
same authenticated runtime commit log as records, relations, events, claims,
series, geo values, and objects.

## Collection authority

A collection contains one to 64 uniquely named vector definitions. Every
definition fixes its canonical field, dense/sparse/multi-dense value shape,
dimensions, metric, optional embedding-model digest, and declared memory tier.
The catalogue is updated through compare-and-swap transitions, advances the
scope catalogue revision, enters the authenticated control journal, and keeps
bounded content-bound idempotency receipts across reopen.

Collection deletion is deliberately fail-closed. Before removing the
catalogue entry, the engine proves within the caller's scan bound that every
latest collection-bound vector version ends at or before the requested
deletion time. It also refuses deletion while an HNSW or TurboQuant artifact
for any named vector remains active. Callers therefore retire points in one
ordinary transaction and retire governed artifacts before deleting metadata;
RRD never makes live or future point data unreachable by silently dropping its
collection definition. Historical runtime mutations remain retained.

## Point mutation authority

`CommitTransaction` is the batch point write API. One transaction may carry
multiple `put_vector` mutations alongside every other logical model. Each
collection-bound vector resolves its collection and name before commit and
must match the configured field, shape, dimensions, model provenance, and
payload-index types. The entire batch commits or none of it does.

Point deletion uses `retire_data` with the `vector` model in the same batch
transaction vocabulary. Retirement is a modeled valid-time fact rather than a
physical rewrite, so historical reads remain reproducible. Retrieve and scroll
resolve the same collection catalogue and materialize the same read-stamped,
transaction-visible, valid-time vector versions used by exact search.

## Payload-index lifecycle

Each collection may own up to 256 indexes over canonical point payload fields.
V1 admits exact RRD scalar kinds: boolean, integer, unsigned, decimal, keyword
(string), and digest. Absent properties are allowed; present indexed values
must have the declared type. Ensure, list, and delete operations use distinct
deny-by-default security actions. Ensure/delete transitions advance collection
generation and catalogue revision, retain configuration digests and timestamps,
and replay exactly from durable operation receipts after restart.

HNSW and TurboQuant filter-property configurations may reference only active
payload indexes. G04-W02 owns using those properties during persistent HNSW
traversal and its fixed selectivity/recall differential; G04-W04 owns complete
artifact retirement/reclamation. G06 owns generated HTTP, MCP, CLI, and SDK
bindings for the engine administration methods.

## Bounded failure semantics

- unknown collections, vectors, or payload indexes fail without mutation;
- changed payloads under one idempotency key conflict;
- receipt, catalogue, generation, and digest overflow fail closed;
- scan-budget exhaustion prevents collection deletion;
- live/future points or active approximate artifacts prevent deletion;
- payload values that differ from an active index kind prevent the entire
  transaction from committing;
- corrupted catalogue identities, receipts, timestamps, or digests prevent
  reopen rather than silently discarding state.
