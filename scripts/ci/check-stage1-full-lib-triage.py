#!/usr/bin/env python3
"""Validate the Rust-exit full axiomc lib-suite triage manifest."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_MANIFEST = REPO_ROOT / "docs/rust-exit-full-lib-triage.json"
REQUIRED_COMMAND_PARTS = [
    "RUST_MIN_STACK=8388608",
    "cargo test",
    "--manifest-path stage1/Cargo.toml",
    "-p axiomc",
    "--lib",
    "--features run-native-tests",
]
ALLOWED_CATEGORIES = {
    "direct_native_contract",
    "environment_gated",
    "stale_generated_rust_expectation",
    "unsupported_construct",
}
ALLOWED_RESOLUTIONS = {
    "environment_gate",
    "ignore_with_linked_blocker",
    "pin_generated_rust_backend",
    "update_direct_native_contract",
}


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise SystemExit(f"{path} must contain a JSON object")
    return payload


def validate(payload: dict[str, Any]) -> tuple[list[str], dict[str, Any]]:
    errors: list[str] = []
    failures = payload.get("failures")
    if not isinstance(failures, list):
        failures = []
        errors.append("failures must be a list")

    if payload.get("schemaVersion") != 1:
        errors.append("schemaVersion must be 1")
    if payload.get("issue") != 1255:
        errors.append("issue must be 1255")
    if payload.get("status") not in {"blocked", "ready"}:
        errors.append("status must be blocked or ready")
    if payload.get("generatedRustCliStatus") != "removed":
        errors.append("generatedRustCliStatus must record removed")

    command = payload.get("command")
    if not isinstance(command, str) or not command:
        errors.append("command must be a non-empty string")
    else:
        for required in REQUIRED_COMMAND_PARTS:
            if required not in command:
                errors.append(f"command must include {required!r}")

    expected = payload.get("expectedFailureCount")
    if expected != len(failures):
        errors.append(
            f"expectedFailureCount must equal failures length ({expected!r} != {len(failures)})"
        )

    names: list[str] = []
    category_counts: Counter[str] = Counter()
    resolution_counts: Counter[str] = Counter()
    for index, failure in enumerate(failures):
        if not isinstance(failure, dict):
            errors.append(f"failures[{index}] must be an object")
            continue

        name = failure.get("name")
        category = failure.get("category")
        resolution = failure.get("resolution")
        blocker = failure.get("blockerIssue")
        rationale = failure.get("rationale")

        if not isinstance(name, str) or not name:
            errors.append(f"failures[{index}].name must be a non-empty string")
        else:
            names.append(name)
        if category not in ALLOWED_CATEGORIES:
            errors.append(f"{name or index}: unknown category {category!r}")
        else:
            category_counts[category] += 1
        if resolution not in ALLOWED_RESOLUTIONS:
            errors.append(f"{name or index}: unknown resolution {resolution!r}")
        else:
            resolution_counts[resolution] += 1
        if category == "environment_gated" and resolution != "environment_gate":
            errors.append(f"{name or index}: environment_gated rows must use environment_gate")
        if category != "environment_gated" and resolution == "environment_gate":
            errors.append(f"{name or index}: only environment_gated rows may use environment_gate")
        if resolution == "ignore_with_linked_blocker" and not isinstance(blocker, int):
            errors.append(f"{name or index}: ignored rows must name a numeric blockerIssue")
        if not isinstance(blocker, int):
            errors.append(f"{name or index}: blockerIssue must be a number")
        if not isinstance(rationale, str) or len(rationale.strip()) < 20:
            errors.append(f"{name or index}: rationale must explain the triage")

    duplicates = sorted(name for name, count in Counter(names).items() if count > 1)
    if duplicates:
        errors.append("duplicate failure rows: " + ", ".join(duplicates))
    if names != sorted(names):
        errors.append("failure rows must be sorted by name")

    # The initial 40-row triage was required to identify every category
    # (stale generated-Rust expectations, direct-native contract repairs, and
    # environment-gated cases). As repair PRs land and rows are removed, the
    # remaining mix legitimately narrows, so category presence is no longer
    # enforced -- rows must still each carry a valid category and resolution.

    summary = {
        "failure_count": len(failures),
        "categories": dict(sorted(category_counts.items())),
        "resolutions": dict(sorted(resolution_counts.items())),
    }
    return errors, summary


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    payload = load_json(args.manifest)
    errors, summary = validate(payload)
    report = {
        "schema": "axiom.stage1.full_lib_triage.v1",
        "ready": not errors and payload.get("status") == "ready",
        "triaged": not errors,
        "issue": payload.get("issue"),
        "status": payload.get("status"),
        "summary": summary,
        "errors": errors,
    }
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    elif errors:
        for error in errors:
            print(error)

    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
