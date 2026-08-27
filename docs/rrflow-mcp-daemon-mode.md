# RRFlow MCP embedded/daemon contract

**Status:** authoritative pre-release implementation gate as of 2026-08-25.
D0 through D4 are implemented and locally verified. Embedded MCP and the
loopback authenticated daemon-client profile are executable. Remote mTLS,
every-domain daemon differentials, and D5 topology qualification remain
incomplete and must not be advertised as complete.

## Purpose

RRFlow has one RRD authority for one database. MCP is an adapter to that
authority, not another database process. The selected deployment profile must
therefore be structurally unambiguous:

| Mode | Sole RRD authority | MCP inputs | Forbidden |
|---|---|---|---|
| Embedded | `rrflow-mcp` through `RrdEngine` | `embedded --db PATH --root PROJECT` | `--url`, daemon credentials, a second opener |
| Daemon | `rrd-server` | `daemon --url http://LOOPBACK:PORT --instance ID --principal ID --api-key-file ABSOLUTE_PATH` | `--db`, direct `RrdEngine`, inline credentials, model-controlled project paths |

Argument parsing rejects missing inputs, mixed modes, unknown arguments, and
credential material supplied inline. Mode selection happens once before MCP
stdio processing begins and cannot change during the process.

## One invocation contract

The engine owns one typed runtime-tool catalogue and one typed invocation
dispatcher. Embedded mode calls it in process. Daemon mode obtains the same
catalogue and calls the same dispatcher through authenticated RRD protocol
operations. MCP `tools/list` never owns a handwritten list in either mode.

The daemon invocation request binds:

- protocol and runtime-tool catalogue versions;
- instance, project, principal, session, request, operation, and correlation
  identities;
- exact tool name and schema-validated arguments digest;
- project-attunement receipt and reasoning attempt for mutations;
- idempotency key where the underlying operation requires it;
- deadline and request-body bounds.

The response carries the tool result plus authoritative operation, read-stamp,
audit, trace, and lifecycle identities. It never returns bearer tokens or
internal transaction leases.

## Project and security authority

`rrd-server` starts with a validated `InstanceBinding`: canonical project root,
database path, instance identity, and persisted binding digest. Runtime calls
use that server-side binding. The MCP client does not transmit a filesystem
root on each call and cannot cause the daemon to inspect another project.

Daemon mode opens an ordinary credentialed RRD session from an operator-owned
credential reference. Credential bytes are read at process start, are never
included in tool schemas or model-authored arguments, are never written to the
runtime journal, and are redacted from diagnostics. Remote mode requires the
existing mutually authenticated TLS transport policy.

Every tool resolves to a granular `SecurityAction`. Catalogue discovery may be
public only to the degree allowed by the service capability policy. Reads,
data commits, index changes, vector changes, backup, restore, estate, audit,
and future security administration cannot share an unrestricted catch-all
grant. Runtime execution must retain the authenticated caller; nested adapter
sessions may manage leases but cannot downgrade or replace that principal.

## Failure and lifecycle semantics

- Unknown tools, schema drift, stale catalogue versions, stale attunement,
  expired sessions, missing grants, and project-binding mismatch fail closed.
- Mutation authorization is consumed once and bound to exact arguments,
  project revision, working state, and reasoning attempt.
- Timeout or transport loss has an indeterminate outcome until the client
  resolves the authoritative operation identity; retries use idempotency rather
  than blind re-execution.
- The same durable runtime, audit, changefeed, trace, and reasoning events are
  produced in embedded and daemon modes.
- MCP remains cooperative unless its host provides an intercepting or
  orchestrated boundary. Daemon transport does not falsely upgrade enforcement.

## Conformance gates

Daemon mode is complete only when black-box tests prove:

1. Embedded and daemon `tools/list` have identical names and schemas.
2. Every executable domain produces equivalent logical results and event
   identities in both modes.
3. A daemon-mode MCP process never opens or locks the database path.
4. Initialized security denies an uncredentialed caller and permits only the
   explicitly granted operations of a credentialed caller.
5. Project/root substitution, stale attunement, cross-instance credentials,
   mixed CLI modes, and catalogue drift are denied.
