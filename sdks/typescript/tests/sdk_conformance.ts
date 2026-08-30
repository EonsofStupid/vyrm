import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import {
  type RequestOptions,
  type RequestPayload,
  RrdApiError,
  RrdClient,
  RrdClientError,
  type Session,
} from "../src/index.js";

type JsonObject = Record<string, unknown>;

interface HarnessManifest {
  format_version: number;
  corpus_sha256: string;
  corpus_path: string;
  base_url: string;
  retry_base_urls: Record<string, string>;
  incompatible_version_url: string;
}

interface Corpus {
  format_version: number;
  protocol: string;
  protocol_version: number;
  incompatible_protocol_version: number;
  required_domains: string[];
  identity: { instance: string; estate: string; principal: string; api_key: string };
  session: { create: JsonObject; renew: JsonObject; close: JsonObject };
  transaction: {
    preview_begin: JsonObject;
    preview: JsonObject;
    abort: JsonObject;
    commit_begin: JsonObject;
    commit_deadline_timeout_ms: number;
    commit: JsonObject & { operation_sha256: string };
  };
  query: JsonObject;
  vector: { ensure: JsonObject; search: JsonObject };
  changefeed: {
    read: JsonObject & { after_cursor: number; limit: number; scope: string };
    follow_wait_timeout_ms: number;
    cancellation_wait_timeout_ms: number;
    cancel_after_ms: number;
  };
  backup: { create: JsonObject & { label: string }; list: JsonObject };
  estate: JsonObject;
  expected: {
    endpoint_count: number;
    query_identity: string;
    vector_reference: string;
    estate_revision: number;
    typed_error: string;
    retry_attempts: number;
  };
}

const manifestPath = process.env.RRD_SDK_CONFORMANCE_MANIFEST;
assert.ok(manifestPath, "RRD_SDK_CONFORMANCE_MANIFEST is required");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as HarnessManifest;
const corpusBytes = readFileSync(manifest.corpus_path);
const corpus = JSON.parse(corpusBytes.toString("utf8")) as Corpus;
assert.equal(manifest.format_version, corpus.format_version);
assert.equal(createHash("sha256").update(corpusBytes).digest("hex"), manifest.corpus_sha256);
assert.deepEqual(
  new Set(corpus.required_domains),
  new Set([
    "auth",
    "backup",
    "cancellation",
    "crud",
    "estate",
    "live_feeds",
    "query",
    "retries",
    "sessions",
    "transactions",
    "typed_errors",
    "vectors",
    "versions",
  ]),
);

const mutation = (
  step: string,
  session?: Session,
  pathParameters?: Record<string, string>,
): RequestOptions => ({
  requestId: `typescript-${step}-request`,
  operationId: `typescript-${step}-operation`,
  idempotencyKey: `typescript-${step}-key`,
  ...(session ? { session } : {}),
  ...(pathParameters ? { pathParameters } : {}),
});
const read = (
  step: string,
  session: Session,
  pathParameters?: Record<string, string>,
): RequestOptions => ({
  requestId: `typescript-${step}-request`,
  operationId: `typescript-${step}-operation`,
  session,
  ...(pathParameters ? { pathParameters } : {}),
});

const retryBaseUrl = manifest.retry_base_urls.typescript;
assert.ok(retryBaseUrl, "TypeScript retry endpoint is required");
const retryClient = new RrdClient({
  baseUrl: retryBaseUrl,
  instance: corpus.identity.instance,
  maxAttempts: corpus.expected.retry_attempts,
});
assert.equal((await retryClient.capabilities()).protocol_version, corpus.protocol_version);

const incompatibleClient = new RrdClient({
  baseUrl: manifest.incompatible_version_url,
  instance: corpus.identity.instance,
});
await assert.rejects(incompatibleClient.capabilities(), RrdClientError);

const client = new RrdClient({ baseUrl: manifest.base_url, instance: corpus.identity.instance });
assert.equal((await client.capabilities()).protocol, corpus.protocol);
assert.equal((await client.endpointCatalogue()).endpoints.length, corpus.expected.endpoint_count);

await assert.rejects(
  client.createSession(
    corpus.identity.principal,
    "wrong-sdk-conformance-key",
    corpus.session.create as RequestPayload<"session-create">,
    mutation("wrong-key"),
  ),
  (error) => error instanceof RrdApiError && error.code === corpus.expected.typed_error,
);

