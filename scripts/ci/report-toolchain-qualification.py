#!/usr/bin/env python3
"""Print a metadata-only summary of toolchain qualification evidence."""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sys
from pathlib import Path
from typing import Any

SCHEMA = "axiom.toolchain_qualification.v0"
SHA_PATTERN = re.compile(r"[0-9a-f]{40}\Z")
MAX_ARTIFACT_LENGTH = 200


def load_validator(repo_root: Path):
    runner = repo_root / "scripts/ci/run-toolchain-qualification.py"
    spec = importlib.util.spec_from_file_location("toolchain_qualification", runner)
    if spec is None or spec.loader is None:
        raise RuntimeError("qualification validator is unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return (
        module.validate_qualification_evidence,
        repo_root / "stage1/schemas/axiom-toolchain-qualification-v0.schema.json",
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--expected-head-sha", required=True)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    return parser.parse_args()


def safe_path(path: Path) -> str:
    value = str(path)
    if any(ord(char) < 32 or ord(char) == 127 for char in value):
        raise ValueError("evidence path contains control characters")
    return value


def safe_artifacts(value: Any) -> str:
    if not isinstance(value, list) or not value:
        raise ValueError("artifacts must be a non-empty array")
    names: list[str] = []
    for artifact in value:
        if (
            not isinstance(artifact, str)
            or not artifact
            or len(artifact) > MAX_ARTIFACT_LENGTH
            or any(ord(char) < 32 or ord(char) == 127 for char in artifact)
        ):
            raise ValueError(
                "artifact names must be bounded strings without control characters"
            )
        names.append(artifact)
    return ",".join(names)


def print_harness_failure(evidence_path: str) -> None:
    print(
        "qualification summary: "
        "status=harness_failure failure_class=harness_failure "
        "error=malformed_or_unverifiable_evidence "
        f"evidence={evidence_path}"
    )


def main() -> int:
    options = parse_args()
    evidence_path = options.evidence.resolve()
    evidence_display = "<unavailable>"
    try:
        evidence_display = safe_path(evidence_path)
        if not SHA_PATTERN.fullmatch(options.expected_head_sha):
            raise ValueError("expected head SHA is not an exact lowercase commit")
        payload = json.loads(evidence_path.read_text(encoding="utf-8"))
        validate, schema_path = load_validator(options.repo_root.resolve())
        evidence = validate(payload, schema_path)
        if evidence["headSha"] != options.expected_head_sha:
            raise ValueError("evidence head SHA does not match expected head SHA")
        for check in evidence["checks"]:
            safe_artifacts(check["artifacts"])
    except (OSError, UnicodeError, json.JSONDecodeError, RuntimeError, ValueError):
        print_harness_failure(evidence_display)
        return 1

    counts = {
        status: sum(check["status"] == status for check in evidence["checks"])
        for status in ("passed", "skipped", "failed")
    }
    print(
        "qualification summary: "
        f"status={evidence['status']} failure_class={evidence['failureClass']} "
        f"passed={counts['passed']} skipped={counts['skipped']} failed={counts['failed']} "
        f"head_sha={evidence['headSha']} evidence={evidence_display}"
    )
    for check in evidence["checks"]:
        if check["status"] == "failed":
            print(
                "qualification failure: "
                f"id={check['id']} status={check['status']} "
                f"failure_class={check['failureClass']} exit_code={check['exitCode']} "
                f"artifacts={safe_artifacts(check['artifacts'])}"
            )
    return 0


if __name__ == "__main__":
    sys.exit(main())
