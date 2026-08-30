from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path
from typing import Any, cast

from rrd_client import RequestOptions, RrdApiError, RrdClient, RrdClientError, Session


def obj(value: object) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise AssertionError("conformance value must be an object")
    return cast(dict[str, Any], value)


manifest_path = os.environ.get("RRD_SDK_CONFORMANCE_MANIFEST")
if not manifest_path:
    raise SystemExit("RRD_SDK_CONFORMANCE_MANIFEST is required")
manifest = obj(json.loads(Path(manifest_path).read_text(encoding="utf-8")))
corpus_bytes = Path(str(manifest["corpus_path"])).read_bytes()
corpus = obj(json.loads(corpus_bytes))
assert hashlib.sha256(corpus_bytes).hexdigest() == manifest["corpus_sha256"]
assert set(cast(list[str], corpus["required_domains"])) == {
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
}
identity = obj(corpus["identity"])
expected = obj(corpus["expected"])
transaction = obj(corpus["transaction"])
session_fixture = obj(corpus["session"])
vector = obj(corpus["vector"])
changefeed = obj(corpus["changefeed"])
backup_fixture = obj(corpus["backup"])


def options(
    step: str,
    *,
    session: Session | None = None,
    mutation: bool = False,
    paths: dict[str, str] | None = None,
    deadline_unix_ms: int | None = None,
) -> RequestOptions:
    return RequestOptions(
        request_id=f"python-{step}-request",
        operation_id=f"python-{step}-operation",
        idempotency_key=f"python-{step}-key" if mutation else None,
        deadline_unix_ms=deadline_unix_ms,
        path_parameters=paths or {},
        session=session,
    )


with RrdClient(
    str(obj(manifest["retry_base_urls"])["python"]),
    str(identity["instance"]),
    max_attempts=int(expected["retry_attempts"]),
) as retry_client:
    assert retry_client.capabilities()["protocol_version"] == corpus["protocol_version"]

with RrdClient(str(manifest["incompatible_version_url"]), str(identity["instance"])) as client:
    try:
        client.capabilities()
        raise AssertionError("incompatible protocol was accepted")
    except RrdClientError:
        pass

with RrdClient(str(manifest["base_url"]), str(identity["instance"])) as client:
    assert client.capabilities()["protocol"] == corpus["protocol"]
    assert (
        len(cast(list[object], client.endpoint_catalogue()["endpoints"]))
        == expected["endpoint_count"]
    )
    try:
        client.create_session(
            str(identity["principal"]),
            "wrong-sdk-conformance-key",
            obj(session_fixture["create"]),
            options("wrong-key", mutation=True),
        )
        raise AssertionError("wrong API key was accepted")
    except RrdApiError as error:
        assert error.code == expected["typed_error"]

    session = client.create_session(
        str(identity["principal"]),
        str(identity["api_key"]),
        obj(session_fixture["create"]),
        options("session-create", mutation=True),
    )
    renewed = client.call(
        "session-renew",
        obj(session_fixture["renew"]),
        options(
            "session-renew",
            session=session,
            mutation=True,
            paths={"session": str(session.lease["session_id"])},
        ),
    )
    session = Session(session.principal_id, renewed)
    client.call(
        "vector-collection-ensure",
        obj(vector["ensure"]),
        options("vector-ensure", session=session, mutation=True),
    )
    preview_lease = client.call(
        "transaction-begin",
        obj(transaction["preview_begin"]),
        options("preview-begin", session=session, mutation=True),
    )
    preview = client.call(
        "transaction-preview",
        obj(transaction["preview"]),
        options(
            "transaction-preview",
            session=session,
            mutation=True,
            paths={"transaction": str(preview_lease["transaction_id"])},
        ),
    )
    assert preview["transaction_id"] == preview_lease["transaction_id"]
    aborted = client.call(
        "transaction-abort",
        obj(transaction["abort"]),
        options(
            "transaction-abort",
            session=session,
            mutation=True,
            paths={"transaction": str(preview_lease["transaction_id"])},
        ),
    )
    assert aborted["state"] == "aborted"
    commit_lease = client.call(
        "transaction-begin",
        obj(transaction["commit_begin"]),
        options("commit-begin", session=session, mutation=True),
    )
    committed = client.call(
        "transaction-commit",
        obj(transaction["commit"]),
        options(
            "transaction-commit",
            session=session,
            mutation=True,
            paths={"transaction": str(commit_lease["transaction_id"])},
            deadline_unix_ms=int(time.time() * 1000)
            + int(transaction["commit_deadline_timeout_ms"]),
        ),
    )
    assert committed["operation_sha256"] == obj(transaction["commit"])["operation_sha256"]
    query = client.call("query-execute", obj(corpus["query"]), options("query", session=session))
    assert any(
        obj(row)["identity"] == expected["query_identity"]
        for row in cast(list[object], query["rows"])
    )
    vectors = client.call(
        "vector-search", obj(vector["search"]), options("vector-search", session=session)
    )
    assert any(
        f"{obj(obj(hit)['reference'])['kind']}:{obj(obj(hit)['reference'])['id']}"
        == expected["vector_reference"]
        for hit in cast(list[object], vectors["hits"])
    )
    changes = client.call(
        "changefeed-read", obj(changefeed["read"]), options("changefeed-read", session=session)
    )
    follow_read = {**obj(changefeed["read"]), "after_cursor": changes["head_cursor"]}
    followed = client.call(
        "changefeed-follow",
        {"read": follow_read, "wait_timeout_ms": changefeed["follow_wait_timeout_ms"]},
        options("changefeed-follow", session=session),
    )
    assert followed["timed_out"] is True
    cancellation_client = RrdClient(
        str(manifest["base_url"]), str(identity["instance"]), max_attempts=1
    )
    try:
        try:
            cancellation_client.call(
                "changefeed-follow",
                {
                    "read": follow_read,
                    "wait_timeout_ms": changefeed["cancellation_wait_timeout_ms"],
                },
                options(
                    "changefeed-cancel",
                    session=session,
                    deadline_unix_ms=int(time.time() * 1000) + int(changefeed["cancel_after_ms"]),
                ),
            )
            raise AssertionError("deadline cancellation was not observed")
        except RrdClientError:
            pass
    finally:
        cancellation_client.close()
    backup = client.call(
        "backup-create",
        obj(backup_fixture["create"]),
        options("backup-create", session=session, mutation=True),
    )
    backup_snapshot = obj(backup["backup"])
    label_prefix = f"{obj(backup_fixture['create'])['label']}--"
    assert str(backup_snapshot["label"]).startswith(label_prefix)
    backups = client.call(
        "backup-list", obj(backup_fixture["list"]), options("backup-list", session=session)
    )
    assert any(
        obj(entry)["backup_sha256"] == backup_snapshot["backup_sha256"]
        for entry in cast(list[object], backups["backups"])
    )
    estate = client.call(
        "estate-read",
        obj(corpus["estate"]),
        options("estate-read", session=session, paths={"estate": str(identity["estate"])}),
    )
    assert estate["revision"] == expected["estate_revision"]
    closed = client.call(
        "session-close",
        obj(session_fixture["close"]),
        options(
            "session-close",
            session=session,
            mutation=True,
            paths={"session": str(session.lease["session_id"])},
        ),
    )
    assert closed["state"] == "closed"

print(f"SDK conformance OK: language=python corpus_sha256={manifest['corpus_sha256']}")
