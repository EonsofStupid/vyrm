import { type } from "arktype";
import { endpoints, type OperationId } from "./generated/endpoints.js";
import type { operations, paths } from "./generated/rrd-openapi.js";

export type { OperationId, operations, paths };

export type RequestPayload<K extends OperationId> = operations[K] extends {
  requestBody: { content: { "application/json": infer Envelope } };
}
  ? Envelope extends { payload: infer Payload }
    ? Payload
    : never
  : undefined;

export type SuccessPayload<K extends OperationId> = operations[K] extends {
  responses: { 200: { content: { "application/json": infer Envelope } } };
}
  ? Envelope extends { outcome: infer Outcome }
    ? Outcome extends { status: "ok"; payload: infer Payload }
      ? Payload
      : never
    : never
  : never;

export interface Session {
  principalId: string;
  lease: SuccessPayload<"session-create">;
}

export interface ResourceSegment {
  kind:
    | "organization"
    | "estate"
    | "project"
    | "instance"
    | "node"
    | "shard"
    | "collection"
    | "table"
    | "record"
    | "transaction"
    | "snapshot"
    | "backup"
    | "operation";
  id: string;
}

export interface RequestOptions {
  requestId?: string;
  operationId?: string;
  idempotencyKey?: string;
  deadlineUnixMs?: number;
  pathParameters?: Readonly<Record<string, string>>;
  resource?: readonly ResourceSegment[];
  session?: Session;
  apiKey?: { principalId: string; credential: string };
  signal?: AbortSignal;
}

export interface ClientConfig {
  baseUrl: string;
  instance: string;
  requestTimeoutMs?: number;
  maxAttempts?: number;
  maxResponseBytes?: number;
  fetch?: typeof globalThis.fetch;
}

const apiErrorBody = type({
  code: "string",
  message: "string",
  retryable: "boolean",
  "details?": "object",
  "+": "reject",
});
const responseOutcome = type.or(
  type({ status: "'ok'", payload: "unknown", "+": "reject" }),
  type({ status: "'error'", error: apiErrorBody, "+": "reject" }),
);
const responseEnvelope = type({
  protocol: "'rrd'",
  protocol_version: "1",
  request_id: "string",
  operation_id: "string",
  outcome: responseOutcome,
  "+": "reject",
});

export class RrdClientError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "RrdClientError";
  }
}

export class RrdApiError extends RrdClientError {
  readonly status: number;
  readonly code: string;
  readonly retryable: boolean;
  readonly details: object | undefined;

  constructor(status: number, error: typeof apiErrorBody.infer) {
    super(`RRD API ${status}: ${error.code}: ${error.message}`);
    this.name = "RrdApiError";
    this.status = status;
    this.code = error.code;
    this.retryable = error.retryable;
    this.details = error.details;
  }
}

export class RrdClient {
  readonly instance: string;
  readonly baseUrl: URL;
  readonly requestTimeoutMs: number;
  readonly maxAttempts: number;
  readonly maxResponseBytes: number;
  readonly transport: typeof globalThis.fetch;

  constructor(config: ClientConfig) {
    this.instance = canonicalId(config.instance, "instance");
    this.baseUrl = loopbackUrl(config.baseUrl);
    this.requestTimeoutMs = boundedInteger(config.requestTimeoutMs ?? 5_000, 1, 300_000, "timeout");
    this.maxAttempts = boundedInteger(config.maxAttempts ?? 2, 1, 8, "max attempts");
    this.maxResponseBytes = boundedInteger(
      config.maxResponseBytes ?? 4 * 1024 * 1024,
      1,
      16 * 1024 * 1024,
      "response limit",
    );
    this.transport = config.fetch ?? globalThis.fetch;
    if (!this.transport) throw new RrdClientError("a Fetch API implementation is required");
  }

  async capabilities(): Promise<SuccessPayload<"capabilities-read">> {
    const capabilities = await this.call("capabilities-read", undefined, {});
    if (
      capabilities.protocol !== "rrd" ||
      capabilities.protocol_version !== 1 ||
      capabilities.instance.id !== this.instance
    ) {
      throw new RrdClientError("RRD capability protocol or instance identity differs");
    }
    return capabilities;
  }

