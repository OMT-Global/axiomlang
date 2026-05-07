#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
AXIOMC_MANIFEST = REPO_ROOT / "stage1/Cargo.toml"
AXIOMC_BIN = REPO_ROOT / "stage1/target/debug/axiomc"
DEFAULT_EXAMPLES = (
    "stage1/examples/hello",
    "stage1/examples/modules",
    "stage1/examples/benchmarks",
)


@dataclass(frozen=True)
class Phase:
    name: str
    args: tuple[str, ...]


PHASES = (
    Phase("parser", ("fmt", "{project}", "--check")),
    Phase("check", ("check", "{project}", "--json")),
    Phase("build", ("build", "{project}", "--json")),
    Phase("run", ("run", "{project}")),
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Record stage1 parser/check/build/run timings for fixed examples as JSON."
    )
    parser.add_argument(
        "--rounds",
        type=int,
        default=3,
        help="number of timing rounds per phase; default: 3",
    )
    parser.add_argument(
        "--example",
        action="append",
        dest="examples",
        help="example project to benchmark; may be repeated",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=REPO_ROOT / ".axiom-build/reports/stage1-bench.json",
        help="JSON report path",
    )
    parser.add_argument(
        "--skip-axiomc-build",
        action="store_true",
        help="reuse stage1/target/debug/axiomc without building it first",
    )
    return parser.parse_args()


def median_ms(samples: list[float]) -> float:
    return float(statistics.median(samples))


def run_command(cmd: list[str]) -> tuple[float, str, str]:
    started = time.perf_counter()
    completed = subprocess.run(
        cmd,
        cwd=REPO_ROOT,
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
    return elapsed_ms, completed.stdout, completed.stderr


def build_axiomc() -> None:
    subprocess.run(
        ["cargo", "build", "--manifest-path", str(AXIOMC_MANIFEST), "-p", "axiomc"],
        cwd=REPO_ROOT,
        check=True,
    )


def command_for_phase(phase: Phase, project: Path) -> list[str]:
    project_arg = str(project.relative_to(REPO_ROOT))
    return [
        str(AXIOMC_BIN),
        *(arg.format(project=project_arg) for arg in phase.args),
    ]


def measure_phase(project: Path, phase: Phase, rounds: int) -> dict[str, object]:
    command = command_for_phase(phase, project)
    samples = [run_command(command)[0] for _ in range(rounds)]
    return {
        "command": command[1:],
        "samples_ms": [round(sample, 3) for sample in samples],
        "median_ms": round(median_ms(samples), 3),
        "min_ms": round(min(samples), 3),
        "max_ms": round(max(samples), 3),
    }


def benchmark_project(project: Path, rounds: int) -> dict[str, object]:
    if not project.exists():
        raise SystemExit(f"benchmark example does not exist: {project}")
    phases = {phase.name: measure_phase(project, phase, rounds) for phase in PHASES}
    return {
        "path": str(project.relative_to(REPO_ROOT)),
        "phases": phases,
    }


def write_report(report: dict[str, object], output: Path) -> None:
    output = output if output.is_absolute() else REPO_ROOT / output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")


def main() -> int:
    args = parse_args()
    if args.rounds <= 0:
        raise SystemExit("--rounds must be greater than zero")
    if not args.skip_axiomc_build:
        build_axiomc()

    examples = tuple(args.examples or DEFAULT_EXAMPLES)
    report = {
        "schema_version": "axiom.stage1.bench-harness.v1",
        "rounds": args.rounds,
        "examples": [
            benchmark_project(REPO_ROOT / example, args.rounds) for example in examples
        ],
    }
    write_report(report, args.output)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