6. Mutation retry after response loss resolves the retained operation without
   duplicate effects, including after server and MCP restart.
7. Loopback development and remote mTLS profiles pass the same protocol suite.
8. `rrflow dev doctor` observes the real daemon-client dependency and explicit
   runtime mode rather than accepting dependency hygiene as transport proof.

## Dependency-ordered implementation slices

| Slice | Implementation | Exit evidence |
|---:|---|---|
| D0 — implemented locally | Freeze runtime catalogue/invocation protocol types, error vocabulary, and granular security-action mapping in `rrd-contract`/engine | schema and action exhaustiveness tests; no generic authorization bypass |
| D1 — implemented locally | Bind `rrd-server` to one `InstanceBinding` and persisted project authority; make estate and Kubernetes provision that identity before serving | wrong-root/rebind/reopen tests plus real estate-child crash matrix and generated Kubernetes contract tests |
| D2 — implemented locally | Make engine runtime dispatch accept and preserve an authenticated caller context | API-key/session allowance, policy denial, payload-drift, nested-session, and exact audit-principal tests |
| D3 — implemented locally | Add bounded authenticated runtime catalogue/invoke routes and `rrd-client` methods | project-bound real-server catalogue/invocation, granular denial/audit, generated-protocol drift, and child-process server tests |
| D4 — implemented locally | Add mutually exclusive embedded/daemon runtime authority to `rrflow-mcp` | parser rejects mixed modes; black-box secured daemon process proves one server opener, exact catalogue parity, authenticated invocation, clean close, and principal audit |
| D5 — topology implemented; surface parity remains | Follow `docs/d5-surface-absorption-map.md`: absorb Connectome/CLI physical access behind engine/protocol contracts, then implement the proxy, supervisor, and full-topology CI smoke | zero-bypass behavior parity plus `cargo rrflow-dev doctor` green on the same tested commit |

No URL-only shortcut, compatibility shim, duplicate tool registry, anonymous
server-side invocation, or second database opener satisfies these gates.

### D3 protocol result

The public endpoint catalogue now contains 31 HTTP operations. Two are the
daemon runtime boundary:

- `POST /v1/runtime/tools/list` requires a session and the dedicated
  `runtime_tool_catalogue_read` grant;
- `POST /v1/runtime/tools/invoke` requires a session, resolves the selected
  tool against the engine-owned 28-tool catalogue, derives its actual mutation
  bit and granular `SecurityAction`, and uses only the server's persisted
  project root.

`EndpointAction` explicitly distinguishes fixed-action endpoints from the
runtime-tool-derived invocation endpoint. There is no grantable
`runtime_tool_dispatch` action. The Rust client requires a validated fetched
catalogue before invocation; the server independently resolves it again.
TypeScript, Python, Go, Java, and .NET generated endpoint maps contain the two
operations. Local TypeScript/Python/Go checks pass. Java and .NET generation
drift checks pass, while build/test execution is unverified on this host
because Maven and the .NET SDK are absent.

### D4 MCP authority result

Mode is a required positional choice, not inferred from whichever flags happen
to be present. Embedded mode validates the database against the discovered
project binding. Daemon mode accepts no database or project path. It:

1. accepts loopback HTTP only in the currently qualified local profile;
2. reads a bounded, owner-only, visible-ASCII API key from an absolute path;
3. creates a principal-bound RRD session and fetches the server catalogue;
4. rejects startup if that catalogue differs from the MCP build;
5. invokes every call through `rrd-client` with descriptor-derived
   idempotency, retry, deadline, and granular authorization semantics;
6. re-authenticates after an expired session and rejects catalogue drift;
7. closes its session without ever opening or receiving the database path.

The black-box daemon test starts one secured, project-bound `rrd-server`, then
runs the MCP process using only URL/instance/principal/credential-reference.
It proves 28-tool name/schema parity, a real authenticated service-status
result, clean session close, and the original MCP principal in durable audit.
The development doctor now derives this as a passing boundary check. Full
mutation-domain parity, response-loss replay, and remote mTLS remain explicit
D5/full-conformance work. The supervised RRD + authenticated Connectome
black-box smoke is implemented; it does not substitute for those differentials.
