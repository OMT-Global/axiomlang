#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
AXIOMC = ["cargo", "run", "--quiet", "--manifest-path", "stage1/Cargo.toml", "-p", "axiomc", "--"]
DEFAULT_EXAMPLES = ["hello", "stdlib_time", "stdlib_sync"]
BENCHMARK_PROJECT = Path("stage1/examples/benchmarks")


def run_timed(cmd: list[str]) -> tuple[float, str]:
    started = time.perf_counter()
    completed = subprocess.run(cmd, cwd=REPO_ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    if completed.returncode != 0:
        sys.stdout.write(completed.stdout)
        sys.stderr.write(completed.stderr)
        raise SystemExit(completed.returncode)
    return elapsed_ms, completed.stdout


def median(values: list[float]) -> float:
    return float(statistics.median(values))


def measure(example: str, rounds: int) -> dict[str, Any]:
    project = Path("stage1/examples") / example
    commands = {
        "parse": AXIOMC + ["parse", str(project), "--json"],
        "check": AXIOMC + ["check", str(project), "--json"],
        "build": AXIOMC + ["build", str(project), "--json", "--locked", "--offline"],
        "run": AXIOMC + ["run", str(project)],
    }
    timings: dict[str, Any] = {}
    for name, cmd in commands.items():
        samples: list[float] = []
        last_stdout = ""
        for _ in range(rounds):
            elapsed, stdout = run_timed(cmd)
            samples.append(elapsed)
            last_stdout = stdout
        timings[name] = {
            "samples_ms": [round(value, 3) for value in samples],
            "median_ms": round(median(samples), 3),
        }
        if name in {"parse", "check", "build"}:
            payload = json.loads(last_stdout)
            timings[name]["statement_count"] = payload.get("statement_count")
        if name == "build":
            payload = json.loads(last_stdout)
            timings[name]["compiler_duration_ms"] = payload.get("duration_ms")
            timings[name]["cache_hits"] = payload.get("cache_hits")
            timings[name]["cache_misses"] = payload.get("cache_misses")
    return {"project": str(project), "timings": timings}


def measure_benchmark_entrypoints(rounds: int) -> dict[str, Any]:
    """Execute checked-in benchmark entrypoints and retain their structured report."""
    command = AXIOMC + [
        "bench",
        str(BENCHMARK_PROJECT),
        "--warmup",
        "1",
        "--iterations",
        str(rounds),
        "--retries",
        "0",
        "--json",
    ]
    elapsed_ms, stdout = run_timed(command)
    try:
        payload = json.loads(stdout)
    except json.JSONDecodeError as exc:
        raise SystemExit(f"axiomc bench returned invalid JSON: {exc}") from exc

    if not isinstance(payload, dict):
        raise SystemExit("axiomc bench returned a non-object report")
    benches = payload.get("benches")
    if payload.get("schema_version") != "axiom.stage1.bench.v1":
        raise SystemExit("axiomc bench returned an unsupported report schema")
    if not isinstance(benches, list) or not benches:
        raise SystemExit("axiomc bench discovered no benchmark entrypoints")
    if payload.get("failed") != 0 or any(
        not isinstance(bench, dict) or bench.get("ok") is not True for bench in benches
    ):
        raise SystemExit("axiomc bench reported a failed benchmark entrypoint")

    return {
        "project": str(BENCHMARK_PROJECT),
        "command": command,
        "elapsed_ms": round(elapsed_ms, 3),
        "report": payload,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Record stage1 parser/check/build/run benchmark timings as JSON.")
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("stage1/benchmarks/generated/stage1-bench.json"),
        help=(
            "Path to write the benchmark report. Defaults to an ignored generated file; "
            "use make stage1-bench-update-baseline to refresh the tracked baseline."
        ),
    )
    parser.add_argument("examples", nargs="*", default=DEFAULT_EXAMPLES)
    args = parser.parse_args()
    if args.rounds < 1:
        raise SystemExit("--rounds must be >= 1")
    report = {
        "schema_version": "axiom.stage1.benchmark_harness.v1",
        "rounds": args.rounds,
        "commands": ["parse", "check", "build", "run"],
        "workloads": {example: measure(example, args.rounds) for example in args.examples},
        "benchmark_entrypoints": measure_benchmark_entrypoints(args.rounds),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
