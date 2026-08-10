#!/usr/bin/env python3
"""Validate a readiness-gate artifact directory against an exact checkout head."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

SCHEMA = "axiom.readiness.gates.v1"
REPORTS = {
    "rust-exit-readiness": "axiom.rust_exit.readiness.v1",
    "self-hosting-language-readiness": "axiom.self_hosting.language_readiness.v0",
    "snapshot-bootstrap-readiness": "axiom.self_hosting.snapshot_bootstrap_readiness.v0",
}
TESTS = ("native-build-purity", "self-hosting-spike-parity")


def validate(directory: Path, head_sha: str) -> list[str]:
    errors: list[str] = []
    if re.fullmatch(r"[0-9a-f]{40}", head_sha) is None:
        return ["expected head SHA is not exact lowercase 40-character Git SHA"]
    aggregate_path = directory / "readiness-gates.json"
    try:
        aggregate = json.loads(aggregate_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return [f"aggregate evidence is unreadable: {error}"]
    if not isinstance(aggregate, dict):
        return ["aggregate evidence root must be an object"]
    if aggregate.get("schema") != SCHEMA:
        errors.append("aggregate schema is invalid")
    if aggregate.get("headSha") != head_sha:
        errors.append("aggregate evidence is stale or bound to another head")
    if aggregate.get("executed") is not True:
        errors.append("aggregate evidence does not prove execution")
    if aggregate.get("evidenceValid") is not True:
        errors.append("aggregate evidence is marked invalid")
    for name, schema in REPORTS.items():
        path = directory / f"{name}.json"
        try:
            report = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            errors.append(f"{name}: unreadable report: {error}")
            continue
        if not isinstance(report, dict) or report.get("schema") != schema:
            errors.append(f"{name}: malformed report schema")
            continue
        if report.get("headSha") != head_sha:
            errors.append(f"{name}: stale head SHA")
        if report.get("executed") is not True:
            errors.append(f"{name}: missing execution proof")
        if not isinstance(report.get("checks"), list) or not report["checks"]:
            errors.append(f"{name}: missing checks")
    tests = aggregate.get("tests")
    if not isinstance(tests, dict):
        errors.append("aggregate tests are missing")
    else:
        for name in TESTS:
            test = tests.get(name)
            if not isinstance(test, dict):
                errors.append(f"{name}: test evidence is missing")
                continue
            if test.get("headSha") != head_sha:
                errors.append(f"{name}: stale head SHA")
            if test.get("executed") is not True:
                errors.append(f"{name}: missing execution proof")
            if not isinstance(test.get("testsRun"), int) or test["testsRun"] < 1:
                errors.append(f"{name}: zero-test evidence")
            if not isinstance(test.get("exitCode"), int):
                errors.append(f"{name}: executable validation exit code is missing")
            if test.get("status") not in {"passed", "failed"}:
                errors.append(f"{name}: executable validation status is malformed")
            if not (directory / f"{name}.log").is_file():
                errors.append(f"{name}: execution log is missing")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact-dir", type=Path, required=True)
    parser.add_argument("--head-sha", required=True)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    errors = validate(args.artifact_dir, args.head_sha)
    report = {"schema": SCHEMA, "ready": not errors, "errors": errors}
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    elif errors:
        for error in errors:
            print(error, file=sys.stderr)
    else:
        print("readiness gate evidence is valid")
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
