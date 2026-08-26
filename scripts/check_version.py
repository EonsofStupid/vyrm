#!/usr/bin/env python3
"""Fail when RRFlow release-version declarations drift from VERSION."""

from __future__ import annotations

import json
import re
import sys
import tomllib
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
VERSION = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
SEMVER = re.compile(
    r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)


def load_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as source:
        return tomllib.load(source)


def fail(message: str, failures: list[str]) -> None:
    failures.append(message)


def main() -> int:
    failures: list[str] = []
    if not SEMVER.fullmatch(VERSION):
        fail(f"VERSION is not valid SemVer: {VERSION!r}", failures)

    workspace = load_toml(ROOT / "Cargo.toml")
    workspace_version = workspace["workspace"]["package"]["version"]  # type: ignore[index]
    if workspace_version != VERSION:
        fail(f"Cargo workspace version is {workspace_version!r}, expected {VERSION!r}", failures)

    manifests = sorted((ROOT / "crates").glob("*/Cargo.toml"))
    for manifest in manifests:
        package = load_toml(manifest)["package"]  # type: ignore[index]
        if package.get("version") != {"workspace": True}:  # type: ignore[union-attr]
            fail(f"{manifest.relative_to(ROOT)} must use `version.workspace = true`", failures)

    typescript = json.loads((ROOT / "sdks/typescript/package.json").read_text(encoding="utf-8"))
    if typescript["version"] != VERSION:
        fail(f"TypeScript SDK version is {typescript['version']!r}, expected {VERSION!r}", failures)

    python_project = load_toml(ROOT / "sdks/python/pyproject.toml")
    python_version = python_project["project"]["version"]  # type: ignore[index]
    if python_version != VERSION:
        fail(f"Python SDK version is {python_version!r}, expected {VERSION!r}", failures)

    python_lock = load_toml(ROOT / "sdks/python/uv.lock")
    locked_versions = [
        package["version"]
        for package in python_lock["package"]  # type: ignore[index]
        if package["name"] == "rrflow-rrd-client"
    ]
    if locked_versions != [VERSION]:
        fail(f"Python SDK lock versions are {locked_versions!r}, expected [{VERSION!r}]", failures)

    java_root = ET.parse(ROOT / "sdks/java/pom.xml").getroot()
    namespace = {"m": "http://maven.apache.org/POM/4.0.0"}
    java_version = java_root.findtext("m:version", namespaces=namespace)
    if java_version != VERSION:
        fail(f"Java SDK version is {java_version!r}, expected {VERSION!r}", failures)

    dotnet_root = ET.parse(
        ROOT / "sdks/dotnet/src/Rrflow.Rrd.Client/Rrflow.Rrd.Client.csproj"
    ).getroot()
    dotnet_version = dotnet_root.findtext("./PropertyGroup/Version")
    if dotnet_version != VERSION:
        fail(f".NET SDK version is {dotnet_version!r}, expected {VERSION!r}", failures)

    contract = json.loads(
        (ROOT / "crates/rrd-contract/fixtures/public-contract-v1.json").read_text(
            encoding="utf-8"
        )
    )
    implementation_version = contract["service"]["implementation_version"]
    if implementation_version != VERSION:
        fail(
            f"public contract implementation version is {implementation_version!r}, "
            f"expected {VERSION!r}",
            failures,
        )

    connectome = ROOT / "apps/connectome"
    if connectome.exists():
        package_path = connectome / "package.json"
        tauri_manifest = connectome / "src-tauri/Cargo.toml"
        if not package_path.is_file() or not tauri_manifest.is_file():
            fail("apps/connectome is present but lacks canonical package manifests", failures)
        else:
            connectome_package = json.loads(package_path.read_text(encoding="utf-8"))
            if connectome_package.get("version") != VERSION:
                fail(
                    f"Connectome package version is {connectome_package.get('version')!r}, "
                    f"expected {VERSION!r}",
                    failures,
                )
            connectome_tauri = load_toml(tauri_manifest)["package"]  # type: ignore[index]
            if connectome_tauri.get("version") != VERSION:  # type: ignore[union-attr]
                fail(
                    f"Connectome Tauri version is {connectome_tauri.get('version')!r}, "  # type: ignore[union-attr]
                    f"expected {VERSION!r}",
                    failures,
                )

    if failures:
        for message in failures:
            print(f"version-policy: ERROR: {message}", file=sys.stderr)
        return 1

    print(f"version-policy: OK: RRFlow release train is {VERSION}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
