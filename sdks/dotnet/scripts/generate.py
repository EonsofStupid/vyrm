from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
REPOSITORY_ROOT = PACKAGE_ROOT.parents[1]
OUTPUT = PACKAGE_ROOT / "src/Rrflow.Rrd.Client/OperationId.g.cs"


def member_name(operation: str) -> str:
    return "".join(part.capitalize() for part in operation.split("-"))


def render() -> str:
    raw = subprocess.run(
        [
            "cargo",
            "run",
            "-q",
            "--manifest-path",
            str(REPOSITORY_ROOT / "Cargo.toml"),
            "-p",
            "rrd-contract",
            "--bin",
            "rrd-contract-export",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    document = json.loads(raw)
    endpoints: list[tuple[str, str, str, str, bool]] = []
    for path, path_item in document["paths"].items():
        for method, operation in path_item.items():
            operation_id = operation.get("operationId")
            if operation_id is None:
                continue
            security = operation.get("security", [])
            scheme = next(iter(security[0]), None) if security else None
            authentication = {
                "rrdApiKey": "ApiKey",
                "rrdBearer": "SessionBearer",
            }.get(scheme, "Public")
            endpoints.append(
                (
                    operation_id,
                    method.upper(),
                    path,
                    authentication,
                    operation["x-rrd-mutation"],
                )
            )
    endpoints.sort()
    members = "\n".join(f"    {member_name(item[0])}," for item in endpoints)
    cases = "\n".join(
        f'        OperationId.{member_name(operation)} => new("{operation}", "{method}", '
        f'"{path}", Authentication.{authentication}, {str(mutation).lower()}),'
        for operation, method, path, authentication, mutation in endpoints
    )
    return f"""// Generated from rrd-contract; do not edit.
namespace Rrflow.Rrd;

public enum OperationId
{{
{members}
}}

public enum Authentication
{{
    Public,
    ApiKey,
    SessionBearer,
}}

public sealed record Endpoint(
    string WireName,
    string Method,
    string Path,
    Authentication Authentication,
    bool Mutation);

public static class EndpointCatalog
{{
    public const int Count = {len(endpoints)};

    public static Endpoint Get(OperationId operation) => operation switch
    {{
{cases}
        _ => throw new System.ArgumentOutOfRangeException(nameof(operation)),
    }};
}}
"""


def main() -> None:
    generated = render()
    if "--check" in sys.argv:
        current = OUTPUT.read_text() if OUTPUT.exists() else ""
        if current != generated:
            raise SystemExit("generated .NET RRD endpoint map is stale; run python scripts/generate.py")
        return
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(generated)


if __name__ == "__main__":
    main()
