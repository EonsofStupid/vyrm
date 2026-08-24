# Generated from rrd-contract; do not edit.
from typing import Final, Literal, TypedDict

OperationId = Literal[
    "audit-read",
    "backup-create",
    "backup-list",
    "capabilities-read",
    "changefeed-follow",
    "changefeed-read",
    "endpoint-catalogue",
    "estate-read",
    "health-live",
    "health-ready",
    "openapi-read",
    "query-execute",
    "query-index-ensure",
    "query-index-list",
    "query-live-poll",
    "restore-create",
    "session-close",
    "session-create",
    "session-renew",
    "transaction-abort",
    "transaction-begin",
    "transaction-commit",
    "transaction-preview",
    "vector-search",
]


class Endpoint(TypedDict):
    method: str
    path: str
    authentication: str
    mutation: bool


ENDPOINTS: Final[dict[OperationId, Endpoint]] = {
    "audit-read": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": False,
        "path": "/v1/audit/read",
    },
    "backup-create": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": True,
        "path": "/v1/backups",
    },
    "backup-list": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": False,
        "path": "/v1/backups/list",
    },
    "capabilities-read": {
        "authentication": "public",
        "method": "GET",
        "mutation": False,
        "path": "/v1/capabilities",
    },
    "changefeed-follow": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": False,
        "path": "/v1/changes/follow",
    },
    "changefeed-read": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": False,
        "path": "/v1/changes/read",
    },
    "endpoint-catalogue": {
        "authentication": "public",
        "method": "GET",
        "mutation": False,
        "path": "/v1/schema/endpoints",
    },
    "estate-read": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": False,
        "path": "/v1/estates/{estate}/read",
    },
    "health-live": {
        "authentication": "public",
        "method": "GET",
        "mutation": False,
        "path": "/v1/health/live",
    },
    "health-ready": {
        "authentication": "public",
        "method": "GET",
        "mutation": False,
        "path": "/v1/health/ready",
    },
    "openapi-read": {
        "authentication": "public",
        "method": "GET",
        "mutation": False,
        "path": "/v1/schema/openapi",
    },
    "query-execute": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": False,
        "path": "/v1/query",
    },
    "query-index-ensure": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": True,
        "path": "/v1/query/indexes/ensure",
    },
    "query-index-list": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": False,
        "path": "/v1/query/indexes/list",
    },
    "query-live-poll": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": False,
        "path": "/v1/query/live/poll",
    },
    "restore-create": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": True,
        "path": "/v1/restores",
    },
    "session-close": {
        "authentication": "session_bearer",
        "method": "DELETE",
        "mutation": True,
        "path": "/v1/sessions/{session}",
    },
    "session-create": {
        "authentication": "api_key",
        "method": "POST",
        "mutation": True,
        "path": "/v1/sessions",
    },
    "session-renew": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": True,
        "path": "/v1/sessions/{session}/renew",
    },
    "transaction-abort": {
        "authentication": "session_bearer",
        "method": "DELETE",
        "mutation": True,
        "path": "/v1/transactions/{transaction}",
    },
    "transaction-begin": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": True,
        "path": "/v1/transactions",
    },
    "transaction-commit": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": True,
        "path": "/v1/transactions/{transaction}/commit",
    },
    "transaction-preview": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": False,
        "path": "/v1/transactions/{transaction}/preview",
    },
    "vector-search": {
        "authentication": "session_bearer",
        "method": "POST",
        "mutation": False,
        "path": "/v1/vector/search",
    },
}
