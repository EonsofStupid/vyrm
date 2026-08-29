#!/usr/bin/env python3
"""Fail when checked-in SDK projections drift from the RRD contract export."""

from __future__ import annotations

import ast
import hashlib
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GENERATED = {
    "typescript schema": ROOT / "sdks/typescript/src/generated/rrd-openapi.ts",
    "typescript endpoints": ROOT / "sdks/typescript/src/generated/endpoints.ts",
    "python endpoints": ROOT / "sdks/python/src/rrd_client/generated/endpoints.py",
    "go endpoints": ROOT / "sdks/go/endpoints_gen.go",
    "java endpoints": ROOT / "sdks/java/src/main/java/io/rrflow/rrd/OperationId.java",
    "dotnet endpoints": ROOT / "sdks/dotnet/src/Rrflow.Rrd.Client/OperationId.g.cs",
}


def contract_export() -> tuple[bytes, dict[str, object]]:
    raw = subprocess.run(
        [
            "cargo",
            "run",
            "-q",
            "--locked",
            "-p",
            "rrd-contract",
            "--bin",
            "rrd-contract-export",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
    ).stdout
    return raw, json.loads(raw)


def canonical_endpoints(document: dict[str, object]) -> dict[str, dict[str, object]]:
    endpoints: dict[str, dict[str, object]] = {}
    paths = document["paths"]
    assert isinstance(paths, dict)
    for path, path_item in paths.items():
        assert isinstance(path_item, dict)
        for method, operation in path_item.items():
            if method not in {"get", "post", "delete"}:
                continue
            assert isinstance(operation, dict)
            operation_id = operation.get("operationId")
            if not isinstance(operation_id, str):
                continue
            security = operation.get("security", [])
            scheme = None
            if isinstance(security, list) and security and isinstance(security[0], dict):
                scheme = next(iter(security[0]), None)
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
    return dict(sorted(endpoints.items()))


def parse_typescript(source: str) -> dict[str, dict[str, object]]:
    encoded = source.split("export const endpoints = ", 1)[1].split(" as const;", 1)[0]
    return json.loads(encoded)


def parse_python(source: str) -> dict[str, dict[str, object]]:
    tree = ast.parse(source)
    for node in tree.body:
        if isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            if node.target.id == "ENDPOINTS" and node.value is not None:
                value = ast.literal_eval(node.value)
                assert isinstance(value, dict)
                return value
    raise AssertionError("generated Python endpoint map omitted ENDPOINTS")


def parse_go(source: str) -> dict[str, dict[str, object]]:
    names = dict(
        re.findall(r'^\s*(Operation\w+)\s+OperationID\s+=\s+"([^"]+)"$', source, re.M)
    )
    pattern = re.compile(
        r'^\s*(Operation\w+):\s+\{Method: "([^"]+)", Path: "([^"]+)", '
        r'Authentication: "([^"]+)", Mutation: (true|false)\},$',
        re.M,
    )
    return {
        names[symbol]: {
            "method": method,
            "path": path,
            "authentication": authentication,
            "mutation": mutation == "true",
        }
        for symbol, method, path, authentication, mutation in pattern.findall(source)
    }


def parse_java(source: str) -> dict[str, dict[str, object]]:
    pattern = re.compile(
        r'^\s*[A-Z0-9_]+\("([^"]+)", "([^"]+)", "([^"]+)", '
        r'Authentication\.([A-Z_]+), (true|false)\)[,;]$',
        re.M,
    )
    authentication_names = {
        "PUBLIC": "public",
        "API_KEY": "api_key",
        "SESSION_BEARER": "session_bearer",
    }
    return {
        operation: {
            "method": method,
            "path": path,
            "authentication": authentication_names[authentication],
            "mutation": mutation == "true",
        }
        for operation, method, path, authentication, mutation in pattern.findall(source)
    }


def parse_dotnet(source: str) -> dict[str, dict[str, object]]:
    pattern = re.compile(
        r'^\s*OperationId\.\w+ => new\("([^"]+)", "([^"]+)", "([^"]+)", '
        r'Authentication\.(\w+), (true|false)\),$',
        re.M,
    )
    authentication_names = {
        "Public": "public",
        "ApiKey": "api_key",
        "SessionBearer": "session_bearer",
    }
    return {
        operation: {
            "method": method,
            "path": path,
            "authentication": authentication_names[authentication],
            "mutation": mutation == "true",
        }
        for operation, method, path, authentication, mutation in pattern.findall(source)
    }


def main() -> None:
    raw, document = contract_export()
    digest = hashlib.sha256(raw).hexdigest()
    sources = {name: path.read_text() for name, path in GENERATED.items()}
    for name, source in sources.items():
        if f"OpenAPI SHA-256: {digest}" not in source:
            raise SystemExit(f"{name} is stale: missing OpenAPI digest {digest}")

    expected = canonical_endpoints(document)
    projections = {
        "typescript endpoints": parse_typescript(sources["typescript endpoints"]),
        "python endpoints": parse_python(sources["python endpoints"]),
        "go endpoints": parse_go(sources["go endpoints"]),
        "java endpoints": parse_java(sources["java endpoints"]),
        "dotnet endpoints": parse_dotnet(sources["dotnet endpoints"]),
    }
    for name, projection in projections.items():
        if projection != expected:
            missing = sorted(set(expected) - set(projection))
            extra = sorted(set(projection) - set(expected))
            mismatched = sorted(
                operation
                for operation in set(expected) & set(projection)
                if expected[operation] != projection[operation]
            )
            raise SystemExit(
                f"{name} drifted from rrd-contract: "
                f"missing={missing} extra={extra} mismatched={mismatched}"
            )
    print(f"generated surface parity: {len(expected)} HTTP operations at OpenAPI {digest}")


if __name__ == "__main__":
    main()
