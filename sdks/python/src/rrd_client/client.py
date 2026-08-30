from __future__ import annotations

import ipaddress
import json
import re
import time
from collections.abc import Mapping
from typing import Any, cast
from urllib.parse import quote, urljoin, urlsplit

import httpx
from pydantic import ValidationError

from .generated import ENDPOINTS, OperationId
from .models import ErrorOutcome, RequestOptions, ResourceSegment, ResponseEnvelope, Session

_CORRELATION = re.compile(r"^[A-Za-z0-9._:-]+$")
_CANONICAL = re.compile(r"^[a-z0-9][a-z0-9._-]*$")
_RESOURCE_KINDS = {
    "organization",
    "estate",
    "project",
    "instance",
    "node",
    "shard",
    "collection",
    "table",
    "record",
    "transaction",
    "snapshot",
    "backup",
    "operation",
}


class RrdClientError(Exception):
    pass


class RrdApiError(RrdClientError):
    def __init__(self, status: int, code: str, message: str, retryable: bool) -> None:
        super().__init__(f"RRD API {status}: {code}: {message}")
        self.status = status
        self.code = code
        self.retryable = retryable


class RrdClient:
    def __init__(
        self,
        base_url: str,
        instance: str,
        *,
        request_timeout: float = 5.0,
        max_attempts: int = 2,
        max_response_bytes: int = 4 * 1024 * 1024,
        transport: httpx.BaseTransport | None = None,
    ) -> None:
        self.base_url = _loopback_url(base_url)
        self.instance = _canonical(instance, "instance")
        if not 0 < request_timeout <= 300:
            raise RrdClientError("request timeout must be in (0, 300] seconds")
        if not 1 <= max_attempts <= 8:
            raise RrdClientError("max attempts must be in 1..=8")
        if not 1 <= max_response_bytes <= 16 * 1024 * 1024:
            raise RrdClientError("response limit must be in 1..=16777216 bytes")
        self.request_timeout = request_timeout
        self.max_attempts = max_attempts
        self.max_response_bytes = max_response_bytes
        self._http = httpx.Client(transport=transport, follow_redirects=False)

    def close(self) -> None:
        self._http.close()

    def __enter__(self) -> RrdClient:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    def capabilities(self) -> dict[str, Any]:
        result = self.call("capabilities-read")
        instance = result.get("instance")
        if (
            result.get("protocol") != "rrd"
            or result.get("protocol_version") != 1
            or not isinstance(instance, dict)
            or instance.get("id") != self.instance
        ):
            raise RrdClientError("RRD capability protocol or instance identity differs")
        return result

    def endpoint_catalogue(self) -> dict[str, Any]:
        return self.call("endpoint-catalogue")

    def openapi(self) -> dict[str, Any]:
        return self.call("openapi-read")

    def create_session(
        self,
        principal_id: str,
        api_key: str,
        payload: Mapping[str, Any],
        options: RequestOptions,
    ) -> Session:
        principal = _canonical(principal_id, "principal")
        if not api_key:
            raise RrdClientError("API-key credential must not be empty")
        request = RequestOptions(
            request_id=options.request_id,
            operation_id=options.operation_id,
            idempotency_key=options.idempotency_key,
            deadline_unix_ms=options.deadline_unix_ms,
            path_parameters=options.path_parameters,
            resource=options.resource,
            principal_id=principal,
            api_key=api_key,
        )
        lease = self.call("session-create", payload, request)
        return Session(principal_id=principal, lease=lease)

    def call(
        self,
        operation: OperationId,
        payload: Mapping[str, Any] | None = None,
        options: RequestOptions | None = None,
    ) -> dict[str, Any]:
        descriptor = ENDPOINTS[operation]
        options = options or RequestOptions()
        context = (
            None if descriptor["method"] == "GET" else _context(options, descriptor["mutation"])
        )
        headers = {"Accept": "application/json"}
        if descriptor["authentication"] == "api_key":
            if options.principal_id is None or options.api_key is None:
                raise RrdClientError(f"{operation} requires API-key authentication")
            headers["X-RRD-Principal"] = _canonical(options.principal_id, "principal")
            headers["Authorization"] = f"ApiKey {options.api_key}"
        elif descriptor["authentication"] == "session_bearer":
            if options.session is None:
                raise RrdClientError(f"{operation} requires a session")
            headers["X-RRD-Session"] = str(options.session.lease["session_id"])
            headers["Authorization"] = f"Bearer {options.session.lease['token']}"
        path = _resolve_path(descriptor["path"], options.path_parameters)
        body = None
        if descriptor["method"] != "GET":
            headers["Content-Type"] = "application/json"
            resource = options.resource or _default_resource(self.instance, options.path_parameters)
            body = json.dumps(
                {
                    "protocol": "rrd",
                    "protocol_version": 1,
                    "context": context,
                    "resource": {"segments": _resource(resource)},
                    "payload": dict(payload or {}),
                },
                separators=(",", ":"),
            )
        retry_safe = (
            descriptor["method"] == "GET"
            or not descriptor["mutation"]
            or bool(options.idempotency_key)
        )
        attempts = self.max_attempts if retry_safe else 1
        last_error: Exception | None = None
        for _ in range(attempts):
            timeout = _remaining_timeout(self.request_timeout, options.deadline_unix_ms)
            try:
                with self._http.stream(
                    descriptor["method"],
                    urljoin(self.base_url, path.lstrip("/")),
                    headers=headers,
                    content=body,
                    timeout=timeout,
                ) as response:
                    decoded = _read_response(response, self.max_response_bytes)
                envelope = ResponseEnvelope.model_validate_json(decoded)
            except (httpx.TransportError, httpx.TimeoutException) as error:
                last_error = error
                continue
            except (ValidationError, UnicodeDecodeError, json.JSONDecodeError) as error:
                raise RrdClientError(f"RRD response envelope is invalid: {error}") from error
            if context and (
                envelope.request_id != context["request_id"]
                or envelope.operation_id != context["operation_id"]
            ):
                raise RrdClientError("RRD response request/operation identity differs")
            if response.is_success != (envelope.outcome.status == "ok"):
                raise RrdClientError("RRD HTTP status and typed outcome disagree")
            if isinstance(envelope.outcome, ErrorOutcome):
                outcome_error = envelope.outcome.error
                raise RrdApiError(
                    response.status_code,
                    outcome_error.code,
                    outcome_error.message,
                    outcome_error.retryable,
                )
            if not isinstance(envelope.outcome.payload, dict):
                raise RrdClientError("RRD success payload must be an object")
            return cast(dict[str, Any], envelope.outcome.payload)
        message = str(last_error) if last_error else "attempt budget exhausted"
        raise RrdClientError(f"RRD transport failed: {message}")


