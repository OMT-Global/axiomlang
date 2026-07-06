#!/usr/bin/env python3
"""Report large Rust-hosted compiler source files for self-hosting work."""

from __future__ import annotations

import argparse
import json
import os
import re
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
SOURCE_ROOT_SUFFIX = "stage1/crates/axiomc/src"
PLAN_SUFFIX = "docs/compiler-source-decomposition-plan.md"
REPORT_VERSION = "axiom.compiler_source.monoliths.v0"
RATCHET_ROW = re.compile(r"^\|\s*`([^`]+)`\s*\|\s*([0-9]+(?:\.[0-9]+)?)\s*\|")

BOUNDARY_MAP: dict[str, list[str]] = {
    "borrowck.rs": ["compiler.hir"],
    "capabilities.rs": ["compiler.hir"],
    "codegen.rs": ["compiler.backend.generated_rust", "compiler.backend.contracts"],
    "cranelift_backend.rs": ["compiler.backend.native"],
    "dap.rs": ["compiler.services.lsp"],
    "diagnostic_catalog.rs": ["compiler.diagnostics"],
    "definitions.rs": ["compiler.hir"],
    "diagnostics.rs": ["compiler.diagnostics"],
    "expressions.rs": ["compiler.hir"],
    "generics.rs": ["compiler.hir"],
    "hir.rs": ["compiler.hir"],
    "json_contract.rs": ["compiler.commands"],
    "lib.rs": ["compiler package facade"],
    "lockfile.rs": ["compiler.package_graph"],
    "lsp.rs": ["compiler.services.lsp"],
    "main.rs": ["compiler.commands"],
    "manifest.rs": ["compiler.package_graph"],
    "mir.rs": ["compiler.mir"],
    "model.rs": ["compiler.hir"],
    "new_project.rs": ["compiler.commands"],
    "ownership.rs": ["compiler.hir"],
    "properties.rs": ["compiler.hir"],
    "reachability.rs": ["compiler.hir"],
    "project.rs": ["compiler.package_graph", "compiler.commands", "compiler.evidence"],
    "registry.rs": ["compiler.package_graph"],
    "signatures.rs": ["compiler.hir"],
    "stdlib.rs": ["compiler.stdlib"],
    "symbols.rs": ["compiler.hir"],
    "syntax.rs": ["compiler.syntax", "compiler.diagnostics"],
    "types.rs": ["compiler.hir"],
}

