#!/usr/bin/env python3
"""Run every supported SDK against one hermetic real RRD daemon corpus."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CORPUS = ROOT / "fixtures/rrd-sdk-conformance-v1.json"
LANGUAGE_COMMANDS = {
    "rust": [
        [
            "cargo",
            "test",
            "-p",
            "rrd-client",
            "--test",
            "sdk_conformance",
            "--locked",
            "--",
            "--nocapture",
        ]
    ],
    "typescript": [
        ["pnpm", "--dir", "sdks/typescript", "install", "--frozen-lockfile"],
        ["pnpm", "--dir", "sdks/typescript", "exec", "tsx", "tests/sdk_conformance.ts"],
    ],
    "python": [
        [
            "uv",
            "--directory",
            "sdks/python",
            "run",
            "--frozen",
            "python",
            "tests/sdk_conformance.py",
        ]
    ],
    "go": [["go", "-C", "sdks/go", "run", "./cmd/conformance"]],
    "java": [
        ["mvn", "-f", "sdks/java/pom.xml", "--batch-mode", "-Dtest=SdkConformanceTest", "test"]
    ],
    "dotnet": [
        [
            "dotnet",
            "restore",
            "sdks/dotnet/tests/Rrflow.Rrd.Client.Tests/Rrflow.Rrd.Client.Tests.csproj",
            "--locked-mode",
        ],
        [
            "dotnet",
            "run",
            "--project",
            "sdks/dotnet/tests/Rrflow.Rrd.Client.Tests/Rrflow.Rrd.Client.Tests.csproj",
            "--no-restore",
        ],
    ],
}


def require_toolchains() -> None:
    missing = sorted(
        {
            commands[0][0]
            for commands in LANGUAGE_COMMANDS.values()
            if shutil.which(commands[0][0]) is None
        }
    )
    if missing:
        raise RuntimeError(f"supported SDK toolchains are missing: {', '.join(missing)}")


def run(command: list[str], env: dict[str, str] | None = None) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def wait_for_manifest(path: Path, process: subprocess.Popen[bytes]) -> dict[str, object]:
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if path.is_file():
            mode = stat.S_IMODE(path.stat().st_mode)
            if mode != 0o600:
                raise RuntimeError(f"harness manifest mode is {mode:o}, expected 600")
            return json.loads(path.read_text(encoding="utf-8"))
        status = process.poll()
        if status is not None:
            raise RuntimeError(f"SDK conformance harness exited before readiness with {status}")
        time.sleep(0.025)
    raise RuntimeError("SDK conformance harness did not publish readiness within 30 seconds")


def main() -> None:
    require_toolchains()
    corpus_bytes = CORPUS.read_bytes()
    corpus = json.loads(corpus_bytes)
    required = {
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
    if set(corpus["required_domains"]) != required:
        raise RuntimeError("shared SDK corpus does not cover every required domain")
    run(
        [
            "cargo",
            "build",
            "-p",
            "rrd-client",
            "--example",
            "sdk_conformance_server",
            "--profile",
            "test",
            "--locked",
        ]
    )
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    harness = (
        target
        / "debug"
        / "examples"
        / ("sdk_conformance_server.exe" if os.name == "nt" else "sdk_conformance_server")
    )
    if not harness.is_file():
        raise RuntimeError(f"SDK conformance harness binary is absent: {harness}")
    with tempfile.TemporaryDirectory(prefix="rrd-sdk-conformance-") as temporary:
        temporary_path = Path(temporary)
        manifest_path = temporary_path / "manifest.json"
        shutdown_path = temporary_path / "shutdown"
        process = subprocess.Popen(
            [
                str(harness),
                "--corpus",
                str(CORPUS),
                "--manifest",
                str(manifest_path),
                "--shutdown",
                str(shutdown_path),
            ],
            cwd=ROOT,
        )
        try:
            manifest = wait_for_manifest(manifest_path, process)
            digest = hashlib.sha256(corpus_bytes).hexdigest()
            if manifest.get("corpus_sha256") != digest:
                raise RuntimeError("harness and orchestrator corpus digests differ")
            retry_urls = manifest.get("retry_base_urls")
            if not isinstance(retry_urls, dict) or set(retry_urls) != set(LANGUAGE_COMMANDS):
                raise RuntimeError("harness retry endpoint ownership differs from supported SDKs")
            environment = os.environ.copy()
            environment["RRD_SDK_CONFORMANCE_MANIFEST"] = str(manifest_path)
            for language, commands in LANGUAGE_COMMANDS.items():
                for command in commands:
                    run(command, environment)
                print(f"SDK conformance OK: language={language} corpus_sha256={digest}", flush=True)
        finally:
            shutdown_path.touch(exist_ok=True)
            try:
                status = process.wait(timeout=15)
            except subprocess.TimeoutExpired as error:
                process.kill()
                process.wait(timeout=5)
                raise RuntimeError(
                    "SDK conformance harness did not stop within 15 seconds"
                ) from error
            if status != 0 and sys.exc_info()[0] is None:
                raise RuntimeError(f"SDK conformance harness exited with {status}")


if __name__ == "__main__":
    main()
