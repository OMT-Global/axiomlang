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
=======
from typing import Any, Callable
>>>>>>> origin/codex/issue-427-python-exit-readiness

ROUNDS = 5
BASELINE_FLOOR_MS = 50.0
COLD_BUILD_LIMIT_MULTIPLIER = 4.0
WARM_BUILD_LIMIT_MULTIPLIER = 2.0
REGRESSION_TOLERANCE = 0.35

REPO_ROOT = Path(__file__).resolve().parents[2]
AXIOMC_MANIFEST = REPO_ROOT / "stage1/Cargo.toml"
AXIOMC_BIN = REPO_ROOT / "stage1/target/debug/axiomc"
REF_ROOT = REPO_ROOT / "stage1/benchmarks/reference"
BASELINE_PATH = REPO_ROOT / "stage1/benchmarks/baselines/stage1-build-median.json"
DIAGNOSTIC_FIXTURE = REPO_ROOT / "stage1/conformance/fail/ownership_use_after_move"
CAPABILITY_NAMES = ["fs", "fs:write", "net", "process", "env", "clock", "crypto", "ffi"]
<<<<<<< HEAD
<<<<<<< HEAD
>>>>>>> origin/codex/worker-a-issue-379-fmt-json
>>>>>>> origin/codex/issue-380-doc-json
>>>>>>> origin/codex/issue-376-doctor-json
>>>>>>> origin/codex/issue-377-inspect-symbols
>>>>>>> origin/codex/issue-378-inspect-graph
>>>>>>> origin/codex/issue-406-collection-lookup
>>>>>>> origin/codex/issue-383-new-templates
>>>>>>> origin/codex/agent-f-fs
>>>>>>> origin/codex/issue-387-capability-validation
>>>>>>> origin/codex/worker-h-issue-413
>>>>>>> origin/codex/issue-369-check-fixtures
>>>>>>> origin/codex/issue-370-command-fixtures
>>>>>>> origin/codex/issue-418-schema-metadata
>>>>>>> origin/codex/issue-422-comparison-gate
CALIBRATION_BASELINE_PATH = REPO_ROOT / "stage1/benchmarks/stage1-build-baseline.json"
=======


@dataclass(frozen=True)
class Workload:
    name: str
    kind: str
    project: Path
    reference: Path


=======
@dataclass(frozen=True)
class CommandResult:
    elapsed_ms: float
    returncode: int
    stdout: str
    stderr: str


>>>>>>> origin/codex/issue-427-python-exit-readiness
WORKLOADS = [
    Workload("hello", "compute", REPO_ROOT / "stage1/examples/hello", REF_ROOT / "hello"),
    Workload("capabilities", "io", REPO_ROOT / "stage1/examples/capabilities", REF_ROOT / "capabilities"),
    Workload("stdlib_async", "concurrency", REPO_ROOT / "stage1/examples/stdlib_async", REF_ROOT / "stdlib_async"),
]


<<<<<<< HEAD
def run(cmd: list[str], *, cwd: Path | None = None) -> float:
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
    if completed.returncode != 0:
    result = CommandResult(elapsed_ms, completed.returncode, completed.stdout, completed.stderr)
    if check and completed.returncode != 0:
        if completed.stdout:
            sys.stdout.write(completed.stdout)
        if completed.stderr:
            sys.stderr.write(completed.stderr)
        raise SystemExit(completed.returncode)
    return elapsed_ms
    return result


def median_ms(samples: list[float]) -> float:
    return float(statistics.median(samples))


def collect_samples(fn, rounds: int = ROUNDS) -> tuple[list[float], float]:
    samples = [fn() for _ in range(rounds)]
    return samples, median_ms(samples)


def ensure_tools() -> None:
    required = ["cargo", "rustc", "go"]
    missing = [tool for tool in required if shutil.which(tool) is None]
    if missing:
        raise SystemExit(f"missing required benchmark tools: {', '.join(missing)}")
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


def axiom_build(workload: Workload, *, cold: bool) -> float:
    if cold:
        shutil.rmtree(workload.project / "dist", ignore_errors=True)
    return run([str(AXIOMC_BIN), "build", str(workload.project), "--json"], cwd=REPO_ROOT)


