// Generated from rrd-contract; do not edit.
export const endpoints = {
  "audit-read": {
    "method": "POST",
    "path": "/v1/audit/read",
    "authentication": "session_bearer",
    "mutation": false
  },
  "backup-create": {
    "method": "POST",
    "path": "/v1/backups",
    "authentication": "session_bearer",
    "mutation": true
  },
  "backup-list": {
    "method": "POST",
    "path": "/v1/backups/list",
    "authentication": "session_bearer",
    "mutation": false
  },
  "capabilities-read": {
    "method": "GET",
    "path": "/v1/capabilities",
    "authentication": "public",
    "mutation": false
  },
  "changefeed-follow": {
    "method": "POST",
    "path": "/v1/changes/follow",
    "authentication": "session_bearer",
    "mutation": false
  },
  "changefeed-read": {
    "method": "POST",
    "path": "/v1/changes/read",
    "authentication": "session_bearer",
    "mutation": false
  },
  "diagnostics-read": {
    "method": "POST",
    "path": "/v1/diagnostics/read",
    "authentication": "session_bearer",
    "mutation": false
  },
  "estate-read": {
    "method": "POST",
    "path": "/v1/estates/{estate}/read",
    "authentication": "session_bearer",
    "mutation": false
  },
  "health-live": {
    "method": "GET",
    "path": "/v1/health/live",
    "authentication": "public",
    "mutation": false
  },
  "health-ready": {
    "method": "GET",
    "path": "/v1/health/ready",
    "authentication": "public",
    "mutation": false
  },
  "query-execute": {
    "method": "POST",
    "path": "/v1/query",
    "authentication": "session_bearer",
    "mutation": false
  },
  "query-index-ensure": {
    "method": "POST",
    "path": "/v1/query/indexes/ensure",
    "authentication": "session_bearer",
    "mutation": true
  },
  "query-index-list": {
    "method": "POST",
    "path": "/v1/query/indexes/list",
    "authentication": "session_bearer",
    "mutation": false
  },
  "query-live-poll": {
    "method": "POST",
    "path": "/v1/query/live/poll",
    "authentication": "session_bearer",
    "mutation": false
  },
  "restore-create": {
    "method": "POST",
    "path": "/v1/restores",
    "authentication": "session_bearer",
    "mutation": true
  },
  "runtime-tool-invoke": {
    "method": "POST",
    "path": "/v1/runtime/tools/invoke",
    "authentication": "session_bearer",
    "mutation": true
  },
  "runtime-tool-catalogue-read": {
    "method": "POST",
    "path": "/v1/runtime/tools/list",
    "authentication": "session_bearer",
    "mutation": false
  },
  "endpoint-catalogue": {
    "method": "GET",
    "path": "/v1/schema/endpoints",
    "authentication": "public",
    "mutation": false
  },
  "openapi-read": {
    "method": "GET",
    "path": "/v1/schema/openapi",
    "authentication": "public",
    "mutation": false
  },
  "session-create": {
    "method": "POST",
    "path": "/v1/sessions",
    "authentication": "api_key",
    "mutation": true
  },
  "session-close": {
    "method": "DELETE",
    "path": "/v1/sessions/{session}",
    "authentication": "session_bearer",
    "mutation": true
  },
  "session-renew": {
    "method": "POST",
    "path": "/v1/sessions/{session}/renew",
    "authentication": "session_bearer",
    "mutation": true
  },
  "subscription-close": {
    "method": "POST",
    "path": "/v1/subscriptions/close",
    "authentication": "session_bearer",
    "mutation": true
  },
  "subscription-open": {
    "method": "POST",
    "path": "/v1/subscriptions/open",
    "authentication": "session_bearer",
    "mutation": true
  },
  "transaction-begin": {
    "method": "POST",
    "path": "/v1/transactions",
    "authentication": "session_bearer",
    "mutation": true
  },
  "transaction-abort": {
    "method": "DELETE",
    "path": "/v1/transactions/{transaction}",
    "authentication": "session_bearer",
    "mutation": true
  },
  "transaction-commit": {
    "method": "POST",
    "path": "/v1/transactions/{transaction}/commit",
    "authentication": "session_bearer",
    "mutation": true
  },
  "transaction-preview": {
    "method": "POST",
    "path": "/v1/transactions/{transaction}/preview",
    "authentication": "session_bearer",
    "mutation": true
  },
  "vector-collection-ensure": {
    "method": "POST",
    "path": "/v1/vector/collections/ensure",
    "authentication": "session_bearer",
    "mutation": true
  },
  "vector-collection-list": {
    "method": "POST",
    "path": "/v1/vector/collections/list",
    "authentication": "session_bearer",
    "mutation": false
  },
  "vector-point-retrieve": {
    "method": "POST",
    "path": "/v1/vector/points/retrieve",
    "authentication": "session_bearer",
    "mutation": false
  },
  "vector-point-scroll": {
    "method": "POST",
    "path": "/v1/vector/points/scroll",
    "authentication": "session_bearer",
    "mutation": false
  },
  "vector-search": {
    "method": "POST",
    "path": "/v1/vector/search",
    "authentication": "session_bearer",
    "mutation": false
  }
} as const;

export type OperationId = keyof typeof endpoints;
