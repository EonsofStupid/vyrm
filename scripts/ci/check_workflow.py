#!/usr/bin/env python3
"""Validate durable CI policy without coupling engine tests to workflow layout."""

from __future__ import annotations

import re
import shlex
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CALLER = ROOT / ".github/workflows/ci.yml"
REUSABLE = ROOT / ".github/workflows/ci-reusable.yml"


class PolicyError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise PolicyError(message)


def job_blocks(workflow: str) -> dict[str, str]:
    marker = "\njobs:\n"
    require(marker in workflow, "reusable workflow must define jobs")
    jobs = workflow.split(marker, 1)[1]
    matches = list(re.finditer(r"(?m)^  ([a-z][a-z0-9-]*):\s*$", jobs))
    require(matches, "reusable workflow must contain at least one job")
    return {
        match.group(1): jobs[match.end() : matches[index + 1].start()]
        if index + 1 < len(matches)
        else jobs[match.end() :]
        for index, match in enumerate(matches)
    }


def workspace_packages() -> set[str]:
    with (ROOT / "Cargo.toml").open("rb") as source:
        workspace = tomllib.load(source)["workspace"]
    return {Path(member).name for member in workspace["members"]}


def optional_feature_packages() -> set[str]:
    packages = set()
    for manifest in (ROOT / "crates").glob("*/Cargo.toml"):
        with manifest.open("rb") as source:
            document = tomllib.load(source)
        features = set(document.get("features", {})) - {"default"}
        if features:
            packages.add(document["package"]["name"])
    return packages


def verify_action_pins(*workflows: str) -> None:
    for workflow in workflows:
        for action in re.findall(r"(?m)^\s*(?:-\s*)?uses:\s+([^\s#]+)", workflow):
            if action.startswith("./"):
                continue
            require("@" in action, f"external action is unpinned: {action}")
            repository, reference = action.rsplit("@", 1)
            require("/" in repository, f"external action repository is invalid: {action}")
            require(
                re.fullmatch(r"[0-9a-fA-F]{40}", reference) is not None,
                f"external action must use a full immutable SHA: {action}",
            )


def verify_arc_policy() -> None:
    deployment = ROOT / "deploy/ci/github-actions/arc"
    controller = (deployment / "controller-values.yaml").read_text()
    standard = (deployment / "rrflow-standard.values.yaml").read_text()
    heavy = (deployment / "rrflow-heavy.values.yaml").read_text()
    installer = (ROOT / "scripts/ci/install-arc.sh").read_text()

    require("0.14.2@sha256:" in controller, "ARC controller image must be digest pinned")
    require(
        "repository_visibility" in installer
        and '!= "PRIVATE"' in installer
        and "refusing to register self-hosted runners" in installer,
        "ARC installer must refuse non-private repositories",
    )
    for name, values, maximum in [
        ("standard", standard, "maxRunners: 2"),
        ("heavy", heavy, "maxRunners: 1"),
    ]:
        require(
            "minRunners: 0" in values and maximum in values,
            f"{name} ARC scale set must stay within physical capacity",
        )
        require(
            "githubConfigSecret: rrflow-arc-github" in values,
            f"{name} ARC scale set must use the pre-created secret",
        )
        require(
            "@sha256:" in values and ":latest" not in values,
            f"{name} ARC runner images must be immutable",
        )
    require(
        'runnerScaleSetName: rrflow-standard' in standard
        and 'cpu: "16"' in standard
        and "memory: 20Gi" in standard,
        "standard ARC allocation must fit the documented host capacity",
    )
    require(
        'runnerScaleSetName: rrflow-heavy' in heavy
        and 'cpu: "46"' in heavy
        and "memory: 60Gi" in heavy
        and 'cpu: "2"' in heavy
        and "memory: 4Gi" in heavy,
        "heavy ARC runner plus Docker sidecar must fit the documented host capacity",
    )