def go_build(workload: Workload, temp_dir: Path) -> float:
    output = temp_dir / f"{workload.name}-go"
    output.unlink(missing_ok=True)
    return run(["go", "build", "-o", str(output), str(workload.reference / "main.go")], cwd=temp_dir)


def rust_build(workload: Workload, temp_dir: Path) -> float:
    output = temp_dir / f"{workload.name}-rust"
    output.unlink(missing_ok=True)
    return run(["rustc", str(workload.reference / "main.rs"), "-O", "-o", str(output)], cwd=temp_dir)


def check_limit(label: str, actual_ms: float, limit_ms: float) -> dict:
    passed = actual_ms <= limit_ms
    status = "PASS" if passed else "FAIL"
    print(f"{status} {label}: {actual_ms:.1f} ms <= {limit_ms:.1f} ms")
    return {
        "label": label,
        "status": status.lower(),
        "actual_ms": actual_ms,
        "limit_ms": limit_ms,
    }
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
        print(f"WARN benchmark regression baseline is missing: {BASELINE_PATH}")
        print(f"WARN benchmark regression baseline is missing: {BASELINE_PATH}", file=sys.stderr)
        return None
    with BASELINE_PATH.open(encoding="utf-8") as handle:
        return json.load(handle)


=======
def baseline_comparison_medians(workload_report: dict) -> dict[str, float]:
    build_medians = workload_report.get("medians_ms", {}).get("build", {})
    return {
        "axiom_cold_build": float(build_medians["axiom_cold"]),
        "axiom_warm_build": float(build_medians["axiom_warm"]),
        "go_build": float(build_medians["go"]),
        "rust_build": float(build_medians["rust"]),
    }


>>>>>>> origin/codex/issue-427-python-exit-readiness
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
<<<<<<< HEAD
        actual_medians = workload_report.get("medians_ms", {})
        actual_medians = baseline_comparison_medians(workload_report)
        for metric_name, actual_value in actual_medians.items():
            baseline_value = baseline_medians.get(metric_name)
            if baseline_value is None:
                warnings.append(f"{workload_name}.{metric_name}: missing baseline metric")
                continue
            limit = float(baseline_value) * (1.0 + tolerance_pct)
            if float(actual_value) > limit:
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
=======
=======
=======
def benchmark_workload(workload: Workload, temp_dir: Path) -> dict:
    print(f"warming benchmark commands for {workload.name} ({workload.kind})...")
    axiom_build(workload, cold=True)
    axiom_build(workload, cold=False)
    go_build(workload, temp_dir)
    rust_build(workload, temp_dir)
=======
def load_baseline(path: Path) -> dict:
    if not path.exists():
        print(f"WARN no committed benchmark baseline found at {path}")
        return {}
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def compare_to_baseline(report: dict, baseline: dict, tolerance: float) -> list[dict]:
    comparisons: list[dict] = []
    baseline_workloads = baseline.get("workloads", {})
    for workload_name, workload_report in report["workloads"].items():
        baseline_medians = baseline_workloads.get(workload_name, {}).get("medians_ms", {})
        current_medians = workload_report.get("medians_ms", {})
        for metric in ("axiom_cold_build", "axiom_warm_build"):
            baseline_ms = baseline_medians.get(metric)
            current_ms = current_medians.get(metric)
            if baseline_ms is None or current_ms is None:
                print(f"WARN missing baseline metric {workload_name}.{metric}")
                continue
            limit_ms = float(baseline_ms) * (1.0 + tolerance)
            status = "PASS" if float(current_ms) <= limit_ms else "WARN"
            delta_pct = ((float(current_ms) - float(baseline_ms)) / float(baseline_ms)) * 100.0 if float(baseline_ms) else 0.0
            print(
                f"{status} {workload_name} {metric} vs committed baseline: "
                f"{float(current_ms):.1f} ms <= {limit_ms:.1f} ms "
                f"(baseline {float(baseline_ms):.1f} ms, delta {delta_pct:+.1f}%)"
            )
            comparisons.append({
                "workload": workload_name,
                "metric": metric,
                "status": status.lower(),
                "current_ms": float(current_ms),
                "baseline_ms": float(baseline_ms),
                "limit_ms": limit_ms,
                "delta_pct": delta_pct,
            })
    return comparisons


