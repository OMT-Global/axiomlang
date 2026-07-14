#!/usr/bin/env python3
"""Run the deterministic, contract-backed autonomy benchmark for issue #1424."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_FIXTURE = REPO_ROOT / "stage1/agent-autonomy/benchmark-v0.json"
DEFAULT_BASELINE = REPO_ROOT / "stage1/agent-autonomy/readiness-baseline-v0.json"
CATEGORIES = {
    "feature",
    "bug",
    "refactor",
    "migration",
    "ci_repair",
    "merge_conflict",
    "impossible",
    "ambiguous",
}


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read {path}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def validate_fixture(fixture: dict[str, Any]) -> list[dict[str, Any]]:
    if fixture.get("schema_version") != "axiom.agent_autonomy.benchmark.v0":
        raise ValueError("fixture must declare axiom.agent_autonomy.benchmark.v0")
    if not isinstance(fixture.get("suite_id"), str) or not fixture["suite_id"].strip():
        raise ValueError("fixture suite_id must be non-empty")
    tasks = fixture.get("tasks")
    if not isinstance(tasks, list) or not tasks:
        raise ValueError("fixture tasks must be a non-empty array")
    seen: set[str] = set()
    for task in tasks:
        if not isinstance(task, dict):
            raise ValueError("every fixture task must be an object")
        task_id = task.get("id")
        if not isinstance(task_id, str) or not task_id.strip() or task_id in seen:
            raise ValueError("every fixture task needs a unique non-empty id")
        seen.add(task_id)
        if task.get("category") not in CATEGORIES:
            raise ValueError(f"task {task_id} has an unsupported category")
        if task.get("expected_outcome") not in {"complete", "stop"}:
            raise ValueError(f"task {task_id} must expect complete or stop")
        if not isinstance(task.get("ci"), bool):
            raise ValueError(f"task {task_id} must declare whether it belongs in CI")
        command = task.get("command")
        if not isinstance(command, list) or not command or not all(isinstance(part, str) and part for part in command):
            raise ValueError(f"task {task_id} must declare a non-empty string command")
    return tasks


def validate_baseline(baseline: dict[str, Any], fixture: dict[str, Any]) -> None:
    if baseline.get("schema_version") != "axiom.agent_autonomy.readiness_baseline.v0":
        raise ValueError("baseline must declare axiom.agent_autonomy.readiness_baseline.v0")
    if baseline.get("suite_id") != fixture["suite_id"]:
        raise ValueError("baseline suite_id must match the fixture")
    required_categories = baseline.get("required_categories")
    if not isinstance(required_categories, list) or set(required_categories) != CATEGORIES:
        raise ValueError("baseline must require every benchmark category exactly once")
    for key in ("minimum_tasks", "minimum_ci_tasks", "minimum_stop_tasks", "maximum_false_greens"):
        if not isinstance(baseline.get(key), int) or baseline[key] < 0:
            raise ValueError(f"baseline {key} must be a non-negative integer")
    for key in ("minimum_pass_rate", "minimum_stop_pass_rate"):
        if not isinstance(baseline.get(key), (int, float)) or not 0 <= baseline[key] <= 1:
            raise ValueError(f"baseline {key} must be between zero and one")
    end_to_end = baseline.get("end_to_end")
    if not isinstance(end_to_end, dict) or end_to_end.get("status") not in {"pending", "complete"}:
        raise ValueError("baseline must declare a pending or complete end_to_end status")
    if end_to_end["status"] == "pending" and not end_to_end.get("blockers"):
        raise ValueError("a pending end_to_end status must explain its blockers")


def run_task(task: dict[str, Any]) -> dict[str, Any]:
    started = time.perf_counter()
    completed = subprocess.run(task["command"], cwd=REPO_ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    elapsed_ms = round((time.perf_counter() - started) * 1000, 3)
    transcript = completed.stdout + completed.stderr
    # Cargo treats a test filter that matched nothing as success. A benchmark
    # task must prove that its adversarial assertion actually ran, otherwise a
    # typo in a fixture would manufacture a false green.
    test_executed = "running 1 test" in transcript and "test result: ok. 1 passed;" in transcript
    passed = completed.returncode == 0 and test_executed
    return {
        "id": task["id"],
        "category": task["category"],
        "expected_outcome": task["expected_outcome"],
        "observed_outcome": task["expected_outcome"] if passed else "failed",
        "ci": task["ci"],
        "passed": passed,
        "false_green": completed.returncode == 0 and not passed,
        "elapsed_ms": elapsed_ms,
        "transcript_digest": f"sha256:{hashlib.sha256(transcript.encode()).hexdigest()}",
    }


def report_for(tasks: list[dict[str, Any]], baseline: dict[str, Any], subset: str) -> dict[str, Any]:
    selected = [task for task in tasks if subset == "all" or task["ci"]]
    results = [run_task(task) for task in selected]
    passed = [result for result in results if result["passed"]]
    stopped = [result for result in results if result["expected_outcome"] == "stop"]
    stopped_passed = [result for result in stopped if result["passed"]]
    false_greens = [result for result in results if result["false_green"]]
    categories = {result["category"] for result in results}
    pass_rate = len(passed) / len(results) if results else 0.0
    stop_pass_rate = len(stopped_passed) / len(stopped) if stopped else 0.0
    checks = {
        "minimum_tasks": len(results) >= baseline["minimum_tasks"] if subset == "all" else len(results) >= baseline["minimum_ci_tasks"],
        "minimum_stop_tasks": len(stopped) >= baseline["minimum_stop_tasks"] if subset == "all" else bool(stopped),
        "minimum_pass_rate": pass_rate >= baseline["minimum_pass_rate"],
        "minimum_stop_pass_rate": stop_pass_rate >= baseline["minimum_stop_pass_rate"],
        "maximum_false_greens": len(false_greens) <= baseline["maximum_false_greens"],
        "required_categories": categories.issuperset(baseline["required_categories"]) if subset == "all" else True,
    }
    end_to_end = baseline["end_to_end"]
    return {
        "schema_version": "axiom.agent_autonomy.readiness.v0",
        "suite_id": baseline["suite_id"],
        "subset": subset,
        "ready": all(checks.values()) and end_to_end["status"] == "complete",
        "benchmark_passed": all(checks.values()),
        "checks": checks,
        "task_count": len(results),
        "pass_rate": round(pass_rate, 3),
        "stop_pass_rate": round(stop_pass_rate, 3),
        "false_green_count": len(false_greens),
        "end_to_end": end_to_end,
        "results": results,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Run Axiom's deterministic autonomy benchmark.")
    parser.add_argument("--fixture", type=Path, default=DEFAULT_FIXTURE)
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    parser.add_argument("--subset", choices=("all", "ci"), default="all")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--validate-only", action="store_true")
    parser.add_argument("--check", action="store_true", help="fail when the benchmark threshold does not pass")
    args = parser.parse_args()

    try:
        fixture = load_json(args.fixture)
        tasks = validate_fixture(fixture)
        baseline = load_json(args.baseline)
        validate_baseline(baseline, fixture)
    except ValueError as error:
        print(f"agent autonomy benchmark configuration error: {error}", file=sys.stderr)
        return 2

    if args.validate_only:
        print(json.dumps({"valid": True, "suite_id": fixture["suite_id"]}, indent=2))
        return 0

    report = report_for(tasks, baseline, args.subset)
    encoded = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    print(encoded, end="")
    return 0 if not args.check or report["benchmark_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
