#!/usr/bin/env python3
"""Report large Rust-hosted compiler source files for self-hosting work."""

from __future__ import annotations

import argparse
import json
import os
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_SOURCE_ROOT = REPO_ROOT / "stage1/crates/axiomc/src"
DEFAULT_PLAN = REPO_ROOT / "docs/compiler-source-decomposition-plan.md"
REPORT_VERSION = "axiom.compiler_source.monoliths.v0"

BOUNDARY_MAP: dict[str, list[str]] = {
    "borrowck.rs": ["compiler.hir"],
    "codegen.rs": ["compiler.backend.generated_rust", "compiler.backend.contracts"],
    "cranelift_backend.rs": ["compiler.backend.native"],
    "dap.rs": ["compiler.services.lsp"],
    "diagnostic_catalog.rs": ["compiler.diagnostics"],
    "diagnostics.rs": ["compiler.diagnostics"],
    "hir.rs": ["compiler.hir"],
    "json_contract.rs": ["compiler.commands"],
    "lib.rs": ["compiler package facade"],
    "lockfile.rs": ["compiler.package_graph"],
    "lsp.rs": ["compiler.services.lsp"],
    "main.rs": ["compiler.commands"],
    "manifest.rs": ["compiler.package_graph"],
    "mir.rs": ["compiler.mir"],
    "new_project.rs": ["compiler.commands"],
    "project.rs": ["compiler.package_graph", "compiler.commands", "compiler.evidence"],
    "registry.rs": ["compiler.package_graph"],
    "stdlib.rs": ["compiler.stdlib"],
    "syntax.rs": ["compiler.syntax", "compiler.diagnostics"],
}


@dataclass(frozen=True)
class SourceFile:
    path: Path
    lines: int


def count_lines(path: Path) -> int:
    with path.open("rb") as handle:
        return sum(1 for _ in handle)


def collect_source_files(source_root: Path) -> list[SourceFile]:
    if not source_root.is_dir():
        raise SystemExit(f"source root does not exist: {source_root}")

    files = [
        SourceFile(path=path, lines=count_lines(path))
        for path in sorted(source_root.rglob("*.rs"))
        if "target" not in path.parts and "dist" not in path.parts
    ]
    return sorted(files, key=lambda item: (-item.lines, item.path.as_posix()))


def repo_relative(path: Path) -> str:
    try:
        return path.relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def boundaries_for(path: Path) -> list[str]:
    return BOUNDARY_MAP.get(path.name, ["unmapped"])


def collected_at() -> str:
    override = os.environ.get("AXIOM_COMPILER_SOURCE_COLLECTED_AT")
    if override:
        return override
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def build_report(source_root: Path, top: int) -> dict[str, Any]:
    files = collect_source_files(source_root)
    total_lines = sum(item.lines for item in files)
    top_files = files[:top]
    top_lines = sum(item.lines for item in top_files)
    largest = top_files[0] if top_files else None

    return {
        "schema_version": REPORT_VERSION,
        "collected_at": collected_at(),
        "source_root": repo_relative(source_root),
        "summary": {
            "total_files": len(files),
            "total_lines": total_lines,
            "largest_file": repo_relative(largest.path) if largest else None,
            "largest_file_lines": largest.lines if largest else 0,
            "top_file_count": top,
            "top_file_lines": top_lines,
            "top_file_line_share": round(top_lines / total_lines, 4) if total_lines else 0,
        },
        "top_files": [
            {
                "path": repo_relative(item.path),
                "lines": item.lines,
                "package_boundaries": boundaries_for(item.path),
            }
            for item in top_files
        ],
    }


def check_plan(report: dict[str, Any], plan_path: Path) -> list[str]:
    if not plan_path.is_file():
        return [f"missing decomposition plan: {repo_relative(plan_path)}"]

    body = plan_path.read_text(encoding="utf-8")
    errors: list[str] = []
    for item in report["top_files"]:
        path = item["path"]
        filename = Path(path).name
        if path not in body and filename not in body:
            errors.append(f"plan does not mention top file {path}")
        for boundary in item["package_boundaries"]:
            if boundary != "unmapped" and boundary not in body:
                errors.append(f"plan does not map {path} to {boundary}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Report Rust-hosted compiler source monoliths."
    )
    parser.add_argument("--source-root", type=Path, default=DEFAULT_SOURCE_ROOT)
    parser.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    parser.add_argument("--top", type=int, default=7)
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    parser.add_argument(
        "--check-plan",
        action="store_true",
        help="fail if the decomposition plan does not cover the top files",
    )
    args = parser.parse_args()

    if args.top <= 0:
        raise SystemExit("--top must be positive")

    report = build_report(args.source_root, args.top)
    errors = check_plan(report, args.plan) if args.check_plan else []
    report["plan_check"] = {
        "plan": repo_relative(args.plan),
        "passed": not errors,
        "errors": errors,
    }

    print(json.dumps(report, indent=2, sort_keys=True))
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