def _context(options: RequestOptions, mutation: bool) -> dict[str, Any]:
    request_id = _correlation(options.request_id, "request ID")
    operation_id = _correlation(options.operation_id, "operation ID")
    idempotency_key = (
        _correlation(options.idempotency_key, "idempotency key")
        if options.idempotency_key
        else None
    )
    if mutation and idempotency_key is None:
        raise RrdClientError("mutating requests require an idempotency key")
    if options.deadline_unix_ms is not None and options.deadline_unix_ms <= 0:
        raise RrdClientError("deadline must be a positive Unix millisecond integer")
    return {
        "request_id": request_id,
        "operation_id": operation_id,
        **({"idempotency_key": idempotency_key} if idempotency_key else {}),
        **({"deadline_unix_ms": options.deadline_unix_ms} if options.deadline_unix_ms else {}),
    }


def _default_resource(instance: str, parameters: Mapping[str, str]) -> tuple[ResourceSegment, ...]:
    segments = []
    if estate := parameters.get("estate"):
        segments.append(ResourceSegment("estate", _canonical(estate, "estate")))
    segments.append(ResourceSegment("instance", instance))
    return tuple(segments)


def _resource(segments: tuple[ResourceSegment, ...]) -> list[dict[str, str]]:
    if not 1 <= len(segments) <= 16:
        raise RrdClientError("resource paths must contain 1..=16 segments")
    kinds: set[str] = set()
    result = []
    for segment in segments:
        if segment.kind not in _RESOURCE_KINDS or segment.kind in kinds:
            raise RrdClientError("resource kind is unknown or repeated")
        kinds.add(segment.kind)
        result.append({"kind": segment.kind, "id": _canonical(segment.id, "resource")})
    return result


def _resolve_path(template: str, parameters: Mapping[str, str]) -> str:
    def replace(match: re.Match[str]) -> str:
        name = match.group(1)
        value = parameters.get(name)
        if not value:
            raise RrdClientError(f"missing path parameter {name}")
        return quote(_correlation(value, f"{name} path parameter"), safe="")

    return re.sub(r"\{([a-z]+)\}", replace, template)


def _correlation(value: str | None, label: str) -> str:
    if value is None or len(value) > 128 or not _CORRELATION.fullmatch(value):
        raise RrdClientError(f"{label} is not a canonical RRD correlation ID")
    return value


def _canonical(value: str, label: str) -> str:
    if len(value) > 128 or not _CANONICAL.fullmatch(value):
        raise RrdClientError(f"{label} is not a canonical RRD identifier")
    return value


def _loopback_url(value: str) -> str:
    parsed = urlsplit(value)
    host = parsed.hostname
    loopback = host == "localhost"
    if host and host != "localhost":
        try:
            loopback = ipaddress.ip_address(host).is_loopback
        except ValueError:
            loopback = False
    if (
        parsed.scheme != "http"
        or not loopback
        or parsed.username
        or parsed.password
        or parsed.query
        or parsed.fragment
    ):
        raise RrdClientError(
            "RRD Python client permits only credential-free loopback HTTP before TLS qualification"
        )
    return value.rstrip("/") + "/"


def _remaining_timeout(configured: float, deadline_unix_ms: int | None) -> float:
    if deadline_unix_ms is None:
        return configured
    remaining = deadline_unix_ms / 1000 - time.time()
    if remaining <= 0:
        raise RrdClientError("RRD request deadline has expired")
    return min(configured, remaining)


def _read_response(response: httpx.Response, maximum: int) -> bytes:
    declared = response.headers.get("content-length")
    if declared and declared.isdigit() and int(declared) > maximum:
        raise RrdClientError("RRD response exceeded the configured byte limit")
    chunks = []
    length = 0
    for chunk in response.iter_bytes():
        length += len(chunk)
        if length > maximum:
            raise RrdClientError("RRD response exceeded the configured byte limit")
        chunks.append(chunk)
    return b"".join(chunks)
