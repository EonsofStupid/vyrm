import assert from "node:assert/strict";
import test from "node:test";
import { RrdApiError, RrdClient, RrdClientError } from "../src/index.js";

const jsonResponse = (value: unknown, status = 200) =>
  new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });

test("public negotiation retries transport loss and validates the ArkType envelope", async () => {
  let attempts = 0;
  const transport: typeof fetch = async (input, init) => {
    attempts += 1;
    assert.equal(new URL(input.toString()).pathname, "/v1/capabilities");
    assert.equal(init?.method, "GET");
    if (attempts === 1) throw new TypeError("connection reset");
    return jsonResponse({
      protocol: "rrd",
      protocol_version: 1,
      request_id: "server-request",
      operation_id: "server-operation",
      outcome: {
        status: "ok",
        payload: {
          protocol: "rrd",
          protocol_version: 1,
          implementation: "rrd-server",
          implementation_version: "0.1.0",
          deployment_mode: "local_daemon",
          instance: { kind: "instance", id: "sdk-test" },
          capabilities: [],
        },
      },
    });
  };
  const client = new RrdClient({
    baseUrl: "http://127.0.0.1:9477",
    instance: "sdk-test",
    fetch: transport,
  });
  const capabilities = await client.capabilities();
  assert.equal(capabilities.protocol_version, 1);
  assert.equal(attempts, 2);
});

test("session and query calls construct authenticated bounded v1 envelopes", async () => {
  const requests: { url: URL; init: RequestInit }[] = [];
  const transport: typeof fetch = async (input, init = {}) => {
    const url = new URL(input.toString());
    requests.push({ url, init });
    const envelope = JSON.parse(String(init.body)) as {
      context: { request_id: string; operation_id: string };
    };
    const payload =
      url.pathname === "/v1/sessions"
        ? {
            session_id: "session-1",
            token: "token-1",
            issued_at_unix_ms: 100,
            idle_expires_at_unix_ms: 60_100,
            absolute_expires_at_unix_ms: 300_100,
            limits: {
              idle_timeout_ms: 60_000,
              absolute_timeout_ms: 300_000,
              max_open_transactions: 2,
            },
          }
        : {
            canonical_query: "FROM record:document",
            scope: "instance:sdk-test",
            read_manifest_sha256: "a".repeat(64),
            known_at_cursor: 2,
            schema_revision: 1,
            plan: {
              plan_sha256: "b".repeat(64),
              exact: true,
              deterministic_order: "identity",
              authorization_boundary: "instance:sdk-test",
              candidates: [],
            },
            execution: {
              scanned_changes: 2,
              stamp_validation: "exact",
              stamp_validation_max_changes: 2,
              stamp_validation_proof_nodes: 1,
              returned_rows: 0,
              output_bytes: 2,
              truncated: false,
            },
            rows: [],
          };
    return jsonResponse({
      protocol: "rrd",
      protocol_version: 1,
      request_id: envelope.context.request_id,
      operation_id: envelope.context.operation_id,
      outcome: { status: "ok", payload },
    });
  };
  const client = new RrdClient({
    baseUrl: "http://localhost:9477",
    instance: "sdk-test",
    fetch: transport,
  });
  const session = await client.createSession(
    "typescript-sdk",
    "not-persisted",
    {
      limits: {
        idle_timeout_ms: 60_000,
        absolute_timeout_ms: 300_000,
        max_open_transactions: 2,
      },
    },
    {
      requestId: "request-session",
      operationId: "operation-session",
      idempotencyKey: "session-key",
    },
  );
  const query = await client.call(
    "query-execute",
    {
      scope: "instance:sdk-test",
      query: "FROM record:document",
      parameters: {},
      budget: {
        max_scanned_changes: 100,
        max_rows: 10,
        max_output_bytes: 4_096,
        max_batch_rows: 10,
      },
    },
    { requestId: "request-query", operationId: "operation-query", session },
  );
  assert.equal(query.known_at_cursor, 2);
  assert.equal(new Headers(requests[0]?.init.headers).get("authorization"), "ApiKey not-persisted");
  assert.equal(new Headers(requests[1]?.init.headers).get("authorization"), "Bearer token-1");
  const queryEnvelope = JSON.parse(String(requests[1]?.init.body));
  assert.deepEqual(queryEnvelope.resource.segments, [{ kind: "instance", id: "sdk-test" }]);
});

test("the client rejects remote cleartext, expired deadlines, and typed API errors", async () => {
  assert.throws(
    () => new RrdClient({ baseUrl: "http://192.0.2.1:9477", instance: "sdk-test" }),
    RrdClientError,
  );
  const client = new RrdClient({
    baseUrl: "http://127.0.0.1:9477",
    instance: "sdk-test",
    maxAttempts: 1,
    fetch: async (_input, init = {}) => {
      const envelope = JSON.parse(String(init.body));
      return jsonResponse(
        {
          protocol: "rrd",
          protocol_version: 1,
          request_id: envelope.context.request_id,
          operation_id: envelope.context.operation_id,
          outcome: {
            status: "error",
            error: {
              code: "permission_denied",
              message: "policy denied",
              retryable: false,
              details: {},
            },
          },
        },
        403,
      );
    },
  });
  const fakeSession = {
    principalId: "typescript-sdk",
    lease: {
      session_id: "session-1",
      token: "token-1",
      issued_at_unix_ms: 1,
      idle_expires_at_unix_ms: 2,
      absolute_expires_at_unix_ms: 3,
      limits: { idle_timeout_ms: 1_000, absolute_timeout_ms: 1_000, max_open_transactions: 1 },
    },
  };
  await assert.rejects(
    client.call(
      "audit-read",
      { after_sequence: 0, limit: 10 },
      { requestId: "request-audit", operationId: "operation-audit", session: fakeSession },
    ),
    (error) => error instanceof RrdApiError && error.code === "permission_denied",
  );
  await assert.rejects(
    client.call(
      "audit-read",
      { after_sequence: 0, limit: 10 },
      {
        requestId: "request-expired",
        operationId: "operation-expired",
        deadlineUnixMs: 1,
        session: fakeSession,
      },
    ),
    /deadline has expired/,
  );
});
