#!/usr/bin/env python3
"""Build a deterministic, reviewable project-shape manifest.

The manifest is deliberately derived from the filesystem rather than from a
model.  It gives a model one bounded, stable description of the repository to
reason over; model-authored context and plans can then cite this evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path


DEFAULT_EXCLUDES = {".git", ".vyrm", "node_modules", "target"}
ROLE_FILES = {
    "AGENTS.md": "agent_instructions",
    "Cargo.toml": "rust_manifest",
    "package.json": "javascript_manifest",
    "pyproject.toml": "python_manifest",
    "README.md": "documentation_root",
}


def build_shape(root: Path, excludes: set[str] | None = None) -> dict[str, object]:
    root = root.resolve()
    excluded = DEFAULT_EXCLUDES | (excludes or set())
    files: list[dict[str, object]] = []
    directories: set[str] = set()

    for current, child_dirs, child_files in os.walk(root):
        child_dirs[:] = sorted(name for name in child_dirs if name not in excluded)
        current_path = Path(current)
        for name in sorted(child_files):
            path = current_path / name
            if not path.is_file():
                continue
            relative = path.relative_to(root)
            data = path.read_bytes()
            parent = relative.parent.as_posix()
            if parent != ".":
                directories.add(parent)
            files.append(
                {
                    "path": relative.as_posix(),
                    "bytes": len(data),
                    "sha256": hashlib.sha256(data).hexdigest(),
                    "role": ROLE_FILES.get(relative.name, "source"),
                }
            )

    files.sort(key=lambda entry: str(entry["path"]))

    identity_input = json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    return {
        "format": "vyrm.project-shape/v1",
        "root": root.name,
        "tree_sha256": hashlib.sha256(identity_input).hexdigest(),
        "directories": sorted(directories),
        "files": files,
    }


def render_markdown(shape: dict[str, object]) -> str:
    lines = [
        "# Project shape",
        "",
        f"- Format: `{shape['format']}`",
        f"- Root: `{shape['root']}`",
        f"- Tree digest: `{shape['tree_sha256']}`",
        "",
        "## Files",
        "",
        "| Path | Role | Bytes | SHA-256 |",
        "|---|---|---:|---|",
    ]
    for file in shape["files"]:  # type: ignore[index]
        lines.append(
            f"| `{file['path']}` | {file['role']} | {file['bytes']} | `{file['sha256']}` |"
        )
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", default=".", type=Path)
    parser.add_argument("--exclude", action="append", default=[])
    parser.add_argument("--format", choices=("json", "markdown"), default="json")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    shape = build_shape(args.root, set(args.exclude))
    output = (
        json.dumps(shape, indent=2, sort_keys=True) + "\n"
        if args.format == "json"
        else render_markdown(shape)
    )
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(output, encoding="utf-8")
    else:
        print(output, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
