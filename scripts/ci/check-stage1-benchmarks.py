#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

ROUNDS = 5
BASELINE_FLOOR_MS = 50.0
COLD_BUILD_LIMIT_MULTIPLIER = 4.0
WARM_BUILD_LIMIT_MULTIPLIER = 2.0

REPO_ROOT = Path(__file__).resolve().parents[2]
AXIOMC_MANIFEST = REPO_ROOT / "stage1/Cargo.toml"
AXIOMC_BIN = REPO_ROOT / "stage1/target/debug/axiomc"
REF_ROOT = REPO_ROOT / "stage1/benchmarks/reference"
BASELINE_PATH = REPO_ROOT / "stage1/benchmarks/baselines/stage1-build-median.json"
DIAGNOSTIC_FIXTURE = REPO_ROOT / "stage1/conformance/fail/ownership_use_after_move"
CAPABILITY_NAMES = ["fs", "fs:write", "net", "process", "env", "clock", "crypto", "ffi"]
>>>>>>> origin/codex/worker-a-issue-379-fmt-json
>>>>>>> origin/codex/issue-380-doc-json
>>>>>>> origin/codex/issue-376-doctor-json
>>>>>>> origin/codex/issue-377-inspect-symbols
>>>>>>> origin/codex/issue-378-inspect-graph
>>>>>>> origin/codex/issue-406-collection-lookup
>>>>>>> origin/codex/issue-383-new-templates
>>>>>>> origin/codex/agent-f-fs


@dataclass(frozen=True)
class Workload:
    name: str
    kind: str
    project: Path
    reference: Path


@dataclass(frozen=True)
class CommandResult:
    elapsed_ms: float
    returncode: int
    stdout: str
    stderr: str


WORKLOADS = [
    Workload("hello", "compute", REPO_ROOT / "stage1/examples/hello", REF_ROOT / "hello"),
    Workload("capabilities", "io", REPO_ROOT / "stage1/examples/capabilities", REF_ROOT / "capabilities"),
    Workload("stdlib_async", "concurrency", REPO_ROOT / "stage1/examples/stdlib_async", REF_ROOT / "stdlib_async"),
]