  endpointCatalogue(): Promise<SuccessPayload<"endpoint-catalogue">> {
    return this.call("endpoint-catalogue", undefined, {});
  }

  openApi(): Promise<SuccessPayload<"openapi-read">> {
    return this.call("openapi-read", undefined, {});
  }

  async createSession(
    principalId: string,
    credential: string,
    payload: RequestPayload<"session-create">,
    options: Omit<RequestOptions, "apiKey" | "session">,
  ): Promise<Session> {
    const canonicalPrincipal = canonicalId(principalId, "principal");
    if (!credential) throw new RrdClientError("API-key credential must not be empty");
    const lease = await this.call("session-create", payload, {
      ...options,
      apiKey: { principalId: canonicalPrincipal, credential },
    });
    return { principalId: canonicalPrincipal, lease };
  }

  async call<K extends OperationId>(
    operation: K,
    payload: RequestPayload<K>,
    options: RequestOptions,
  ): Promise<SuccessPayload<K>> {
    const descriptor = endpoints[operation];
    const requestContext =
      descriptor.method === "GET" ? undefined : context(options, descriptor.mutation);
    const headers = new Headers({ Accept: "application/json" });
    if (descriptor.authentication === "api_key") {
      if (!options.apiKey) throw new RrdClientError(`${operation} requires API-key authentication`);
      headers.set("X-RRD-Principal", canonicalId(options.apiKey.principalId, "principal"));
      headers.set("Authorization", `ApiKey ${options.apiKey.credential}`);
    } else if (descriptor.authentication === "session_bearer") {
      if (!options.session) throw new RrdClientError(`${operation} requires a session`);
      headers.set("X-RRD-Session", options.session.lease.session_id);
      headers.set("Authorization", `Bearer ${options.session.lease.token}`);
    }
    const url = new URL(resolvePath(descriptor.path, options.pathParameters), this.baseUrl);
    let body: string | undefined;
    if (descriptor.method !== "GET") {
      headers.set("Content-Type", "application/json");
      body = JSON.stringify({
        protocol: "rrd",
        protocol_version: 1,
        context: requestContext,
        resource: {
          segments: options.resource
            ? validateResource(options.resource)
            : defaultResource(this.instance, options.pathParameters),
        },
        payload,
      });
    }

    const attempts =
      descriptor.method === "GET" || !descriptor.mutation || options.idempotencyKey
        ? this.maxAttempts
        : 1;
    let lastError: unknown;
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      const remaining = remainingTimeout(this.requestTimeoutMs, options.deadlineUnixMs);
      const timeoutSignal = AbortSignal.timeout(remaining);
      const signal = options.signal
        ? AbortSignal.any([options.signal, timeoutSignal])
        : timeoutSignal;
      try {
        const response = await this.transport(url, {
          method: descriptor.method,
          headers,
          ...(body === undefined ? {} : { body }),
          signal,
        });
        const encoded = await readBounded(response, this.maxResponseBytes);
        let decoded: unknown;
        try {
          decoded = JSON.parse(encoded);
        } catch (error) {
          throw new RrdClientError(
            error instanceof Error
              ? `RRD response JSON is invalid: ${error.message}`
              : "RRD response JSON is invalid",
          );
        }
        const validated = responseEnvelope(decoded);
        if (validated instanceof type.errors) {
          throw new RrdClientError(`RRD response envelope is invalid: ${validated.summary}`);
        }
        if (
          requestContext &&
          (validated.request_id !== requestContext.request_id ||
            validated.operation_id !== requestContext.operation_id)
        ) {
          throw new RrdClientError("RRD response request/operation identity differs");
        }
        if (response.ok !== (validated.outcome.status === "ok")) {
          throw new RrdClientError("RRD HTTP status and typed outcome disagree");
        }
        if (validated.outcome.status === "error") {
          throw new RrdApiError(response.status, validated.outcome.error);
        }
        return validated.outcome.payload as SuccessPayload<K>;
      } catch (error) {
        if (error instanceof RrdApiError || error instanceof RrdClientError) throw error;
        lastError = error;
        if (attempt + 1 === attempts || options.signal?.aborted) break;
      }
    }
    throw new RrdClientError(
      lastError instanceof Error
        ? `RRD transport failed: ${lastError.message}`
        : "RRD transport failed",
    );
  }
}