let session = await client.createSession(
  corpus.identity.principal,
  corpus.identity.api_key,
  corpus.session.create as RequestPayload<"session-create">,
  mutation("session-create"),
);
const renewed = await client.call(
  "session-renew",
  corpus.session.renew as RequestPayload<"session-renew">,
  mutation("session-renew", session, { session: session.lease.session_id }),
);
session = { principalId: session.principalId, lease: renewed };

await client.call(
  "vector-collection-ensure",
  corpus.vector.ensure as RequestPayload<"vector-collection-ensure">,
  mutation("vector-ensure", session),
);
const previewLease = await client.call(
  "transaction-begin",
  corpus.transaction.preview_begin as RequestPayload<"transaction-begin">,
  mutation("preview-begin", session),
);
const preview = await client.call(
  "transaction-preview",
  corpus.transaction.preview as RequestPayload<"transaction-preview">,
  mutation("transaction-preview", session, { transaction: previewLease.transaction_id }),
);
assert.equal(preview.transaction_id, previewLease.transaction_id);
const aborted = await client.call(
  "transaction-abort",
  corpus.transaction.abort as RequestPayload<"transaction-abort">,
  mutation("transaction-abort", session, { transaction: previewLease.transaction_id }),
);
assert.equal(aborted.state, "aborted");

const commitLease = await client.call(
  "transaction-begin",
  corpus.transaction.commit_begin as RequestPayload<"transaction-begin">,
  mutation("commit-begin", session),
);
const committed = await client.call(
  "transaction-commit",
  corpus.transaction.commit as RequestPayload<"transaction-commit">,
  {
    ...mutation("transaction-commit", session, { transaction: commitLease.transaction_id }),
    deadlineUnixMs: Date.now() + corpus.transaction.commit_deadline_timeout_ms,
  },
);
assert.equal(committed.operation_sha256, corpus.transaction.commit.operation_sha256);

const query = await client.call(
  "query-execute",
  corpus.query as RequestPayload<"query-execute">,
  read("query", session),
);
assert.ok(query.rows.some((row) => row.identity === corpus.expected.query_identity));
const vectors = await client.call(
  "vector-search",
  corpus.vector.search as RequestPayload<"vector-search">,
  read("vector-search", session),
);
assert.ok(
  vectors.hits.some(
    (hit) => `${hit.reference.kind}:${hit.reference.id}` === corpus.expected.vector_reference,
  ),
);

const changes = await client.call(
  "changefeed-read",
  corpus.changefeed.read as RequestPayload<"changefeed-read">,
  read("changefeed-read", session),
);
const followRead = { ...corpus.changefeed.read, after_cursor: changes.head_cursor };
const followed = await client.call(
  "changefeed-follow",
  { read: followRead, wait_timeout_ms: corpus.changefeed.follow_wait_timeout_ms },
  read("changefeed-follow", session),
);
assert.equal(followed.timed_out, true);
const controller = new AbortController();
const cancellation = setTimeout(() => controller.abort(), corpus.changefeed.cancel_after_ms);
try {
  await assert.rejects(
    client.call(
      "changefeed-follow",
      { read: followRead, wait_timeout_ms: corpus.changefeed.cancellation_wait_timeout_ms },
      { ...read("changefeed-cancel", session), signal: controller.signal },
    ),
    RrdClientError,
  );
} finally {
  clearTimeout(cancellation);
}

const backup = await client.call(
  "backup-create",
  corpus.backup.create as RequestPayload<"backup-create">,
  mutation("backup-create", session),
);
const backupPrefix = `${corpus.backup.create.label}--`;
assert.match(backup.backup.label.slice(backupPrefix.length), /^[0-9a-f]{16}$/);
const backups = await client.call(
  "backup-list",
  corpus.backup.list as RequestPayload<"backup-list">,
  read("backup-list", session),
);
assert.ok(backups.backups.some((entry) => entry.backup_sha256 === backup.backup.backup_sha256));
const estate = await client.call(
  "estate-read",
  corpus.estate as RequestPayload<"estate-read">,
  read("estate-read", session, { estate: corpus.identity.estate }),
);
assert.equal(estate.revision, corpus.expected.estate_revision);
const closed = await client.call(
  "session-close",
  corpus.session.close as RequestPayload<"session-close">,
  mutation("session-close", session, { session: session.lease.session_id }),
);
assert.equal(closed.state, "closed");
console.error(`SDK conformance OK: language=typescript corpus_sha256=${manifest.corpus_sha256}`);
