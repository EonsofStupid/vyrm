# Vyrm / SurrealDB claim-runtime differential

Status: first local diagnostic; not a general database-superiority claim.

The executable harness
[`eval/surrealdb_claim_differential.py`](../eval/surrealdb_claim_differential.py)
compares one shared primitive rather than unrelated feature lists:

1. identical typed claim fields;
2. atomic authoritative batches with an authoritative sequence watermark;
3. a separate sequence reference for every claim;
4. bounded ordered replay;
5. complete paged object verification before timing is accepted; and
6. clean stop, restart, verification, RSS, and allocated-disk accounting.

Vyrm runs embedded in an isolated benchmark child. SurrealDB runs as a local
SurrealKV server through a persistent HTTP SQL connection. That topology is an
intentional product comparison but not an engine-only comparison, so the
harness retains both end-to-end time and the sum of SurrealDB's own reported
statement-execution times. Surreal process RSS excludes the Python client while
Vyrm RSS includes its small Rust harness; that asymmetry favors Surreal and is
recorded in the evidence contract.

The first three-trial diagnostic pins SurrealDB 3.0.5 and its local binary
SHA-256. At 2,048 claims, 64-claim batches, 512 bounded reads, and width 32,
both systems passed full verification. Vyrm measured:

| Cell | Vyrm / SurrealDB | Interpretation |
|---|---:|---|
| Write throughput, end-to-end | 57.08× | Vyrm higher |
| Read throughput, end-to-end | 895.61× | Vyrm higher |
| Write throughput, Surreal server time | 10.19× | Vyrm higher |
| Read throughput, Surreal server time | 17.50× | Vyrm higher |
| Write p95, end-to-end | 0.031× | Vyrm lower |
| Read p95, end-to-end | 0.001× | Vyrm lower |
| Clean-start readiness | 0.005× | Vyrm lower |
| Reopened process RSS | 0.060× | Vyrm lower |
| Reopened allocated bytes | 1.380× | SurrealDB lower |

The bounded verdict is therefore **not** “all measured cells favor Vyrm.” The
disk result is a useful lifecycle finding: this Vyrm profile reopens from its
authoritative WAL without forced maintenance, while its explicitly maintained
segment is much smaller. The comparison must not substitute Vyrm's maintained
state unless an equivalent Surreal maintenance boundary is measured.

SurrealDB's own documentation describes SurrealKV as a local/embedded storage
choice and says SurrealDB 3.x enables disk sync by default. Its benchmark page
also requires durable persistent comparisons. Those are the invariants used
here: [storage engines](https://surrealdb.com/docs/build/embedding/storage-engines),
[performance methodology](https://surrealdb.com/benchmarks), and
[SurrealDB repository](https://github.com/surrealdb/surrealdb).

Next gates are nine alternating trials, fixed hardware/CPU state, a larger
corpus that crosses Vyrm's maintenance threshold, equivalent maintained-state
accounting, and separate graph/query/vector workloads. Until those exist, this
evidence supports only the claim-runtime row above; SurrealDB's broader query,
graph, live-query, full-text, and vector features are out of scope.

Evidence:
[`2026-08-23-vyrm-surrealdb-3.0.5-claim-diagnostic-v1.json`](../eval/results/2026-08-23-vyrm-surrealdb-3.0.5-claim-diagnostic-v1.json).
