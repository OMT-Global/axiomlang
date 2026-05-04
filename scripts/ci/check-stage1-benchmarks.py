#!/usr/bin/env python3
from __future__ import annotations

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

ROUNDS = 5
BASELINE_FLOOR_MS = 50.0
COLD_BUILD_LIMIT_MULTIPLIER = 4.0
WARM_BUILD_LIMIT_MULTIPLIER = 2.0

REPO_ROOT = Path(__file__).resolve().parents[2]
AXIOMC_MANIFEST = REPO_ROOT / "stage1/Cargo.toml"
AXIOMC_BIN = REPO_ROOT / "stage1/target/debug/axiomc"
REF_ROOT = REPO_ROOT / "stage1/benchmarks/reference"
BASELINE_PATH = REPO_ROOT / "stage1/benchmarks/baselines/stage1-build-median.json"


@dataclass(frozen=True)
class Workload:
    name: str
    kind: str
    project: Path
    reference: Path


WORKLOADS = [
    Workload("hello", "compute", REPO_ROOT / "stage1/examples/hello", REF_ROOT / "hello"),
    Workload("capabilities", "io", REPO_ROOT / "stage1/examples/capabilities", REF_ROOT / "capabilities"),
    Workload("stdlib_async", "concurrency", REPO_ROOT / "stage1/examples/stdlib_async", REF_ROOT / "stdlib_async"),
]


def run(cmd: list[str], *, cwd: Path | None = None) -> float:
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
        if completed.stdout:
            sys.stdout.write(completed.stdout)
        if completed.stderr:
            sys.stderr.write(completed.stderr)
        raise SystemExit(completed.returncode)
    return elapsed_ms


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


def check_limit(label: str, actual_ms: float, limit_ms: float) -> None:
    status = "PASS" if actual_ms <= limit_ms else "FAIL"
    print(f"{status} {label}: {actual_ms:.1f} ms <= {limit_ms:.1f} ms")
    if actual_ms > limit_ms:
        raise SystemExit(1)


def load_regression_baseline() -> dict | None:
    if not BASELINE_PATH.exists():
        print(f"WARN benchmark regression baseline is missing: {BASELINE_PATH}")
        return None
    with BASELINE_PATH.open(encoding="utf-8") as handle:
        return json.load(handle)


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
        actual_medians = workload_report.get("medians_ms", {})
        for metric_name, actual_value in actual_medians.items():
            baseline_value = baseline_medians.get(metric_name)
            if baseline_value is None:
                warnings.append(f"{workload_name}.{metric_name}: missing baseline metric")
                continue
            limit = float(baseline_value) * (1.0 + tolerance_pct)
            if float(actual_value) > limit:
                warnings.append(
                    f"{workload_name}.{metric_name}: {actual_value:.1f} ms exceeds "
                    f"baseline {float(baseline_value):.1f} ms + {tolerance_pct:.0%}"
                )
    return warnings


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

    check_limit(f"{workload.name} axiom cold build", axiom_cold_median, cold_limit)
    check_limit(f"{workload.name} axiom warm build", axiom_warm_median, warm_limit)
    return result


def main() -> int:
    os.chdir(REPO_ROOT)
    ensure_tools()
    build_axiomc()

    with tempfile.TemporaryDirectory(prefix="axiom-stage1-bench-") as temp_name:
        temp_dir = Path(temp_name)
        workloads = {workload.name: benchmark_workload(workload, temp_dir) for workload in WORKLOADS}

    report = {
        "rounds": ROUNDS,
        "baseline_floor_ms": BASELINE_FLOOR_MS,
        "cold_build_limit_multiplier": COLD_BUILD_LIMIT_MULTIPLIER,
        "warm_build_limit_multiplier": WARM_BUILD_LIMIT_MULTIPLIER,
        "workloads": workloads,
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
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
