from __future__ import annotations

import json
import pprint
import subprocess
import sys
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
REPOSITORY_ROOT = PACKAGE_ROOT.parents[1]
OUTPUT = PACKAGE_ROOT / "src" / "rrd_client" / "generated" / "endpoints.py"


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
    endpoints: dict[str, dict[str, object]] = {}
    for path, path_item in document["paths"].items():
        for method, operation in path_item.items():
            operation_id = operation.get("operationId")
            if operation_id is None:
                continue
            security = operation.get("security", [])
            scheme = next(iter(security[0]), None) if security else None
            authentication = {
                "rrdApiKey": "api_key",
                "rrdBearer": "session_bearer",
            }.get(scheme, "public")
            endpoints[operation_id] = {
                "method": method.upper(),
                "path": path,
                "authentication": authentication,
                "mutation": operation["x-rrd-mutation"],
            }
    operations = ", ".join(repr(operation) for operation in sorted(endpoints))
    encoded = pprint.pformat(endpoints, sort_dicts=True, width=100)
    source = (
        "# Generated from rrd-contract; do not edit.\n"
        "from typing import Final, Literal, TypedDict\n\n"
        "OperationId = Literal[" + operations + "]\n\n"
        "class Endpoint(TypedDict):\n"
        "    method: str\n"
        "    path: str\n"
        "    authentication: str\n"
        "    mutation: bool\n\n"
        f"ENDPOINTS: Final[dict[OperationId, Endpoint]] = {encoded}\n"
    )
    return subprocess.run(
        ["ruff", "format", "-", "--stdin-filename", str(OUTPUT)],
        check=True,
        capture_output=True,
        input=source,
        text=True,
    ).stdout


def main() -> None:
    generated = render()
    if "--check" in sys.argv:
        current = OUTPUT.read_text() if OUTPUT.exists() else ""
        if current != generated:
            raise SystemExit(
                "generated Python RRD endpoint map is stale; run uv run python scripts/generate.py"
            )
        return
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(generated)


if __name__ == "__main__":
    main()