def timed_run(cmd: list[str], *, cwd: Path | None = None, check: bool = True) -> CommandResult:
    started = time.perf_counter()
    completed = subprocess.run(
        cmd,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    result = CommandResult(elapsed_ms, completed.returncode, completed.stdout, completed.stderr)
    if check and completed.returncode != 0:
        if completed.stdout:
            sys.stdout.write(completed.stdout)
        if completed.stderr:
            sys.stderr.write(completed.stderr)
        raise SystemExit(completed.returncode)
    return result


def median_ms(samples: list[float]) -> float:
    return float(statistics.median(samples))


def collect_samples(fn: Callable[[], CommandResult], rounds: int = ROUNDS) -> tuple[list[float], float]:
    samples = [fn().elapsed_ms for _ in range(rounds)]
    return samples, median_ms(samples)


def ensure_tools() -> list[str]:
    required = ["cargo", "rustc", "go"]
    return [tool for tool in required if shutil.which(tool) is None]


def build_axiomc() -> None:
    subprocess.run(
        ["cargo", "build", "--manifest-path", str(AXIOMC_MANIFEST), "-p", "axiomc"],
        cwd=REPO_ROOT,
        check=True,
    )


def axiom_build(workload: Workload, *, cold: bool) -> CommandResult:
    if cold:
        shutil.rmtree(workload.project / "dist", ignore_errors=True)
    return timed_run([str(AXIOMC_BIN), "build", str(workload.project), "--json"], cwd=REPO_ROOT)


def axiom_binary_from_build(result: CommandResult) -> Path:
    payload = json.loads(result.stdout)
    return REPO_ROOT / payload["binary"] if not Path(payload["binary"]).is_absolute() else Path(payload["binary"])


def go_build(workload: Workload, temp_dir: Path) -> tuple[CommandResult, Path]:
    output = temp_dir / f"{workload.name}-go"
    output.unlink(missing_ok=True)
    result = timed_run(["go", "build", "-o", str(output), str(workload.reference / "main.go")], cwd=temp_dir)
    return result, output


def rust_build(workload: Workload, temp_dir: Path) -> tuple[CommandResult, Path]:
    output = temp_dir / f"{workload.name}-rust"
    output.unlink(missing_ok=True)
    result = timed_run(["rustc", str(workload.reference / "main.rs"), "-O", "-o", str(output)], cwd=temp_dir)
    return result, output


def run_binary(binary: Path) -> CommandResult:
    return timed_run([str(binary)], cwd=REPO_ROOT)


def load_regression_baseline() -> dict | None:
    if not BASELINE_PATH.exists():
        print(f"WARN benchmark regression baseline is missing: {BASELINE_PATH}", file=sys.stderr)
        return None
    with BASELINE_PATH.open(encoding="utf-8") as handle:
        return json.load(handle)


def baseline_comparison_medians(workload_report: dict) -> dict[str, float]:
    build_medians = workload_report.get("medians_ms", {}).get("build", {})
    return {
        "axiom_cold_build": float(build_medians["axiom_cold"]),
        "axiom_warm_build": float(build_medians["axiom_warm"]),
        "go_build": float(build_medians["go"]),
        "rust_build": float(build_medians["rust"]),
    }


def compare_regression_baseline(report: dict, baseline: dict | None) -> list[str]:
    if baseline is None:
        return ["missing committed benchmark baseline"]

    tolerance_pct = float(baseline.get("tolerance_pct", 0.35))
    warnings: list[str] = []
    baseline_workloads = baseline.get("workloads", {})
    report_workloads = report.get("workloads", {})

    for workload_name, workload_report in report_workloads.items():
        workload_baseline = baseline_workloads.get(workload_name)
        if workload_baseline is None:
            warnings.append(f"{workload_name}: missing baseline workload")
            continue
        baseline_medians = workload_baseline.get("medians_ms", {})
        actual_medians = baseline_comparison_medians(workload_report)
        for metric_name, actual_value in actual_medians.items():
            baseline_value = baseline_medians.get(metric_name)
            if baseline_value is None:
                warnings.append(f"{workload_name}.{metric_name}: missing baseline metric")
                continue
            limit = float(baseline_value) * (1.0 + tolerance_pct)
            if actual_value > limit:
                warnings.append(
                    f"{workload_name}.{metric_name}: {actual_value:.1f} ms exceeds "
                    f"baseline {float(baseline_value):.1f} ms + {tolerance_pct:.0%}"
                )
    return warnings


<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
def file_size(path: Path) -> int | None:
    return path.stat().st_size if path.exists() else None
=======
=======
=======
=======
=======
=======
=======
=======
def benchmark_workload(workload: Workload, temp_dir: Path) -> dict:
    print(f"warming benchmark commands for {workload.name} ({workload.kind})...")
    axiom_build(workload, cold=True)
    axiom_build(workload, cold=False)
    go_build(workload, temp_dir)
    rust_build(workload, temp_dir)


def capability_manifest_coverage(workload: Workload) -> dict[str, Any]:
    result = timed_run([str(AXIOMC_BIN), "caps", str(workload.project), "--json"], cwd=REPO_ROOT)
    payload = json.loads(result.stdout)
    descriptors = payload.get("capabilities", [])
    covered = sorted({item.get("name") for item in descriptors if item.get("enabled")})
    declared = sorted({item.get("name") for item in descriptors})
    return {
        "schema_version": payload.get("schema_version"),
        "declared": declared,
        "enabled": covered,
        "declared_count": len(declared),
        "known_capability_count": len(CAPABILITY_NAMES),
        "coverage_ratio": round(len(declared) / len(CAPABILITY_NAMES), 3),
    }


def diagnostic_quality() -> dict[str, Any]:
    result = timed_run([str(AXIOMC_BIN), "check", str(DIAGNOSTIC_FIXTURE), "--json"], cwd=REPO_ROOT, check=False)
    payload = json.loads(result.stdout) if result.stdout.strip().startswith("{") else {}
    error = payload.get("error", {})
    fields = ["kind", "message", "path", "line", "column", "code"]
    present = [field for field in fields if error.get(field) not in (None, "")]
    return {
        "fixture": str(DIAGNOSTIC_FIXTURE.relative_to(REPO_ROOT)),
        "returncode": result.returncode,
        "json": bool(payload),
        "present_fields": present,
        "score": round(len(present) / len(fields), 3),
        "message": error.get("message"),
    }


def compare_limit(actual_ms: float, limit_ms: float) -> str:
    return "pass" if actual_ms <= limit_ms else "advisory-fail"


def benchmark_workload(workload: Workload, temp_dir: Path) -> dict[str, Any]:
    print(f"warming comparison commands for {workload.name} ({workload.kind})...", file=sys.stderr)
    axiom_warm_build = axiom_build(workload, cold=True)
    axiom_binary = axiom_binary_from_build(axiom_warm_build)
    go_warm_build, go_binary = go_build(workload, temp_dir)
    rust_warm_build, rust_binary = rust_build(workload, temp_dir)
    run_binary(axiom_binary)
    run_binary(go_binary)
    run_binary(rust_binary)

    print(f"collecting comparison medians for {workload.name}...", file=sys.stderr)
    axiom_cold_samples, axiom_cold_median = collect_samples(lambda: axiom_build(workload, cold=True))
    final_axiom_build = axiom_build(workload, cold=False)
    axiom_binary = axiom_binary_from_build(final_axiom_build)
    axiom_warm_samples, axiom_warm_median = collect_samples(lambda: axiom_build(workload, cold=False))
    go_build_samples, go_build_median = collect_samples(lambda: go_build(workload, temp_dir)[0])
    _, go_binary = go_build(workload, temp_dir)
    rust_build_samples, rust_build_median = collect_samples(lambda: rust_build(workload, temp_dir)[0])
    _, rust_binary = rust_build(workload, temp_dir)

    axiom_run_samples, axiom_run_median = collect_samples(lambda: run_binary(axiom_binary))
    go_run_samples, go_run_median = collect_samples(lambda: run_binary(go_binary))
    rust_run_samples, rust_run_median = collect_samples(lambda: run_binary(rust_binary))

    reference_floor = max(min(go_build_median, rust_build_median), BASELINE_FLOOR_MS)
    cold_limit = reference_floor * COLD_BUILD_LIMIT_MULTIPLIER
    warm_limit = reference_floor * WARM_BUILD_LIMIT_MULTIPLIER

    return {
        "kind": workload.kind,
        "policy": {
            "mode": "advisory",
            "reference_floor_ms": reference_floor,
            "limits_ms": {
                "axiom_cold_build": cold_limit,
                "axiom_warm_build": warm_limit,
            },
            "status": {
                "axiom_cold_build": compare_limit(axiom_cold_median, cold_limit),
                "axiom_warm_build": compare_limit(axiom_warm_median, warm_limit),
            },
        },
        "samples_ms": {
            "build": {
                "axiom_cold": axiom_cold_samples,
                "axiom_warm": axiom_warm_samples,
                "go": go_build_samples,
                "rust": rust_build_samples,
            },
            "run": {
                "axiom": axiom_run_samples,
                "go": go_run_samples,
                "rust": rust_run_samples,
            },
        },
        "medians_ms": {
            "build": {
                "axiom_cold": axiom_cold_median,
                "axiom_warm": axiom_warm_median,
                "go": go_build_median,
                "rust": rust_build_median,
            },
            "run": {
                "axiom": axiom_run_median,
                "go": go_run_median,
                "rust": rust_run_median,
            },
        },
        "binary_size_bytes": {
            "axiom": file_size(axiom_binary),
            "go": file_size(go_binary),
            "rust": file_size(rust_binary),
        },
        "capability_manifest_coverage": capability_manifest_coverage(workload),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Report advisory Go/Rust/Axiom stage1 workload comparisons.")
    parser.add_argument("--json-out", type=Path, help="write the machine-readable report to this path")
    parser.add_argument("--enforce", action="store_true", help="fail when advisory limits are exceeded")
    args = parser.parse_args()

    os.chdir(REPO_ROOT)
    missing = ensure_tools()
    if missing:
        raise SystemExit(f"missing required benchmark tools: {', '.join(missing)}")
    build_axiomc()

    with tempfile.TemporaryDirectory(prefix="axiom-stage1-comparison-") as temp_name:
        temp_dir = Path(temp_name)
        workloads = {workload.name: benchmark_workload(workload, temp_dir) for workload in WORKLOADS}

    report = {
        "schema_version": "axiom.stage1.comparison.v1",
        "policy": "advisory-nonblocking",
        "rounds": ROUNDS,
        "baseline_floor_ms": BASELINE_FLOOR_MS,
        "cold_build_limit_multiplier": COLD_BUILD_LIMIT_MULTIPLIER,
        "warm_build_limit_multiplier": WARM_BUILD_LIMIT_MULTIPLIER,
        "workloads": workloads,
        "diagnostics_quality": diagnostic_quality(),
    }
    baseline_warnings = compare_regression_baseline(report, load_regression_baseline())
    report["regression_baseline"] = {
        "path": str(BASELINE_PATH.relative_to(REPO_ROOT)),
        "blocking": False,
        "warnings": baseline_warnings,
    }

    if baseline_warnings:
        print("WARN benchmark regression baseline comparison is non-blocking:")
        for warning in baseline_warnings:
            print(f"WARN {warning}")
    else:
        print("PASS benchmark regression baseline comparison")

    rendered = json.dumps(report, indent=2)
    print(rendered)
    if args.json_out:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(rendered + "\n", encoding="utf-8")

    advisory_failures = [
        f"{name}:{metric}"
        for name, data in workloads.items()
        for metric, status in data["policy"]["status"].items()
        if status != "pass"
    ]
    if advisory_failures:
        print("ADVISORY comparison limit findings: " + ", ".join(advisory_failures), file=sys.stderr)
    return 1 if args.enforce and advisory_failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