def benchmark_workload(workload: Workload, temp_dir: Path) -> dict:
    print(f"warming benchmark commands for {workload.name} ({workload.kind})...")
    axiom_build(workload, cold=True)
    axiom_build(workload, cold=False)
    go_build(workload, temp_dir)
    rust_build(workload, temp_dir)

    print(f"collecting benchmark medians for {workload.name}...")
    axiom_cold_samples, axiom_cold_median = collect_samples(lambda: axiom_build(workload, cold=True))
    axiom_warm_samples, axiom_warm_median = collect_samples(lambda: axiom_build(workload, cold=False))
    go_samples, go_median = collect_samples(lambda: go_build(workload, temp_dir))
    rust_samples, rust_median = collect_samples(lambda: rust_build(workload, temp_dir))

    reference_floor = max(min(go_median, rust_median), BASELINE_FLOOR_MS)
    cold_limit = reference_floor * COLD_BUILD_LIMIT_MULTIPLIER
    warm_limit = reference_floor * WARM_BUILD_LIMIT_MULTIPLIER

    result = {
        "kind": workload.kind,
        "samples_ms": {
            "axiom_cold_build": axiom_cold_samples,
            "axiom_warm_build": axiom_warm_samples,
            "go_build": go_samples,
            "rust_build": rust_samples,
        },
        "medians_ms": {
            "axiom_cold_build": axiom_cold_median,
            "axiom_warm_build": axiom_warm_median,
            "go_build": go_median,
            "rust_build": rust_median,
        },
        "reference_floor_ms": reference_floor,
        "limits_ms": {
            "axiom_cold_build": cold_limit,
            "axiom_warm_build": warm_limit,
        },
    }

    native_budget_checks = [
        check_limit(f"{workload.name} axiom cold build vs native reference budget", axiom_cold_median, cold_limit),
        check_limit(f"{workload.name} axiom warm build vs native reference budget", axiom_warm_median, warm_limit),
    ]
    result["native_reference_budget"] = {
        "blocking": True,
        "checks": native_budget_checks,
    }
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description="Run the non-blocking stage1 build benchmark comparison.")
    parser.add_argument("--baseline", type=Path, default=CALIBRATION_BASELINE_PATH, help="committed JSON baseline to compare against")
    parser.add_argument("--tolerance", type=float, default=REGRESSION_TOLERANCE, help="allowed fractional regression before WARN output")
    args = parser.parse_args()

    os.chdir(REPO_ROOT)
    ensure_tools()
    build_axiomc()

    with tempfile.TemporaryDirectory(prefix="axiom-stage1-bench-") as temp_name:
=======
def file_size(path: Path) -> int | None:
    return path.stat().st_size if path.exists() else None


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
=======
        "schema_version": "axiom.stage1.comparison.v1",
        "policy": "advisory-nonblocking",
>>>>>>> origin/codex/issue-427-python-exit-readiness
        "rounds": ROUNDS,
        "baseline_floor_ms": BASELINE_FLOOR_MS,
        "cold_build_limit_multiplier": COLD_BUILD_LIMIT_MULTIPLIER,
        "warm_build_limit_multiplier": WARM_BUILD_LIMIT_MULTIPLIER,
        "regression_tolerance": args.tolerance,
        "baseline_path": str(args.baseline.relative_to(REPO_ROOT) if args.baseline.is_relative_to(REPO_ROOT) else args.baseline),
        "workloads": workloads,
<<<<<<< HEAD
    }

    report["baseline_comparisons"] = compare_to_baseline(report, load_baseline(args.baseline), args.tolerance)
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

    print(json.dumps(report, indent=2))

    failed_budget_checks = [
        check
        for workload in report["workloads"].values()
        for check in workload.get("native_reference_budget", {}).get("checks", [])
        if check.get("status") == "fail"
    ]
    if failed_budget_checks:
        print("FAIL native reference benchmark budget check is blocking")
        return 1

    return 0
    if advisory_failures:
        print("ADVISORY comparison limit findings: " + ", ".join(advisory_failures), file=sys.stderr)
    return 1 if args.enforce and advisory_failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
