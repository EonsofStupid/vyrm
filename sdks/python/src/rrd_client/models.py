from __future__ import annotations

from dataclasses import dataclass, field
from typing import Annotated, Any, Literal

from pydantic import BaseModel, ConfigDict, Field


class ErrorBody(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)

    code: str
    message: str
    retryable: bool
    details: dict[str, str] = Field(default_factory=dict)


class OkOutcome(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)

    status: Literal["ok"]
    payload: Any


class ErrorOutcome(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)

    status: Literal["error"]
    error: ErrorBody


class ResponseEnvelope(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)

    protocol: Literal["rrd"]
    protocol_version: Literal[1]
    request_id: str
    operation_id: str
    outcome: Annotated[OkOutcome | ErrorOutcome, Field(discriminator="status")]


@dataclass(frozen=True, slots=True)
class ResourceSegment:
    kind: str
    id: str


@dataclass(frozen=True, slots=True)
class Session:
    principal_id: str
    lease: dict[str, Any]


@dataclass(frozen=True, slots=True)
class RequestOptions:
    request_id: str | None = None
    operation_id: str | None = None
    idempotency_key: str | None = None
    deadline_unix_ms: int | None = None
    path_parameters: dict[str, str] = field(default_factory=dict)
    resource: tuple[ResourceSegment, ...] | None = None
    session: Session | None = None
    principal_id: str | None = None
    api_key: str | None = None