PATH_BOUNDARY_MAP: dict[str, list[str]] = {
    "stage1/crates/axiomc/src/hir/diagnostics.rs": ["compiler.hir"],
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


def checkout_relative(path: Path, checkout_root: Path) -> str:
    """Relativize against the data checkout so plan ceilings keyed by
    repo-relative paths match even when the script runs from a separate
    trusted checkout (the PR Fast CI shape)."""
    try:
        return path.resolve().relative_to(checkout_root.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def boundaries_for(path: Path, checkout_root: Path = REPO_ROOT) -> list[str]:
    relative = checkout_relative(path, checkout_root)
    if relative in PATH_BOUNDARY_MAP:
        return PATH_BOUNDARY_MAP[relative]
    if path.name == "diagnostics.rs" and path.parent.name == "hir":
        return ["compiler.hir"]
    return BOUNDARY_MAP.get(path.name, ["unmapped"])


def collected_at() -> str:
    override = os.environ.get("AXIOM_COMPILER_SOURCE_COLLECTED_AT")
    if override:
        return override
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def build_report(source_root: Path, top: int, checkout_root: Path = REPO_ROOT) -> dict[str, Any]:
    files = collect_source_files(source_root)
    total_lines = sum(item.lines for item in files)
    top_files = files[:top]
    top_lines = sum(item.lines for item in top_files)
    largest = top_files[0] if top_files else None

    return {
        "schema_version": REPORT_VERSION,
        "collected_at": collected_at(),
        "source_root": checkout_relative(source_root, checkout_root),
        "summary": {
            "total_files": len(files),
            "total_lines": total_lines,
            "largest_file": checkout_relative(largest.path, checkout_root) if largest else None,
            "largest_file_lines": largest.lines if largest else 0,
            "top_file_count": top,
            "top_file_lines": top_lines,
            "top_file_line_share": top_lines / total_lines if total_lines else 0,
        },
        "top_files": [
            {
                "path": checkout_relative(item.path, checkout_root),
                "lines": item.lines,
                "package_boundaries": boundaries_for(item.path, checkout_root),
            }
            for item in top_files
        ],
    }


def check_plan(report: dict[str, Any], plan_path: Path, checkout_root: Path = REPO_ROOT) -> list[str]:
    if not plan_path.is_file():
        return [f"missing decomposition plan: {checkout_relative(plan_path, checkout_root)}"]

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


def parse_ratchet_ceilings(
    plan_path: Path, checkout_root: Path = REPO_ROOT
) -> tuple[dict[str, float], dict[str, int], list[str]]:
    if not plan_path.is_file():
        return {}, {}, [f"missing decomposition plan: {checkout_relative(plan_path, checkout_root)}"]

    metrics: dict[str, float] = {}
    files: dict[str, int] = {}
    errors: list[str] = []
    in_section = False

    for line in plan_path.read_text(encoding="utf-8").splitlines():
        if line.startswith("## "):
            in_section = line.strip() == "## Ratchet Ceilings"
            continue
        if not in_section:
            continue

        match = RATCHET_ROW.match(line)
        if not match:
            continue

        key, raw_value = match.groups()
        if key.startswith("summary."):
            metrics[key] = float(raw_value)
            continue

        try:
            files[key] = int(raw_value)
        except ValueError:
            errors.append(f"ratchet ceiling for {key} must be an integer line count")

    if not metrics and not files:
        errors.append("plan does not define a ## Ratchet Ceilings table")
    return metrics, files, errors


def current_lines_for(path: str, top_files: dict[str, int], checkout_root: Path = REPO_ROOT) -> int:
    if path in top_files:
        return top_files[path]

    candidate = checkout_root / path
    if candidate.is_file():
        return count_lines(candidate)
    return 0


def check_ratchet(report: dict[str, Any], plan_path: Path, checkout_root: Path = REPO_ROOT) -> list[str]:
    metrics, file_ceilings, errors = parse_ratchet_ceilings(plan_path, checkout_root)
    if errors:
        return errors

    top_files = {item["path"]: item["lines"] for item in report["top_files"]}
    for item in report["top_files"]:
        path = item["path"]
        if path not in file_ceilings:
            errors.append(f"ratchet is missing a line ceiling for top file {path}")

    share_ceiling = metrics.get("summary.top_file_line_share")
    if share_ceiling is None:
        errors.append("ratchet is missing summary.top_file_line_share ceiling")
    else:
        current_share = float(report["summary"]["top_file_line_share"])
        if current_share > share_ceiling:
            errors.append(
                "top file line share "
                f"{current_share:.4f} exceeds ratchet ceiling {share_ceiling:.4f}"
            )

    lines_ceiling = metrics.get("summary.top_file_lines")
    if lines_ceiling is None:
        errors.append("ratchet is missing summary.top_file_lines ceiling")
    else:
        current_lines = int(report["summary"]["top_file_lines"])
        if current_lines > int(lines_ceiling):
            errors.append(
                f"top file lines {current_lines} exceeds ratchet ceiling {int(lines_ceiling)}"
            )

    for path, ceiling in sorted(file_ceilings.items()):
        current = current_lines_for(path, top_files, checkout_root)
        if current > ceiling:
            errors.append(f"{path} has {current} lines, above ratchet ceiling {ceiling}")

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Report Rust-hosted compiler source monoliths."
    )
    parser.add_argument(
        "--checkout-root",
        type=Path,
        default=REPO_ROOT,
        help="root of the data checkout that source paths and plan ceilings are relative to",
    )
    parser.add_argument("--source-root", type=Path, default=None)
    parser.add_argument("--plan", type=Path, default=None)
    parser.add_argument("--top", type=int, default=7)
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    parser.add_argument(
        "--check-plan",
        action="store_true",
        help="fail if the decomposition plan does not cover the top files",
    )
    parser.add_argument(
        "--check-ratchet",
        action="store_true",
        help="fail if tracked monolith line counts or top-file share exceed the plan ceilings",
    )
    args = parser.parse_args()

    if args.top <= 0:
        raise SystemExit("--top must be positive")

    checkout_root = args.checkout_root
    source_root = args.source_root or checkout_root / SOURCE_ROOT_SUFFIX
    plan = args.plan or checkout_root / PLAN_SUFFIX

    report = build_report(source_root, args.top, checkout_root)
    plan_errors = check_plan(report, plan, checkout_root) if args.check_plan else []
    ratchet_errors = check_ratchet(report, plan, checkout_root) if args.check_ratchet else []
    report["plan_check"] = {
        "plan": checkout_relative(plan, checkout_root),
        "passed": not plan_errors,
        "errors": plan_errors,
    }
    report["ratchet_check"] = {
        "plan": checkout_relative(plan, checkout_root),
        "passed": not ratchet_errors,
        "errors": ratchet_errors,
    }

    print(json.dumps(report, indent=2, sort_keys=True))
    return 1 if plan_errors or ratchet_errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
