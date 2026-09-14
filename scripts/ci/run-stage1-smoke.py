#!/usr/bin/env python3
"""Exercise the shared Stage1 example contracts against structured CLI evidence."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
EXPECTATIONS = ROOT / "scripts/ci/stage1-smoke-expectations.json"
VALIDATOR = ROOT / "scripts/ci/validate-stage1-smoke-report.py"
MODES = {"direct-native", "bounded-static", "blocked"}


def load_contracts() -> list[dict]:
    rows = json.loads(EXPECTATIONS.read_text(encoding="utf-8"))
    if not isinstance(rows, list) or not rows:
        raise ValueError("smoke expectations must be a nonempty array")
    seen = set()
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError("each smoke expectation must be an object")
        example = row.get("example")
        if not isinstance(example, str) or not example or any(
            char not in "abcdefghijklmnopqrstuvwxyz0123456789_" for char in example
        ):
            raise ValueError(f"invalid smoke example: {example!r}")
        if example in seen:
            raise ValueError(f"duplicate smoke example: {example}")
        seen.add(example)
        if row.get("lane") not in {"basic", "stdlib"}:
            raise ValueError(f"{example}: unknown smoke lane")
        if row.get("build") not in MODES or row.get("test", "direct-native") not in MODES:
            raise ValueError(f"{example}: unknown lowering expectation")
        if not (ROOT / "stage1/examples" / example / "axiom.toml").is_file():
            raise ValueError(f"{example}: missing example manifest")
        if "runtime_env" in row and row["build"] != "direct-native":
            raise ValueError(f"{example}: runtime proof requires a direct-native build")
    return rows


def invoke(args: list[str], env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True)


def cargo(command: str, project: str, *args: str) -> list[str]:
    return [
        "cargo", "run", "--manifest-path", "stage1/Cargo.toml", "-p", "axiomc",
        "--", command, project, *args,
    ]


def report_command(
    row: dict, command: str, reports: Path, env: dict[str, str]
) -> dict:
    example = row["example"]
    project = f"stage1/examples/{example}"
    mode = row.get(command)
    args = [] if command == "check" else ["--backend", "cranelift"]
    if command == "build" and row.get("package"):
        args += ["--package", row["package"]]
    if command == "test":
        args += row.get("test_args", [])
    result = invoke(cargo(command, project, *args, "--json"), env)
    report = reports / f"{example}-{command}.json"
    report.write_text(result.stdout, encoding="utf-8")
    expected_failure = mode == "blocked"
    if (result.returncode != 0) != expected_failure:
        sys.stderr.write(result.stderr + result.stdout)
        raise ValueError(f"{command} {example}: expected {'failure' if expected_failure else 'success'}")
    payload = json.loads(result.stdout)
    if not isinstance(payload, dict):
        raise ValueError(f"{command} {example}: report must be an object")
    if command == "check":
        if payload.get("ok") is not True:
            raise ValueError(f"check {example}: report must pass")
        return payload
    validation = [
        sys.executable, str(VALIDATOR), "--report", str(report),
        "--command", command, "--project", project, "--expect", mode,
    ]
    if command == "test":
        for field, flag in (
            ("expected_success_cases", "--expected-success-case"),
            ("expected_bounded_static_cases", "--expected-bounded-static-case"),
            ("expected_blocked_cases", "--expected-blocked-case"),
        ):
            for case in row.get(field, []):
                validation += [flag, case]
    subprocess.run(validation, cwd=ROOT, check=True)
    return payload


def prove_runtime_env(row: dict, build: dict, reports: Path, env: dict[str, str]) -> None:
    """Run one compiled artifact under changing inputs, without rebuilding it."""
    proof = row["runtime_env"]
    binary = build.get("binary")
    if not isinstance(binary, str) or not binary:
        raise ValueError(f"{row['example']}: runtime proof requires a binary")
    binary_path = Path(binary)
    if not binary_path.is_absolute():
        binary_path = ROOT / binary_path
    for index, value in enumerate([None, *proof["values"]]):
        runtime_env = env.copy()
        runtime_env.pop(proof["name"], None)
        if value is not None:
            runtime_env[proof["name"]] = value
        result = invoke([str(binary_path)], runtime_env)
        expected = "none\n" if value is None else value + "\n"
        (reports / f"{row['example']}-runtime-{index}.stdout").write_text(
            result.stdout, encoding="utf-8"
        )
        if result.returncode != 0 or result.stdout != expected:
            raise ValueError(
                f"{row['example']}: runtime environment proof {index} failed: "
                f"expected {expected!r}, got {result.stdout!r} (exit {result.returncode})"
            )


def run_contract(row: dict, reports: Path) -> None:
    env = os.environ.copy()
    if row.get("runtime_env"):
        # Keep the fixture test deterministic even when the caller defines it.
        env.pop(row["runtime_env"]["name"], None)
    report_command(row, "check", reports, env)
    build = report_command(row, "build", reports, env)
    if row["build"] != "blocked":
        if row.get("runtime_env"):
            # Prove this build artifact before `run` can rebuild the same path.
            prove_runtime_env(row, build, reports, env)
        project = f"stage1/examples/{row['example']}"
        args = ["--backend", "cranelift"]
        if row.get("package"):
            args += ["--package", row["package"]]
        result = invoke(cargo("run", project, *args), env)
        if result.returncode != 0:
            sys.stderr.write(result.stderr)
            raise ValueError(f"run {row['example']} failed")
        sys.stdout.write(result.stdout)
    if row.get("test"):
        report_command(row, "test", reports, env)
    print(f"validated {row['example']}: build={row['build']} test={row.get('test', 'not-in-lane')}", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lane", required=True, choices=("basic", "stdlib"))
    parser.add_argument("--report-dir", type=Path, default=ROOT / ".axiom-build/reports/stage1-smoke")
    args = parser.parse_args()
    rows = load_contracts()
    reports = args.report_dir.resolve()
    reports.mkdir(parents=True, exist_ok=True)
    for row in rows:
        if row["lane"] == args.lane:
            run_contract(row, reports)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
