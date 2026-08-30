# RRD governed functions and triggers v1

## Authority and compatibility

`rrd-engine` is the only function execution and trigger authority. Its
automation catalogue is a revisioned engine-owned subcatalogue of the one
instance/database authority, not a separately composable database. Function
definitions do not open storage, publish their own commit cursors, or bypass
the engine's session, policy, transaction, and audit paths.

The wire-independent contract is `FUNCTION_CONTRACT_VERSION = 1`. Every
catalogue carries that version, a monotonic revision, digested function
content, explicit limits, declared capabilities, and sorted function and
trigger identities. A replacement publishes the immutable revision record and
its compare-and-swap head in one control batch. Restart resolves the head and
validates the stored revision and catalogue SHA-256 before any execution.

Changing source bytes, decoded WebAssembly bytes, an identifier, limit,
capability, trigger, effect, or retry field changes the catalogue digest. A
transaction commit intent pins the exact catalogue revision and derived runtime
commit digest. Recovery reloads that immutable revision; it never substitutes
the current head.

## Runtime contracts

Both runtimes receive the same natural JSON domain. Input and output allow at
most 64 nested containers and 8,192 total values in addition to the byte bound.
Integers are limited to the exact interoperable JSON/JavaScript range
`-9007199254740991..=9007199254740991`; larger signed or unsigned values fail
closed instead of silently losing precision. Decimal strings must round-trip to
the same canonical JSON number. A later contract version may add an explicitly
tagged arbitrary-precision numeric ABI without changing v1 behavior.

### JavaScript ES2020

The source must evaluate to one synchronous function. It receives one natural
JSON value and must return a JSON-serializable value. Each call gets a fresh
QuickJS runtime and context. The engine fixes input/output byte limits, heap
bytes, stack bytes, and an interrupt-callback budget.

The runtime exposes no engine host functions, filesystem, network, module
loader, clock, locale, or randomness. `Date`, `Intl`, `eval`, `Function`, and
`Math.random` are disabled before user source executes. Promise-like output is
rejected because v1 is synchronous.

### Portable WebAssembly JSON v1

The module must export:

```text
memory
rrd_alloc(input_len: i32) -> i32
rrd_run(input_ptr: i32, input_len: i32) -> i64
```

The engine writes UTF-8 JSON into the allocation. `rrd_run` packs the output
pointer in the high 32 bits and the output length in the low 32 bits. The
output must be UTF-8 JSON and fit the declared bound.

Wasmi uses eager compilation, deterministic instruction fuel, bounded value and
call stacks derived from `stack_bytes`, no reusable stack cache, and store
limits for memory and instance count. Imports are rejected, so v1 has no WASI,
clock, randomness, filesystem, network, or other host capability.

## Trigger and transaction semantics

A trigger selects one of the ten public typed transaction mutations and may
optionally narrow record-like mutations by kind. Trigger order is canonical:
original mutation order followed by trigger identity order. Generated effects
are not re-matched, which prevents recursive trigger chains.

Two effects exist:

- `require_true` requires the function result to be exactly boolean `true`.
  Any other result rejects the complete transaction before publication.
- `append_event` requires an object result and the function's explicit
  `emit_event` capability. The engine lowers it to one typed event in the same
  `RuntimeCommit` as the triggering data.

The input contains the original typed mutation, its zero-based index, and the
transaction's frozen runtime time. Schema validation and uniqueness checks run
against the final derived commit. Either every original and derived mutation
publishes under one cursor, or no data mutation publishes.

## Security, audit, retry, and failures

Catalogue reads, catalogue replacement, and execution map to distinct
deny-by-default `SecurityAction` values. A secured transaction additionally
requires `function_execute` when its pinned catalogue contains triggers.
Trigger execution records authorized and terminal allowed/failed audit
evidence with request, operation, principal, source input, and output/error
digests; function bodies, credentials, and raw transport data are not audit
payloads.

V1 has exactly one function attempt. A deterministic runtime error is terminal
and is never blindly retried inside the runtime. The enclosing idempotent
transaction is the only retry boundary: it replays the frozen time, catalogue
revision, mutation input, and expected derived commit digest. A different
idempotency key, operation digest, catalogue revision, or derived result fails
closed.

Contract violations, missing identities/revisions, permission denials, runtime
errors, and resource-limit exhaustion remain distinct engine error kinds.
Resource exhaustion, malformed JSON, ABI mismatch, imports, invalid pointers,
false validators, and append-event type mismatches cannot advance the data
cursor.

Sandbox violations in function content, such as a WebAssembly import, are
runtime failures rather than authorization denials. `permission_denied` is
reserved for the principal/action/resource policy decision. A missing immutable
revision during transaction recovery is a failed precondition, not a missing
user function.

## Surface boundary

The native engine entry points are available in v1. G06 owns generated HTTP,
MCP, CLI, SDK, and Connectome projection from this contract. No adapter may
embed a second runtime or maintain a competing function catalogue.
