from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
REPOSITORY_ROOT = PACKAGE_ROOT.parents[1]
OUTPUT = PACKAGE_ROOT / "src/main/java/io/rrflow/rrd/OperationId.java"


def enum_name(operation: str) -> str:
    return operation.upper().replace("-", "_")


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
                "rrdApiKey": "API_KEY",
                "rrdBearer": "SESSION_BEARER",
            }.get(scheme, "PUBLIC")
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
    constants = []
    for operation, method, path, authentication, mutation in endpoints:
        constants.append(
            f'    {enum_name(operation)}("{operation}", "{method}", "{path}", '
            f"Authentication.{authentication}, {str(mutation).lower()})"
        )
    return (
        "// Generated from rrd-contract; do not edit.\n"
        "package io.rrflow.rrd;\n\n"
        "public enum OperationId {\n"
        + ",\n".join(constants)
        + ";\n\n"
        "    public enum Authentication { PUBLIC, API_KEY, SESSION_BEARER }\n\n"
        "    private final String wireName;\n"
        "    private final String method;\n"
        "    private final String path;\n"
        "    private final Authentication authentication;\n"
        "    private final boolean mutation;\n\n"
        "    OperationId(String wireName, String method, String path, Authentication authentication, boolean mutation) {\n"
        "        this.wireName = wireName;\n"
        "        this.method = method;\n"
        "        this.path = path;\n"
        "        this.authentication = authentication;\n"
        "        this.mutation = mutation;\n"
        "    }\n\n"
        "    public String wireName() { return wireName; }\n"
        "    public String method() { return method; }\n"
        "    public String path() { return path; }\n"
        "    public Authentication authentication() { return authentication; }\n"
        "    public boolean mutation() { return mutation; }\n"
        "}\n"
    )


def main() -> None:
    generated = render()
    if "--check" in sys.argv:
        current = OUTPUT.read_text() if OUTPUT.exists() else ""
        if current != generated:
            raise SystemExit("generated Java RRD endpoint enum is stale; run python scripts/generate.py")
        return
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(generated)


if __name__ == "__main__":
    main()
