#!/usr/bin/env python3
"""Report Rust-exit coverage for official command and LSP surfaces."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_COMMAND_LSP_SNAPSHOT = (
    REPO_ROOT / "stage1/compiler-contracts/snapshots/command-lsp.json"
)
DEFAULT_READINESS_MANIFEST = REPO_ROOT / "docs/rust-exit-readiness.json"
SCHEMA = "axiom.rust_exit.command_surface_coverage.v0"
REQUIRED_COMMAND_SURFACES = ("check", "build", "run", "test", "doc")
REQUIRED_LSP_FLOWS = (
    "serve_stdio",
    "initialize",
    "open_document",
    "change_document",
    "publish_diagnostics",
    "shutdown",
    "exit",
)
# Closed doc/LSP ownership proof. The issue stays listed in
# docs/rust-exit-readiness.json so `make rust-exit-readiness` keeps validating
# its CLOSED state live; this offline report records it as the governing proof
# rather than a live blocker.
DOC_LSP_PROOF_ISSUE = 731


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError(f"{path} root must be an object")
    return payload


def readiness_blockers(manifest: dict[str, Any]) -> set[int]:
    blockers: set[int] = set()
    for item in manifest.get("blockingIssues", []):
        if isinstance(item, dict) and isinstance(item.get("issue"), int):
            blockers.add(item["issue"])
    return blockers


def fixture_paths_exist(fixtures: object, errors: list[str], surface: str) -> list[str]:
    if not isinstance(fixtures, list):
        errors.append(f"{surface} fixtures must be an array")
        return []
    existing: list[str] = []
    for index, fixture in enumerate(fixtures):
        if not isinstance(fixture, str) or not fixture:
            errors.append(f"{surface} fixture[{index}] must be a non-empty string")
            continue
        path = Path(fixture)
        if path.is_absolute() or ".." in path.parts:
            errors.append(f"{surface} fixture[{index}] must be repository-relative")
            continue
        if not (REPO_ROOT / path).exists():
            errors.append(f"{surface} fixture does not exist: {fixture}")
            continue
        existing.append(fixture)
    return existing


def command_row(
    command: dict[str, Any],
    blockers: set[int],
    errors: list[str],
) -> dict[str, Any]:
    name = command.get("name")
    if not isinstance(name, str):
        name = "<unknown>"
    fixtures = fixture_paths_exist(command.get("fixtures"), errors, f"command {name}")
    status = "implemented"
    notes = "Command has a package-boundary contract and checked command fixtures."
    proof_issues: list[int] = []
    if name == "doc":
        proof_issues = [DOC_LSP_PROOF_ISSUE]
        notes = (
            "Documentation command contract is present; doc/LSP ownership "
            f"proof #{DOC_LSP_PROOF_ISSUE} is closed and its state is "
            "validated live by make rust-exit-readiness."
        )
        if DOC_LSP_PROOF_ISSUE not in blockers:
            errors.append(
                f"command doc ownership proof #{DOC_LSP_PROOF_ISSUE} is missing from "
                "docs/rust-exit-readiness.json"
            )

    return {
        "surface": name,
        "kind": "command",
        "status": status,
        "blockers": [],
        "proof_issues": proof_issues,
        "api": command.get("api"),
        "stable_output": command.get("stable_output"),
        "fixtures": fixtures,
        "validation_command": "make stage1-command-lsp-boundary",
        "notes": notes,
    }


def lsp_row(
    services: list[dict[str, Any]],
    blockers: set[int],
    errors: list[str],
) -> dict[str, Any]:
    service_map = {
        service.get("flow"): service
        for service in services
        if isinstance(service, dict) and isinstance(service.get("flow"), str)
    }
    missing = sorted(set(REQUIRED_LSP_FLOWS) - set(service_map))
    if missing:
        errors.append("lsp service coverage missing flows: " + ", ".join(missing))
    if DOC_LSP_PROOF_ISSUE not in blockers:
        errors.append(
            f"lsp ownership proof #{DOC_LSP_PROOF_ISSUE} is missing from "
            "docs/rust-exit-readiness.json"
        )

    return {
        "surface": "lsp",
        "kind": "service",
        "status": "implemented",
        "blockers": [],
        "proof_issues": [DOC_LSP_PROOF_ISSUE],
        "api": "compiler.services.lsp.serve_stdio",
        "stable_output": "Content-Length framed JSON-RPC 2.0",
        "flows": list(REQUIRED_LSP_FLOWS),
        "validation_command": "make stage1-command-lsp-boundary",
        "notes": (
            "LSP protocol contract, stdio harness, and compiler-service "
            f"driver ownership are in place; ownership proof #{DOC_LSP_PROOF_ISSUE} "
            "is closed and its state is validated live by make rust-exit-readiness."
        ),
    }


def build_report(
    snapshot: dict[str, Any],
    manifest: dict[str, Any],
) -> tuple[dict[str, Any], int]:
    errors: list[str] = []
    blockers = readiness_blockers(manifest)

    commands_value = snapshot.get("commands")
    if not isinstance(commands_value, list):
        errors.append("command/LSP snapshot commands must be an array")
        commands: list[dict[str, Any]] = []
    else:
        commands = [item for item in commands_value if isinstance(item, dict)]

    command_map = {
        command.get("name"): command
        for command in commands
        if isinstance(command.get("name"), str)
    }
    missing_commands = sorted(set(REQUIRED_COMMAND_SURFACES) - set(command_map))
    if missing_commands:
        errors.append("command surface coverage missing commands: " + ", ".join(missing_commands))

    release = snapshot.get("official_release")
    if not isinstance(release, dict):
        errors.append("command/LSP snapshot official_release must be an object")
        release = {}
    if release.get("requires_cargo") is not False:
        errors.append("official release command behavior must not require Cargo")
    if release.get("requires_rustc") is not False:
        errors.append("official release command behavior must not require rustc")

    rows = [
        command_row(command_map[name], blockers, errors)
        for name in REQUIRED_COMMAND_SURFACES
        if name in command_map
    ]

    services_value = snapshot.get("lsp_services")
    if not isinstance(services_value, list):
        errors.append("command/LSP snapshot lsp_services must be an array")
        services: list[dict[str, Any]] = []
    else:
        services = [item for item in services_value if isinstance(item, dict)]
    rows.append(lsp_row(services, blockers, errors))

    status_counts: dict[str, int] = {}
    for row in rows:
        status = row["status"]
        status_counts[status] = status_counts.get(status, 0) + 1

    blocked_surfaces = [row["surface"] for row in rows if row["status"] != "implemented"]
    report = {
        "schema": SCHEMA,
        "ready": not blocked_surfaces and not errors,
        "target": "rust-exit command surface",
        "command_lsp_contract": snapshot.get("contract"),
        "readiness_manifest": str(DEFAULT_READINESS_MANIFEST.relative_to(REPO_ROOT)),
        "summary": {
            "surface_count": len(rows),
            "implemented": status_counts.get("implemented", 0),
            "blocked": status_counts.get("blocked", 0),
            "partial": status_counts.get("partial", 0),
            "blocked_surfaces": blocked_surfaces,
        },
        "rows": rows,
        "errors": errors,
    }
    return report, 1 if errors else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--command-lsp-snapshot", type=Path, default=DEFAULT_COMMAND_LSP_SNAPSHOT)
    parser.add_argument("--readiness-manifest", type=Path, default=DEFAULT_READINESS_MANIFEST)
    parser.add_argument("--json", action="store_true", help="emit JSON")
    parser.add_argument(
        "--enforce-ready",
        action="store_true",
        help="fail while any command surface remains blocked",
    )
    args = parser.parse_args()

    try:
        snapshot = load_json(args.command_lsp_snapshot)
        manifest = load_json(args.readiness_manifest)
        report, status = build_report(snapshot, manifest)
    except Exception as error:
        report = {
            "schema": SCHEMA,
            "ready": False,
            "target": "rust-exit command surface",
            "summary": {
                "surface_count": 0,
                "implemented": 0,
                "blocked": 0,
                "partial": 0,
                "blocked_surfaces": [],
            },
            "rows": [],
            "errors": [str(error)],
        }
        status = 1

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    elif report["ready"]:
        print("rust-exit command surface coverage: ready")
    else:
        blocked = ", ".join(report["summary"]["blocked_surfaces"]) or "none"
        print(f"rust-exit command surface coverage: blocked ({blocked})")

    if args.enforce_ready and not report["ready"]:
        return 1
    return status


if __name__ == "__main__":
    raise SystemExit(main())
