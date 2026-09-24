#!/usr/bin/env python3
"""Execute readiness gates and publish exact-head evidence.

The readiness reports intentionally remain non-blocking when they are honest
``ready: false`` reports. This runner fails only when a checker did not run,
emitted malformed evidence, produced a stale-head report, or a required test
command reported zero tests.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

SCHEMA = "axiom.readiness.gates.v1"
REPORT_SCHEMAS = {
    "rust-exit-readiness": "axiom.rust_exit.readiness.v1",
    "self-hosting-language-readiness": "axiom.self_hosting.language_readiness.v0",
    "snapshot-bootstrap-readiness": "axiom.self_hosting.snapshot_bootstrap_readiness.v0",
}
TEST_COMMANDS = {
    "native-build-purity": (
        "cargo test --manifest-path stage1/Cargo.toml -p axiomc --test cranelift_backend "
        "cranelift_backend_pure_artifact_is_invariant_to_build_env_and_stdin -- --nocapture"
    ),
    "self-hosting-spike-parity": "bash scripts/ci/run-self-hosting-spike-parity.sh",
}


def exact_sha(value: object) -> bool:
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{40}", value) is not None


def run_shell(command: str, root: Path) -> tuple[int, str]:
    result = subprocess.run(
        ["bash", "-o", "pipefail", "-c", command],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    return result.returncode, result.stdout


def validate_child_report(
    name: str, report: object, head_sha: str, exit_code: int, command: str
) -> list[str]:
    errors: list[str] = []
    if not isinstance(report, dict):
        return [f"{name}: report root must be an object"]
    if report.get("schema") != REPORT_SCHEMAS[name]:
        errors.append(f"{name}: unexpected schema {report.get('schema')!r}")
    if report.get("ready") not in {True, False}:
        errors.append(f"{name}: ready must be boolean")
    checks = report.get("checks")
    if not isinstance(checks, list) or not checks:
        errors.append(f"{name}: checks must be a non-empty array")
    elif any(
        not isinstance(item, dict) or item.get("status") not in {"pass", "fail"}
        for item in checks
    ):
        errors.append(f"{name}: checks contain malformed status evidence")
    if exit_code not in {0, 1}:
        errors.append(f"{name}: checker did not execute as a readiness command (exit {exit_code})")
    expected_exit = 0 if report.get("ready") is True else 1
    if exit_code != expected_exit:
        errors.append(
            f"{name}: exit {exit_code} does not match ready={report.get('ready')!r}"
        )
    report["headSha"] = head_sha
    report["executed"] = True
    report["executedCommand"] = command
    report["exitCode"] = exit_code
    return errors


def validate_test_output(name: str, exit_code: int, output: str) -> tuple[list[str], int]:
    errors: list[str] = []
    counts = [int(value) for value in re.findall(r"running\s+(\d+)\s+tests?", output)]
    tests_run = max(counts, default=0)
    if tests_run < 1:
        errors.append(f"{name}: executable validation produced zero-test evidence")
    return errors, tests_run


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--head-sha", required=True)
    parser.add_argument("--require-issue-states", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = args.repo_root.resolve()
    output = args.output_dir.resolve()
    errors: list[str] = []
    if not exact_sha(args.head_sha):
        errors.append("--head-sha must be an exact lowercase commit SHA")
    try:
        actual_head = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True
        ).strip()
    except (OSError, subprocess.CalledProcessError) as error:
        actual_head = ""
        errors.append(f"cannot resolve checkout HEAD: {error}")
    if actual_head != args.head_sha:
        errors.append(f"readiness head {args.head_sha} does not match checkout HEAD {actual_head}")

    output.mkdir(parents=True, exist_ok=True)
    for name in (*REPORT_SCHEMAS, *TEST_COMMANDS, "readiness-gates"):
        for suffix in (".json", ".log"):
            (output / f"{name}{suffix}").unlink(missing_ok=True)

    reports: dict[str, dict] = {}
    for name, schema in REPORT_SCHEMAS.items():
        command = {
            "rust-exit-readiness": "bash scripts/ci/check-rust-exit-readiness.sh --json",
            "self-hosting-language-readiness": "python3 scripts/ci/check-self-hosting-language-readiness.py --json",
            "snapshot-bootstrap-readiness": "python3 scripts/ci/check-snapshot-bootstrap-readiness.py --json",
        }[name]
        if name != "snapshot-bootstrap-readiness" and args.require_issue_states:
            command += " --require-issue-states"
        exit_code, stdout = run_shell(command, root)
        report_path = output / f"{name}.json"
        try:
            report = json.loads(stdout)
        except json.JSONDecodeError as error:
            report = {"schema": schema, "ready": False, "checks": []}
            errors.append(f"{name}: checker emitted malformed JSON: {error}")
        errors.extend(validate_child_report(name, report, args.head_sha, exit_code, command))
        report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        reports[name] = report

    tests: dict[str, dict] = {}
    for name, command in TEST_COMMANDS.items():
        exit_code, stdout = run_shell(command, root)
        (output / f"{name}.log").write_text(stdout, encoding="utf-8")
        test_errors, tests_run = validate_test_output(name, exit_code, stdout)
        errors.extend(test_errors)
        tests[name] = {
            "command": command,
            "headSha": args.head_sha,
            "executed": True,
            "exitCode": exit_code,
            "testsRun": tests_run,
            "status": "passed" if exit_code == 0 and not test_errors else "failed",
        }

    aggregate = {
        "schema": SCHEMA,
        "headSha": args.head_sha,
        "executed": True,
        "status": "ready" if all(report.get("ready") is True for report in reports.values()) else "blocked",
        "evidenceValid": not errors,
        "reports": {
            name: {
                "path": f"{name}.json",
                "headSha": report.get("headSha"),
                "ready": report.get("ready"),
                "exitCode": report.get("exitCode"),
            }
            for name, report in reports.items()
        },
        "tests": tests,
        "errors": errors,
    }
    (output / "readiness-gates.json").write_text(
        json.dumps(aggregate, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
