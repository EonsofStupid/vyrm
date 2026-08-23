#!/usr/bin/env python3
"""Bounded Vyrm/native versus SurrealDB claim-log differential.

This is deliberately not a general database benchmark. It compares one
frontier-AI runtime primitive: atomic durable claim batches, an authoritative
sequence watermark, bounded ordered replay, full-corpus verification, and a
clean restart. Vyrm runs embedded in its benchmark child; SurrealDB runs as a
local SurrealKV server and is measured end-to-end over its HTTP SQL endpoint.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import shutil
import signal
import socket
import statistics
import subprocess
import tempfile
import time
from typing import Any


FORMAT_VERSION = 1


def positive(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return parsed


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trials", type=positive, default=3)
    parser.add_argument("--operations", type=positive, default=2_048)
    parser.add_argument("--batch-size", type=positive, default=64)
    parser.add_argument("--reads", type=positive, default=1_024)
    parser.add_argument("--read-width", type=positive, default=32)
    parser.add_argument("--surreal", default=shutil.which("surreal"))
    parser.add_argument(
        "--vyrm-benchmark",
        default=os.environ.get(
            "VYRM_ENGINE_BENCHMARK",
            "/workspace/warden-storage/cache/wardenop/cargo-target/release/examples/engine_benchmark",
        ),
    )
    parser.add_argument("--output", type=Path)
    parsed = parser.parse_args()
    if parsed.batch_size > parsed.operations or parsed.read_width > parsed.operations:
        parser.error("batch size and read width cannot exceed operations")
    if not parsed.surreal:
        parser.error("SurrealDB binary was not found; pass --surreal")
    return parsed


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def version(binary: Path) -> str:
    return subprocess.run(
        [str(binary), "version"], check=True, capture_output=True, text=True
    ).stdout.strip()


def free_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


class SurrealProcess:
    def __init__(self, binary: Path, storage: Path):
        self.binary = binary
        self.storage = storage.resolve()
        self.port = free_port()
        self.process: subprocess.Popen[bytes] | None = None
        self.connection: http.client.HTTPConnection | None = None
        self.peak_rss_kib = 0

    def start(self) -> int:
        started = time.perf_counter_ns()
        environment = os.environ.copy()
        environment["SURREAL_ONLINE_VERSION_CHECK"] = "false"
        self.process = subprocess.Popen(
            [
                str(self.binary),
                "start",
                "--no-banner",
                "-l",
                "error",
                "--bind",
                f"127.0.0.1:{self.port}",
                "-u",
                "root",
                "-p",
                "vyrm-benchmark",
                f"surrealkv://{self.storage}",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            env=environment,
        )
        deadline = time.monotonic() + 30
        last_error: Exception | None = None
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                detail = (self.process.stderr.read() if self.process.stderr else b"").decode(
                    "utf-8", "replace"
                )
                raise RuntimeError(f"SurrealDB exited during startup: {detail}")
            try:
                self._connect()
                self.query(
                    "DEFINE NAMESPACE IF NOT EXISTS vyrm;"
                    "USE NS vyrm;"
                    "DEFINE DATABASE IF NOT EXISTS claim_benchmark;",
                    scoped=False,
                )
                response = self.query("RETURN true;")
                if response[-1].get("result") is True:
                    self.sample_rss()
                    return time.perf_counter_ns() - started
            except (ConnectionError, OSError, RuntimeError) as error:
                last_error = error
                self.close_connection()
                time.sleep(0.025)
        raise RuntimeError(f"SurrealDB did not become ready: {last_error}")

    def _connect(self) -> None:
        self.connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=30)
        self.connection.connect()
        assert self.connection.sock is not None
        self.connection.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)

    def query(
        self, sql: str, *, allow_errors: bool = False, scoped: bool = True
    ) -> list[dict[str, Any]]:
        if self.connection is None:
            self._connect()
        assert self.connection is not None
        authorization = base64.b64encode(b"root:vyrm-benchmark").decode("ascii")
        headers = {
            "Authorization": f"Basic {authorization}",
            "Accept": "application/json",
            "Content-Type": "text/plain",
        }
        if scoped:
            headers["Surreal-NS"] = "vyrm"
            headers["Surreal-DB"] = "claim_benchmark"
        self.connection.request(
            "POST",
            "/sql",
            body=sql.encode("utf-8"),
            headers=headers,
        )
        response = self.connection.getresponse()
        payload = response.read()
        if response.status != 200:
            raise RuntimeError(f"SurrealDB HTTP {response.status}: {payload[:500]!r}")
        decoded = json.loads(payload)
        if not isinstance(decoded, list):
            raise RuntimeError("SurrealDB response is not a statement array")
        errors = [item for item in decoded if item.get("status") != "OK"]
        if errors and not allow_errors:
            raise RuntimeError(f"SurrealDB statement failed: {errors[:2]!r}")
        self.sample_rss()
        return decoded

    def sample_rss(self) -> None:
        if self.process is None:
            return
        status = Path(f"/proc/{self.process.pid}/status")
        try:
            for line in status.read_text().splitlines():
                if line.startswith("VmHWM:"):
                    self.peak_rss_kib = max(self.peak_rss_kib, int(line.split()[1]))
                    return
        except FileNotFoundError:
            return

    def close_connection(self) -> None:
        if self.connection is not None:
            self.connection.close()
            self.connection = None

    def stop(self) -> None:
        self.sample_rss()
        self.close_connection()
        if self.process is None:
            return
        self.process.send_signal(signal.SIGTERM)
        try:
            self.process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        if self.process.returncode not in (0, -signal.SIGTERM):
            detail = (self.process.stderr.read() if self.process.stderr else b"").decode(
                "utf-8", "replace"
            )
            raise RuntimeError(f"SurrealDB stopped with {self.process.returncode}: {detail}")
        self.process = None


def initialize_surreal(server: SurrealProcess) -> None:
    server.query(
        " ".join(
            [
                "DEFINE TABLE claim SCHEMAFULL;",
                "DEFINE FIELD ordinal ON claim TYPE int;",
                "DEFINE FIELD subject ON claim TYPE string;",
                "DEFINE FIELD predicate ON claim TYPE string;",
                "DEFINE FIELD object ON claim TYPE string;",
                "DEFINE FIELD valid_from ON claim TYPE int;",
                "DEFINE FIELD tx_time ON claim TYPE int;",
                "DEFINE FIELD actor ON claim TYPE string;",
                "DEFINE FIELD session ON claim TYPE string;",
                "DEFINE INDEX claim_ordinal ON claim FIELDS ordinal UNIQUE;",
                "DEFINE TABLE sequence SCHEMAFULL;",
                "DEFINE FIELD ordinal ON sequence TYPE int;",
                "DEFINE FIELD claim_id ON sequence TYPE record<claim>;",
                "DEFINE INDEX sequence_ordinal ON sequence FIELDS ordinal UNIQUE;",
                "DEFINE TABLE meta SCHEMAFULL;",
                "DEFINE FIELD value ON meta TYPE int;",
            ]
        )
    )


def claim_sql(ordinal: int) -> str:
    index = ordinal - 1
    content = {
        "ordinal": ordinal,
        "subject": f"benchmark:{index:012}",
        "predicate": "value",
        "object": f"payload-{index:012}",
        "valid_from": ordinal,
        "tx_time": ordinal,
        "actor": "benchmark",
        "session": "engine-promotion-v1",
    }
    encoded = json.dumps(content, separators=(",", ":"))
    return (
        f"CREATE claim:{ordinal} CONTENT {encoded} RETURN NONE;"
        f"CREATE sequence:{ordinal} CONTENT "
        f"{{ordinal:{ordinal},claim_id:claim:{ordinal}}} RETURN NONE;"
    )


def duration_ns(value: str) -> int:
    units = (("ns", 1), ("µs", 1_000), ("ms", 1_000_000), ("s", 1_000_000_000))
    for suffix, multiplier in units:
        if value.endswith(suffix):
            return round(float(value[: -len(suffix)]) * multiplier)
    raise RuntimeError(f"unknown SurrealDB duration {value!r}")


def statement_time_ns(response: list[dict[str, Any]]) -> int:
    return sum(duration_ns(str(statement["time"])) for statement in response)


def write_surreal(
    server: SurrealProcess, operations: int, batch_size: int
) -> tuple[list[int], list[int], int]:
    samples: list[int] = []
    server_samples: list[int] = []
    overall = time.perf_counter_ns()
    for first in range(1, operations + 1, batch_size):
        last = min(first + batch_size - 1, operations)
        sql = "BEGIN TRANSACTION;" + "".join(
            claim_sql(ordinal) for ordinal in range(first, last + 1)
        )
        sql += f"UPSERT meta:sequence CONTENT {{value:{last}}} RETURN NONE;COMMIT TRANSACTION;"
        started = time.perf_counter_ns()
        response = server.query(sql)
        samples.append(time.perf_counter_ns() - started)
        server_samples.append(statement_time_ns(response))
    return samples, server_samples, time.perf_counter_ns() - overall


def select_claims(server: SurrealProcess, after: int, through: int) -> list[dict[str, Any]]:
    response = server.query(
        "SELECT ordinal,subject,predicate,object,valid_from,tx_time,actor,session "
        f"FROM claim WHERE ordinal > {after} AND ordinal <= {through} ORDER BY ordinal;"
    )
    result = response[-1].get("result")
    if not isinstance(result, list):
        raise RuntimeError("SurrealDB claim query did not return an array")
    return result


def verify_surreal(server: SurrealProcess, operations: int, read_width: int) -> None:
    sequence = server.query("SELECT VALUE value FROM ONLY meta:sequence;")[-1].get("result")
    if sequence != operations:
        raise RuntimeError(f"SurrealDB sequence differs: {sequence!r} != {operations}")
    after = 0
    while after < operations:
        through = min(after + read_width, operations)
        rows = select_claims(server, after, through)
        if len(rows) != through - after:
            raise RuntimeError("SurrealDB verification page has the wrong cardinality")
        for offset, row in enumerate(rows):
            index = after + offset
            expected = {
                "ordinal": index + 1,
                "subject": f"benchmark:{index:012}",
                "predicate": "value",
                "object": f"payload-{index:012}",
                "valid_from": index + 1,
                "tx_time": index + 1,
                "actor": "benchmark",
                "session": "engine-promotion-v1",
            }
            if row != expected:
                raise RuntimeError(f"SurrealDB claim differs at ordinal {index + 1}")
        after = through


def read_surreal(
    server: SurrealProcess, operations: int, reads: int, read_width: int
) -> tuple[list[int], list[int], int]:
    samples: list[int] = []
    server_samples: list[int] = []
    span = operations - read_width + 1
    overall = time.perf_counter_ns()
    for iteration in range(reads):
        after = (iteration * 7_919) % span
        started = time.perf_counter_ns()
        response = server.query(
            "SELECT ordinal,subject,predicate,object,valid_from,tx_time,actor,session "
            f"FROM claim WHERE ordinal > {after} AND ordinal <= {after + read_width} "
            "ORDER BY ordinal;"
        )
        result = response[-1].get("result")
        if not isinstance(result, list):
            raise RuntimeError("SurrealDB bounded replay did not return an array")
        rows = result
        if len(rows) != read_width:
            raise RuntimeError("SurrealDB bounded replay has the wrong cardinality")
        samples.append(time.perf_counter_ns() - started)
        server_samples.append(statement_time_ns(response))
    return samples, server_samples, time.perf_counter_ns() - overall


def percentile(sorted_samples: list[int], fraction: float) -> int:
    index = max(0, min(len(sorted_samples) - 1, math.ceil(fraction * len(sorted_samples)) - 1))
    return sorted_samples[index]


def latency(samples: list[int]) -> dict[str, int]:
    ordered = sorted(samples)
    return {
        "samples": len(ordered),
        "minimum_ns": ordered[0],
        "p50_ns": percentile(ordered, 0.50),
        "p95_ns": percentile(ordered, 0.95),
        "p99_ns": percentile(ordered, 0.99),
        "maximum_ns": ordered[-1],
    }


def footprint(root: Path) -> dict[str, int]:
    apparent = allocated = files = 0
    for path in root.rglob("*"):
        if path.is_symlink():
            raise RuntimeError(f"footprint refuses symbolic link {path}")
        if path.is_file():
            status = path.stat()
            apparent += status.st_size
            allocated += status.st_blocks * 512
            files += 1
    return {"apparent_bytes": apparent, "allocated_bytes": allocated, "files": files}


def surreal_trial(binary: Path, root: Path, config: dict[str, int]) -> dict[str, Any]:
    storage = root / "surreal"
    server = SurrealProcess(binary, storage)
    server.start()
    initialize_surreal(server)
    write_samples, server_write_samples, write_elapsed = write_surreal(
        server, config["operations"], config["batch_size"]
    )
    write_peak = server.peak_rss_kib
    server.stop()
    reopened = footprint(storage)

    server = SurrealProcess(binary, storage)
    recovery_ns = server.start()
    verify_surreal(server, config["operations"], config["read_width"])
    read_samples, server_read_samples, read_elapsed = read_surreal(
        server, config["operations"], config["reads"], config["read_width"]
    )
    peak = server.peak_rss_kib
    server.stop()
    return {
        "backend": "surrealdb_surrealkv_http",
        "correctness_verified": True,
        "write_operations_per_second": config["operations"] / (write_elapsed / 1e9),
        "write_batch_latency": latency(write_samples),
        "server_write_batch_latency": latency(server_write_samples),
        "server_write_operations_per_second": config["operations"]
        / (sum(server_write_samples) / 1e9),
        "read_operations_per_second": config["reads"] / (read_elapsed / 1e9),
        "read_latency": latency(read_samples),
        "server_read_latency": latency(server_read_samples),
        "server_read_operations_per_second": config["reads"]
        / (sum(server_read_samples) / 1e9),
        "recovery_ns": recovery_ns,
        "write_peak_rss_kib": write_peak,
        "peak_rss_kib": peak,
        "reopened_footprint": reopened,
        "semantic_sequence": config["operations"],
    }


def vyrm_trial(binary: Path, root: Path, config: dict[str, int]) -> dict[str, Any]:
    command = [
        str(binary),
        "--child",
        "native",
        "--path",
        str(root / "native"),
        "--operations",
        str(config["operations"]),
        "--batch-size",
        str(config["batch_size"]),
        "--reads",
        str(config["reads"]),
        "--read-width",
        str(config["read_width"]),
        "--trials",
        "1",
    ]
    completed = subprocess.run(command, check=True, capture_output=True)
    return json.loads(completed.stdout)


def median_backend(backend: str, trials: list[dict[str, Any]]) -> dict[str, Any]:
    metric = lambda path: statistics.median(
        trial[path[0]][path[1]] if len(path) == 2 else trial[path[0]] for trial in trials
    )
    aggregated = {
        "backend": backend,
        "correctness_verified": all(trial["correctness_verified"] for trial in trials),
        "write_operations_per_second": metric(("write_operations_per_second",)),
        "write_p95_ns": metric(("write_batch_latency", "p95_ns")),
        "read_operations_per_second": metric(("read_operations_per_second",)),
        "read_p95_ns": metric(("read_latency", "p95_ns")),
        "recovery_ns": metric(("recovery_ns",)),
        "peak_rss_kib": metric(("peak_rss_kib",)),
        "reopened_allocated_bytes": statistics.median(
            trial["footprint"]["reopened"]["allocated_bytes"]
            if backend == "vyrm_native_embedded"
            else trial["reopened_footprint"]["allocated_bytes"]
            for trial in trials
        ),
    }
    if backend != "vyrm_native_embedded":
        aggregated.update(
            {
                "server_write_operations_per_second": metric(
                    ("server_write_operations_per_second",)
                ),
                "server_write_p95_ns": metric(("server_write_batch_latency", "p95_ns")),
                "server_read_operations_per_second": metric(
                    ("server_read_operations_per_second",)
                ),
                "server_read_p95_ns": metric(("server_read_latency", "p95_ns")),
            }
        )
    return aggregated


def main() -> None:
    args = arguments()
    surreal_binary = Path(args.surreal).resolve()
    vyrm_binary = Path(args.vyrm_benchmark).resolve()
    if not surreal_binary.is_file() or not vyrm_binary.is_file():
        raise SystemExit("both --surreal and --vyrm-benchmark must name existing binaries")
    config = {
        "trials": args.trials,
        "operations": args.operations,
        "batch_size": args.batch_size,
        "reads": args.reads,
        "read_width": args.read_width,
    }
    native_trials: list[dict[str, Any]] = []
    surreal_trials: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="vyrm-surreal-differential-") as temporary:
        root = Path(temporary)
        for trial in range(args.trials):
            trial_root = root / f"trial-{trial}"
            trial_root.mkdir()
            if trial % 2 == 0:
                native_trials.append(vyrm_trial(vyrm_binary, trial_root, config))
                surreal_trials.append(surreal_trial(surreal_binary, trial_root, config))
            else:
                surreal_trials.append(surreal_trial(surreal_binary, trial_root, config))
                native_trials.append(vyrm_trial(vyrm_binary, trial_root, config))
    native = median_backend("vyrm_native_embedded", native_trials)
    surreal = median_backend("surrealdb_surrealkv_http", surreal_trials)
    ratios = {
        "vyrm_to_surreal_write_throughput": native["write_operations_per_second"]
        / surreal["write_operations_per_second"],
        "vyrm_to_surreal_read_throughput": native["read_operations_per_second"]
        / surreal["read_operations_per_second"],
        "vyrm_to_surreal_write_p95": native["write_p95_ns"] / surreal["write_p95_ns"],
        "vyrm_to_surreal_read_p95": native["read_p95_ns"] / surreal["read_p95_ns"],
        "vyrm_to_surreal_recovery": native["recovery_ns"] / surreal["recovery_ns"],
        "vyrm_to_surreal_peak_rss": native["peak_rss_kib"] / surreal["peak_rss_kib"],
        "vyrm_to_surreal_reopened_allocated": native["reopened_allocated_bytes"]
        / surreal["reopened_allocated_bytes"],
        "vyrm_to_surreal_server_write_throughput": native["write_operations_per_second"]
        / surreal["server_write_operations_per_second"],
        "vyrm_to_surreal_server_read_throughput": native["read_operations_per_second"]
        / surreal["server_read_operations_per_second"],
        "vyrm_to_surreal_server_write_p95": native["write_p95_ns"]
        / surreal["server_write_p95_ns"],
        "vyrm_to_surreal_server_read_p95": native["read_p95_ns"]
        / surreal["server_read_p95_ns"],
    }
    all_measured_cells_favor_vyrm = (
        native["correctness_verified"]
        and surreal["correctness_verified"]
        and ratios["vyrm_to_surreal_write_throughput"] >= 1
        and ratios["vyrm_to_surreal_read_throughput"] >= 1
        and ratios["vyrm_to_surreal_server_write_throughput"] >= 1
        and ratios["vyrm_to_surreal_server_read_throughput"] >= 1
        and all(
            ratios[name] <= 1
            for name in (
                "vyrm_to_surreal_write_p95",
                "vyrm_to_surreal_read_p95",
                "vyrm_to_surreal_recovery",
                "vyrm_to_surreal_peak_rss",
                "vyrm_to_surreal_reopened_allocated",
                "vyrm_to_surreal_server_write_p95",
                "vyrm_to_surreal_server_read_p95",
            )
        )
    )
    evidence = {
        "format_version": FORMAT_VERSION,
        "schema": "vyrm.surrealdb-claim-differential.v1",
        "measured_at_unix_ms": time.time_ns() // 1_000_000,
        "platform": {"architecture": os.uname().machine, "operating_system": os.uname().sysname},
        "config": config,
        "surrealdb": {
            "version": version(surreal_binary),
            "binary_sha256": sha256_file(surreal_binary),
            "storage": "surrealkv",
            "transport": "persistent localhost HTTP SQL",
            "timing": "end-to-end client timing plus summed server-reported statement execution time",
            "durability": "SurrealDB 3.x default disk sync",
        },
        "contract": (
            "same claim fields, atomic authoritative batch and sequence watermark, ordered bounded "
            "replay, complete paged object verification, clean restart; Vyrm is embedded while "
            "SurrealDB is a local server, and client RSS is excluded only for SurrealDB"
        ),
        "aggregation": "median of isolated alternating trials",
        "native_trials": native_trials,
        "surreal_trials": surreal_trials,
        "native": native,
        "surreal": surreal,
        "ratios": ratios,
        "bounded_verdict": {
            "all_measured_cells_favor_vyrm": all_measured_cells_favor_vyrm,
            "general_database_superiority": False,
        },
    }
    encoded = json.dumps(evidence, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded)
        print(f"wrote benchmark evidence to {args.output}", file=os.sys.stderr)
    print(encoded, end="")


if __name__ == "__main__":
    main()