def verify_engine_suites(block: str) -> tuple[int, int]:
    suites = re.findall(
        r'(?m)^\s+- name: ([a-z][a-z0-9-]*)\s*$\n\s+packages: "([^"]+)"\s*$',
        block,
    )
    required_suites = {
        "kernel-storage",
        "query-retrieval",
        "engine-authority",
        "service-protocol",
        "product-operations",
    }
    observed_suites = {name for name, _ in suites}
    require(observed_suites == required_suites, "engine suites must follow product boundaries")

    routed: list[str] = []
    for name, selection in suites:
        arguments = shlex.split(selection)
        require(
            len(arguments) % 2 == 0
            and all(arguments[index] == "-p" for index in range(0, len(arguments), 2)),
            f"engine suite {name} must contain only explicit -p package selections",
        )
        routed.extend(arguments[index] for index in range(1, len(arguments), 2))

    expected = workspace_packages()
    duplicates = sorted({package for package in routed if routed.count(package) > 1})
    require(not duplicates, f"workspace packages appear in multiple engine suites: {duplicates}")
    require(
        set(routed) == expected,
        f"engine suite coverage mismatch: missing={sorted(expected - set(routed))} "
        f"unexpected={sorted(set(routed) - expected)}",
    )
    require(
        "cargo test $PACKAGE_SELECTION --all-targets --locked" in block
        and "cargo clippy $PACKAGE_SELECTION --all-targets --locked -- -D warnings" in block,
        "every engine suite must run locked all-target tests and strict Clippy",
    )
    require(
        "matrix.name == 'service-protocol'" in block
        and "cargo build -p rrflow-cli --bin rrd-estate-controller --locked" in block,
        "the service-protocol suite must explicitly build its controller fixture",
    )
    return len(suites), len(routed)


def verify_optional_features(optional: str, pgvector: str) -> int:
    matrix = optional.split("        package:\n", 1)
    require(len(matrix) == 2, "optional-feature job must define a package matrix")
    package_lines = matrix[1].split("    env:\n", 1)[0]
    routed = set(re.findall(r"(?m)^\s+- ([a-z][a-z0-9-]*)\s*$", package_lines))
    expected = optional_feature_packages()
    require(
        routed | {"rrd-operator-knowledge"} == expected,
        f"optional-feature coverage mismatch: expected={sorted(expected)} routed={sorted(routed)}",
    )
    require(
        "cargo test -p ${{ matrix.package }} --all-targets --all-features --locked" in optional
        and "cargo clippy -p ${{ matrix.package }} --all-targets --all-features --locked -- -D warnings"
        in optional,
        "optional-feature packages must run locked all-feature tests and strict Clippy",
    )
    require(
        "RRFLOW_PGVECTOR_TEST_URL" in pgvector
        and "cargo test -p rrd-operator-knowledge --all-targets --all-features --locked"
        in pgvector,
        "PostgreSQL operator knowledge must retain its real pgvector qualification",
    )
    return len(expected)


def verify_sdk_conformance(block: str) -> None:
    required = [
        "uses: pnpm/action-setup@b906affcce14559ad1aafd4ab0e942779e9f58b1",
        "uses: actions/setup-node@249970729cb0ef3589644e2896645e5dc5ba9c38",
        "uses: actions/setup-python@ece7cb06caefa5fff74198d8649806c4678c61a1",
        "uses: astral-sh/setup-uv@37802adc94f370d6bfd71619e3f0bf239e1f3b78",
        "uses: actions/setup-go@924ae3a1cded613372ab5595356fb5720e22ba16",
        "uses: actions/setup-java@b6effb05e454b25005698d916606bdc6ffcbf961",
        "uses: actions/setup-dotnet@26b0ec14cb23fa6904739307f278c14f94c95bf1",
        "python3 scripts/ci/run_sdk_conformance.py",
        "node-version: 24.20.0",
        "version: 11.22.0",
        "python-version: 3.11.16",
        "version: 0.11.21",
        "go-version: 1.24.13",
        "cache: false",
        "java-version: 21.0.12.1+1",
        "dotnet-version: 10.0.111",
        "MAVEN_VERSION: 3.9.12",
        "MAVEN_SHA512: 0a1be79f02466533fc1a80abbef8796e4f737c46c6574ede5658b110899942a94db634477dfd3745501c80aef9aac0d4f841d38574373f7e2d24cce89d694f7084f",
        "sha512sum --check --strict",
    ]
    missing = [fragment for fragment in required if fragment not in block]
    require(not missing, f"SDK conformance toolchain or runner contract is incomplete: {missing}")
    require(
        block.count("python3 scripts/ci/run_sdk_conformance.py") == 1,
        "SDK qualification must execute through one cohesive orchestrator",
    )