function context(options: RequestOptions, mutation: boolean) {
  const requestId = correlationId(options.requestId, "request ID");
  const operationId = correlationId(options.operationId, "operation ID");
  const idempotencyKey = options.idempotencyKey
    ? correlationId(options.idempotencyKey, "idempotency key")
    : undefined;
  if (mutation && !idempotencyKey)
    throw new RrdClientError("mutating requests require an idempotency key");
  if (
    options.deadlineUnixMs !== undefined &&
    (!Number.isSafeInteger(options.deadlineUnixMs) || options.deadlineUnixMs <= 0)
  ) {
    throw new RrdClientError("deadline must be a positive safe Unix millisecond integer");
  }
  return {
    request_id: requestId,
    operation_id: operationId,
    ...(idempotencyKey ? { idempotency_key: idempotencyKey } : {}),
    ...(options.deadlineUnixMs ? { deadline_unix_ms: options.deadlineUnixMs } : {}),
  };
}

function defaultResource(
  instance: string,
  parameters: Readonly<Record<string, string>> | undefined,
) {
  const segments: ResourceSegment[] = [];
  if (parameters?.estate)
    segments.push({ kind: "estate", id: canonicalId(parameters.estate, "estate") });
  segments.push({ kind: "instance", id: instance });
  return segments;
}

function validateResource(resource: readonly ResourceSegment[]): ResourceSegment[] {
  if (resource.length === 0 || resource.length > 16) {
    throw new RrdClientError("resource paths must contain 1..=16 segments");
  }
  const kinds = new Set<string>();
  return resource.map((segment) => {
    if (kinds.has(segment.kind)) throw new RrdClientError("resource paths must not repeat a kind");
    kinds.add(segment.kind);
    return { kind: segment.kind, id: canonicalId(segment.id, `${segment.kind} resource`) };
  });
}

function resolvePath(
  template: string,
  parameters: Readonly<Record<string, string>> | undefined,
): string {
  return template.replace(/\{([a-z]+)\}/g, (_, name: string) => {
    const value = parameters?.[name];
    if (!value) throw new RrdClientError(`missing path parameter ${name}`);
    return encodeURIComponent(correlationId(value, `${name} path parameter`));
  });
}

function correlationId(value: string | undefined, label: string): string {
  if (!value || value.length > 128 || !/^[A-Za-z0-9._:-]+$/.test(value)) {
    throw new RrdClientError(`${label} is not a canonical RRD correlation ID`);
  }
  return value;
}

function canonicalId(value: string, label: string): string {
  if (value.length > 128 || !/^[a-z0-9][a-z0-9._-]*$/.test(value)) {
    throw new RrdClientError(`${label} is not a canonical RRD identifier`);
  }
  return value;
}

function loopbackUrl(value: string): URL {
  const url = new URL(value);
  if (
    url.protocol !== "http:" ||
    !["localhost", "127.0.0.1", "[::1]"].includes(url.hostname) ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  ) {
    throw new RrdClientError(
      "RRD TypeScript client permits only credential-free loopback HTTP before TLS qualification",
    );
  }
  if (!url.pathname.endsWith("/")) url.pathname += "/";
  return url;
}

function boundedInteger(value: number, minimum: number, maximum: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    throw new RrdClientError(`${label} must be an integer in ${minimum}..=${maximum}`);
  }
  return value;
}

function remainingTimeout(configured: number, deadline: number | undefined): number {
  if (deadline === undefined) return configured;
  const remaining = deadline - Date.now();
  if (remaining <= 0) throw new RrdClientError("RRD request deadline has expired");
  return Math.min(configured, remaining);
}

async function readBounded(response: Response, maximum: number): Promise<string> {
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > maximum) {
    throw new RrdClientError("RRD response exceeded the configured byte limit");
  }
  if (!response.body) return "";
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > maximum) {
      await reader.cancel();
      throw new RrdClientError("RRD response exceeded the configured byte limit");
    }
    chunks.push(value);
  }
  const combined = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    combined.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder("utf-8", { fatal: true }).decode(combined);
}
