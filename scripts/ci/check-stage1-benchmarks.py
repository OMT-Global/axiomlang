#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
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
REGRESSION_TOLERANCE = 0.35

REPO_ROOT = Path(__file__).resolve().parents[2]
AXIOMC_MANIFEST = REPO_ROOT / "stage1/Cargo.toml"
AXIOMC_BIN = REPO_ROOT / "stage1/target/debug/axiomc"
REF_ROOT = REPO_ROOT / "stage1/benchmarks/reference"
BASELINE_PATH = REPO_ROOT / "stage1/benchmarks/baselines/stage1-build-median.json"
DIAGNOSTIC_FIXTURE = REPO_ROOT / "stage1/conformance/fail/ownership_use_after_move"
CAPABILITY_NAMES = ["fs", "fs:write", "net", "process", "env", "clock", "crypto", "ffi", "async"]


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
    timed_run(["cargo", "build", "--manifest-path", str(AXIOMC_MANIFEST), "-p", "axiomc"], cwd=REPO_ROOT)


def axiom_build(workload: Workload, *, cold: bool) -> CommandResult:
    if cold:
        shutil.rmtree(workload.project / "dist", ignore_errors=True)
    return timed_run([str(AXIOMC_BIN), "build", str(workload.project), "--json"], cwd=REPO_ROOT)


def axiom_binary_from_build(result: CommandResult) -> Path:
    payload = json.loads(result.stdout)
    binary = Path(payload["binary"])
    return binary if binary.is_absolute() else REPO_ROOT / binary


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


def file_size(path: Path) -> int | None:
    return path.stat().st_size if path.exists() else None


def compare_limit(actual_ms: float, limit_ms: float) -> str:
    return "pass" if actual_ms <= limit_ms else "advisory-fail"


def capability_manifest_coverage(workload: Workload) -> dict[str, Any]:
    result = timed_run([str(AXIOMC_BIN), "caps", str(workload.project), "--json"], cwd=REPO_ROOT)
    payload = json.loads(result.stdout)
    descriptors = payload.get("capabilities", [])
    declared = sorted({item.get("name") for item in descriptors})
    enabled = sorted({item.get("name") for item in descriptors if item.get("enabled")})
    return {
        "schema_version": payload.get("schema_version"),
        "declared": declared,
        "enabled": enabled,
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


def load_baseline(path: Path) -> dict:
    if not path.exists():
        print(f"WARN no committed benchmark baseline found at {path}", file=sys.stderr)
        return {}
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def baseline_build_medians(workload_report: dict[str, Any]) -> dict[str, float]:
    build = workload_report.get("medians_ms", {}).get("build", {})
    return {
        "axiom_cold_build": float(build["axiom_cold"]),
        "axiom_warm_build": float(build["axiom_warm"]),
        "go_build": float(build["go"]),
        "rust_build": float(build["rust"]),
    }


def compare_to_baseline(report: dict[str, Any], baseline: dict[str, Any], tolerance: float) -> list[dict[str, Any]]:
    comparisons: list[dict[str, Any]] = []
    baseline_workloads = baseline.get("workloads", {})
    for workload_name, workload_report in report["workloads"].items():
        baseline_medians = baseline_workloads.get(workload_name, {}).get("medians_ms", {})
        for metric, current_ms in baseline_build_medians(workload_report).items():
            baseline_ms = baseline_medians.get(metric)
            if baseline_ms is None:
                print(f"WARN missing baseline metric {workload_name}.{metric}", file=sys.stderr)
                continue
            limit_ms = float(baseline_ms) * (1.0 + tolerance)
            delta_pct = ((float(current_ms) - float(baseline_ms)) / float(baseline_ms)) * 100.0 if float(baseline_ms) else 0.0
            status = "pass" if float(current_ms) <= limit_ms else "warn"
            print(
                f"{status.upper()} {workload_name} {metric} vs committed baseline: "
                f"{float(current_ms):.1f} ms <= {limit_ms:.1f} ms "
                f"(baseline {float(baseline_ms):.1f} ms, delta {delta_pct:+.1f}%)",
                file=sys.stderr,
            )
            comparisons.append(
                {
                    "workload": workload_name,
                    "metric": metric,
                    "status": status,
                    "current_ms": float(current_ms),
                    "baseline_ms": float(baseline_ms),
                    "limit_ms": limit_ms,
                    "delta_pct": delta_pct,
                }
            )
    return comparisons


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
    parser.add_argument("--baseline", type=Path, default=BASELINE_PATH, help="committed JSON baseline to compare against")
    parser.add_argument("--tolerance", type=float, default=REGRESSION_TOLERANCE, help="allowed fractional regression before WARN output")
    parser.add_argument("--enforce", action="store_true", help="fail when advisory limits are exceeded")
    args = parser.parse_args()

    missing = ensure_tools()
    if missing:
        raise SystemExit(f"missing required benchmark tools: {', '.join(missing)}")
    build_axiomc()

    with tempfile.TemporaryDirectory(prefix="axiom-stage1-comparison-") as temp_name:
        temp_dir = Path(temp_name)
        workloads = {workload.name: benchmark_workload(workload, temp_dir) for workload in WORKLOADS}

    report: dict[str, Any] = {
        "schema_version": "axiom.stage1.comparison.v1",
        "policy": "advisory-nonblocking",
        "rounds": ROUNDS,
        "baseline_floor_ms": BASELINE_FLOOR_MS,
        "cold_build_limit_multiplier": COLD_BUILD_LIMIT_MULTIPLIER,
        "warm_build_limit_multiplier": WARM_BUILD_LIMIT_MULTIPLIER,
        "regression_tolerance": args.tolerance,
        "baseline_path": str(args.baseline.relative_to(REPO_ROOT) if args.baseline.is_relative_to(REPO_ROOT) else args.baseline),
        "workloads": workloads,
        "diagnostics_quality": diagnostic_quality(),
    }
    report["baseline_comparisons"] = compare_to_baseline(report, load_baseline(args.baseline), args.tolerance)

    advisory_failures = [
        f"{name}.{metric}"
        for name, workload in workloads.items()
        for metric, status in workload["policy"]["status"].items()
        if status == "advisory-fail"
    ]
    if advisory_failures:
        print("ADVISORY comparison limit findings: " + ", ".join(advisory_failures), file=sys.stderr)

    payload = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.json_out:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(payload, encoding="utf-8")
    else:
        print(payload, end="")

    return 1 if args.enforce and advisory_failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