def main() -> None:
    caller = CALLER.read_text()
    reusable = REUSABLE.read_text()
    jobs = job_blocks(reusable)

    for trigger in ["pull_request:", "merge_group:", "workflow_dispatch:"]:
        require(
            re.search(rf"(?m)^  {re.escape(trigger)}\s*$", caller) is not None,
            f"candidate CI must include {trigger}",
        )
    require(re.search(r"(?m)^\s*push:\s*$", caller) is None, "candidate CI must not duplicate PR work on push")
    require("pull_request_target" not in caller, "candidate CI must not execute proposed code with pull_request_target")
    require(
        caller.count("uses: ./.github/workflows/ci-reusable.yml") == 1,
        "candidate CI must invoke exactly one reusable workflow",
    )
    require("cancel-in-progress: true" in caller, "stale candidate CI must be cancelled")
    require(
        "github.event.pull_request.head.repo.full_name == github.repository" in caller
        and "vars.CI_SELF_HOSTED_ENABLED == 'true'" in caller,
        "self-hosted routing must preserve the fork trust boundary",
    )
    require(
        "permissions:\n  contents: read" in caller
        and "permissions:\n  contents: read" in reusable,
        "caller and reusable workflow must default to read-only contents",
    )

    expected_jobs = {
        "topology-smoke",
        "estate-portability",
        "verify",
        "engine-suites",
        "sdk-conformance",
        "optional-features",
        "pgvector-feature",
        "ci-gate",
    }
    require(set(jobs) == expected_jobs, f"candidate CI jobs must have cohesive ownership: {sorted(jobs)}")
    for job, block in jobs.items():
        require("timeout-minutes:" in block, f"CI job {job} must have an explicit timeout")

    gate = jobs["ci-gate"]
    needs_match = re.search(r"(?ms)^    needs:\s*$\n((?:^      - [a-z0-9-]+\s*$\n?)+)", gate)
    require(needs_match is not None, "ci-gate must enumerate every substantive job")
    needs = set(re.findall(r"(?m)^      - ([a-z0-9-]+)\s*$", needs_match.group(1)))
    require(needs == set(jobs) - {"ci-gate"}, "ci-gate must reduce every substantive job exactly once")
    result_dependencies = set(re.findall(r"needs\.([a-z0-9-]+)\.result", gate))
    require(result_dependencies == needs, "ci-gate must inspect every dependency result")
    result_variables = re.findall(
        r"(?m)^\s+([A-Z][A-Z0-9_]+):\s+\$\{\{\s*needs\.[a-z0-9-]+\.result\s*}}\s*$",
        gate,
    )
    require(
        all(f'test "${variable}" = success' in gate for variable in result_variables),
        "ci-gate must require success for every reduced result",
    )

    heavy_jobs = [
        "verify",
        "engine-suites",
        "sdk-conformance",
        "optional-features",
        "pgvector-feature",
    ]
    for job in heavy_jobs:
        block = jobs[job]
        require(
            'CARGO_BUILD_JOBS: "2"' in block
            and 'CARGO_PROFILE_TEST_DEBUG: "0"' in block
            and 'RUSTFLAGS: "-C link-arg=-Wl,--threads=1"' in block,
            f"Linux-heavy job {job} must retain the bounded compiler/linker profile",
        )
        require("cache-on-failure: false" in block, f"Linux-heavy job {job} must not publish failed artifacts")
    require(
        "cargo test --workspace" not in reusable and "cargo clippy --workspace" not in reusable,
        "hosted fallback must not link the entire workspace in one target graph",
    )
    require(
        "cargo fmt --all -- --check" in jobs["verify"]
        and "python3 scripts/ci/check_workflow.py" in jobs["verify"],
        "repository policy must check formatting and this CI contract",
    )
    require(
        "python3 scripts/ci/check_generated_surfaces.py" in jobs["verify"],
        "repository policy must reject generated product-surface drift",
    )

    suite_count, package_count = verify_engine_suites(jobs["engine-suites"])
    verify_sdk_conformance(jobs["sdk-conformance"])
    feature_count = verify_optional_features(jobs["optional-features"], jobs["pgvector-feature"])
    require(
        reusable.count("cargo build -p rrflow-cli --bin rrd-estate-controller --locked") == 2,
        "portability and service-protocol qualification must both declare the controller fixture",
    )

    verify_action_pins(caller, reusable)
    for image in re.findall(r"(?m)^\s+image:\s+([^\s#]+)", reusable):
        require(
            re.fullmatch(r"[^@]+@sha256:[0-9a-fA-F]{64}", image) is not None,
            f"CI service image must be digest pinned: {image}",
        )
    verify_arc_policy()
    print(
        f"CI policy OK: {len(jobs) - 1} substantive jobs, {suite_count} cohesive engine suites, "
        f"{package_count} default-feature packages, {feature_count} optional-feature packages"
    )


if __name__ == "__main__":
    try:
        main()
    except PolicyError as error:
        print(f"CI policy violation: {error}", file=sys.stderr)
        raise SystemExit(1) from error
