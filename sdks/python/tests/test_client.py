from __future__ import annotations

import json
from typing import Any

import httpx
import pytest

from rrd_client import ENDPOINTS, RequestOptions, RrdApiError, RrdClient, RrdClientError, Session


def envelope(
    payload: dict[str, Any],
    *,
    request_id: str = "server-request",
    operation_id: str = "server-operation",
    status_code: int = 200,
) -> httpx.Response:
    return httpx.Response(
        status_code,
        json={
            "protocol": "rrd",
            "protocol_version": 1,
            "request_id": request_id,
            "operation_id": operation_id,
            "outcome": {"status": "ok", "payload": payload},
        },
    )


def test_public_negotiation_retries_transport_loss_and_validates_envelope() -> None:
    attempts = 0

    def handler(request: httpx.Request) -> httpx.Response:
        nonlocal attempts
        attempts += 1
        assert request.url.path == "/v1/capabilities"
        assert request.method == "GET"
        if attempts == 1:
            raise httpx.ConnectError("connection reset", request=request)
        return envelope(
            {
                "protocol": "rrd",
                "protocol_version": 1,
                "implementation": "rrd-server",
                "implementation_version": "0.1.0",
                "deployment_mode": "local_server",
                "instance": {"kind": "instance", "id": "sdk-test"},
                "capabilities": [],
            }
        )

    with RrdClient(
        "http://127.0.0.1:9477",
        "sdk-test",
        transport=httpx.MockTransport(handler),
    ) as client:
        capabilities = client.capabilities()
    assert capabilities["protocol_version"] == 1
    assert attempts == 2


def test_session_and_query_construct_authenticated_bounded_v1_envelopes() -> None:
    requests: list[tuple[httpx.Request, dict[str, Any]]] = []

    def handler(request: httpx.Request) -> httpx.Response:
        body: dict[str, Any] = json.loads(request.content)
        requests.append((request, body))
        if request.url.path == "/v1/sessions":
            payload: dict[str, Any] = {
                "session_id": "session-1",
                "token": "token-1",
                "issued_at_unix_ms": 100,
                "idle_expires_at_unix_ms": 60_100,
                "absolute_expires_at_unix_ms": 300_100,
                "limits": {
                    "idle_timeout_ms": 60_000,
                    "absolute_timeout_ms": 300_000,
                    "max_open_transactions": 2,
                },
            }
        else:
            payload = {
                "canonical_query": "FROM record:document",
                "scope": "instance:sdk-test",
                "read_manifest_sha256": "a" * 64,
                "known_at_cursor": 2,
                "schema_revision": 1,
                "plan": {},
                "execution": {},
                "rows": [],
            }
        context = body["context"]
        return envelope(
            payload,
            request_id=context["request_id"],
            operation_id=context["operation_id"],
        )

    with RrdClient(
        "http://localhost:9477",
        "sdk-test",
        transport=httpx.MockTransport(handler),
    ) as client:
        session = client.create_session(
            "python-sdk",
            "not-persisted",
            {
                "limits": {
                    "idle_timeout_ms": 60_000,
                    "absolute_timeout_ms": 300_000,
                    "max_open_transactions": 2,
                }
            },
            RequestOptions(
                request_id="request-session",
                operation_id="operation-session",
                idempotency_key="session-key",
            ),
        )
        query = client.call(
            "query-execute",
            {
                "scope": "instance:sdk-test",
                "query": "FROM record:document",
                "parameters": {},
                "budget": {
                    "max_scanned_changes": 100,
                    "max_rows": 10,
                    "max_output_bytes": 4_096,
                    "max_batch_rows": 10,
                },
            },
            RequestOptions(
                request_id="request-query",
                operation_id="operation-query",
                session=session,
            ),
        )

    assert query["known_at_cursor"] == 2
    assert requests[0][0].headers["authorization"] == "ApiKey not-persisted"
    assert requests[1][0].headers["authorization"] == "Bearer token-1"
    assert requests[1][1]["resource"]["segments"] == [{"kind": "instance", "id": "sdk-test"}]


def test_remote_cleartext_expired_deadlines_and_typed_api_errors_are_rejected() -> None:
    with pytest.raises(RrdClientError, match="loopback"):
        RrdClient("http://192.0.2.1:9477", "sdk-test")

    def handler(request: httpx.Request) -> httpx.Response:
        body = json.loads(request.content)
        return httpx.Response(
            403,
            json={
                "protocol": "rrd",
                "protocol_version": 1,
                "request_id": body["context"]["request_id"],
                "operation_id": body["context"]["operation_id"],
                "outcome": {
                    "status": "error",
                    "error": {
                        "code": "permission_denied",
                        "message": "policy denied",
                        "retryable": False,
                        "details": {},
                    },
                },
            },
        )

    session = Session(
        principal_id="python-sdk",
        lease={"session_id": "session-1", "token": "token-1"},
    )
    with RrdClient(
        "http://127.0.0.1:9477",
        "sdk-test",
        max_attempts=1,
        transport=httpx.MockTransport(handler),
    ) as client:
        with pytest.raises(RrdApiError) as captured:
            client.call(
                "audit-read",
                {"after_sequence": 0, "limit": 10},
                RequestOptions(
                    request_id="request-audit",
                    operation_id="operation-audit",
                    session=session,
                ),
            )
        assert captured.value.code == "permission_denied"

        with pytest.raises(RrdClientError, match="deadline has expired"):
            client.call(
                "audit-read",
                {"after_sequence": 0, "limit": 10},
                RequestOptions(
                    request_id="request-expired",
                    operation_id="operation-expired",
                    deadline_unix_ms=1,
                    session=session,
                ),
            )


def test_generated_catalogue_covers_the_frozen_contract() -> None:
    assert len(ENDPOINTS) == 31
    assert ENDPOINTS["capabilities-read"] == {
        "method": "GET",
        "path": "/v1/capabilities",
        "authentication": "public",
        "mutation": False,
    }
    assert ENDPOINTS["backup-create"]["mutation"] is True
