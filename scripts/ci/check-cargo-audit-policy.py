#!/usr/bin/env python3
"""Enforce the repository policy for cargo-audit advisories."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import sys
from pathlib import Path
from typing import Any


ADVISORY_ID = re.compile(r"^RUSTSEC-\d{4}-\d{4}$")
ISSUE_URL = re.compile(r"^https://github\.com/OMT-Global/axiomlang/issues/[1-9]\d*$")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--report", required=True, type=Path)
    result.add_argument("--policy", required=True, type=Path)
    result.add_argument(
        "--today",
        type=dt.date.fromisoformat,
        default=dt.datetime.now(dt.timezone.utc).date(),
        help="override the UTC date for deterministic tests",
    )
    return result


def advisory_id(finding: dict[str, Any]) -> str | None:
    advisory = finding.get("advisory")
    if isinstance(advisory, dict) and isinstance(advisory.get("id"), str):
        return advisory["id"]
    if isinstance(finding.get("id"), str):
        return finding["id"]
    return None


def findings(report: dict[str, Any]) -> list[tuple[str, str]]:
    result: list[tuple[str, str]] = []
    vulnerabilities = report.get("vulnerabilities", {})
    if isinstance(vulnerabilities, dict):
        for finding in vulnerabilities.get("list", []):
            if isinstance(finding, dict):
                identifier = advisory_id(finding)
                if identifier:
                    result.append((identifier, "vulnerability"))

    warnings = report.get("warnings", {})
    if isinstance(warnings, dict):
        for kind, entries in warnings.items():
            if not isinstance(entries, list):
                continue
            for finding in entries:
                if isinstance(finding, dict):
                    identifier = advisory_id(finding)
                    if identifier:
                        result.append((identifier, f"warning:{kind}"))
    return result


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"unable to read JSON from {path}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def main() -> int:
    args = parser().parse_args()
    errors: list[str] = []
    try:
        report = load_json(args.report)
        policy = load_json(args.policy)
    except ValueError as error:
        print(json.dumps({"status": "fail", "errors": [str(error)]}, indent=2))
        return 1

    if policy.get("version") != 1:
        errors.append("policy version must be 1")

    raw_exceptions = policy.get("exceptions", [])
    if not isinstance(raw_exceptions, list):
        errors.append("policy exceptions must be an array")
        raw_exceptions = []

    exception_by_id: dict[str, dict[str, Any]] = {}
    for index, exception in enumerate(raw_exceptions):
        if not isinstance(exception, dict):
            errors.append(f"exception {index} must be an object")
            continue
        identifier = exception.get("advisory")
        if not isinstance(identifier, str) or not ADVISORY_ID.fullmatch(identifier):
            errors.append(f"exception {index} must use a RUSTSEC-YYYY-NNNN advisory id")
            continue
        if identifier in exception_by_id:
            errors.append(f"duplicate exception for {identifier}")
        exception_by_id[identifier] = exception

        issue = exception.get("issue")
        if not isinstance(issue, str) or not ISSUE_URL.fullmatch(issue):
            errors.append(f"exception {identifier} must link to an Axiomlang issue")

        reason = exception.get("reason")
        if not isinstance(reason, str) or len(reason.strip()) < 10:
            errors.append(f"exception {identifier} must explain the temporary risk decision")

        expires_at = exception.get("expires_at")
        try:
            expiry = dt.date.fromisoformat(expires_at) if isinstance(expires_at, str) else None
        except ValueError:
            expiry = None
        if expiry is None:
            errors.append(f"exception {identifier} must use an ISO-8601 expires_at date")
        elif expiry <= args.today:
            errors.append(f"exception {identifier} expired on {expiry.isoformat()}")

    active_findings = findings(report)
    active_ids = {identifier for identifier, _ in active_findings}
    for identifier, kind in active_findings:
        if identifier not in exception_by_id:
            errors.append(f"active {kind} advisory {identifier} has no approved exception")

    for identifier in exception_by_id:
        if identifier not in active_ids:
            errors.append(f"exception {identifier} does not match an active cargo-audit finding")

    result = {
        "status": "fail" if errors else "pass",
        "active_advisories": [
            {"advisory": identifier, "kind": kind} for identifier, kind in active_findings
        ],
        "exceptions": sorted(exception_by_id),
        "errors": errors,
    }
    print(json.dumps(result, indent=2, sort_keys=True))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
