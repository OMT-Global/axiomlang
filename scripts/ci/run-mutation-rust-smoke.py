#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
AXIOMC_MANIFEST = REPO_ROOT / "stage1/Cargo.toml"


@dataclass(frozen=True)
class Mutant:
    name: str
    area: str
    file: Path
    find: str
    replace: str
    test_filter: str


MUTANTS = (
    Mutant(
        name="parser_for_loop_diagnostic",
        area="parser",
        file=REPO_ROOT / "stage1/crates/axiomc/src/syntax.rs",
        find="stage1 bootstrap does not support `for` loops yet",
        replace="stage1 bootstrap accepts `for` loops now",
        test_filter="parser_rejects_for_loops_explicitly",
    ),
    Mutant(
        name="hir_panic_argument_type",
        area="hir",
        file=REPO_ROOT / "stage1/crates/axiomc/src/hir.rs",
        find="panic expects a string argument",
        replace="panic accepts any argument",
        test_filter="panic_statement_requires_single_string_argument",
    ),
    Mutant(
        name="mir_equality_lowering",
        area="mir",
        file=REPO_ROOT / "stage1/crates/axiomc/src/mir.rs",
        find="hir::CompareOp::Eq => CompareOp::Eq,",
        replace="hir::CompareOp::Eq => CompareOp::Ne,",
        test_filter="build_project_emits_native_binary_with_local_consts",
    ),
    Mutant(
        name="codegen_runtime_error_report",
        area="codegen",
        file=REPO_ROOT / "stage1/crates/axiomc/src/codegen.rs",
        find='out.push_str("fn axiom_runtime_error(kind: &str, message: &str) -> ! {\\n");',
        replace='out.push_str("fn axiom_runtime_failure(kind: &str, message: &str) -> ! {\\n");',
        test_filter="render_rust_uses_structured_runtime_error_reporting",
    ),
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a bounded stage1 Rust mutation smoke profile and record survivors."
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=REPO_ROOT / ".axiom-build/reports/mutation-rust-smoke.json",
        help="JSON report path",
    )
    parser.add_argument(
        "--fail-on-survivors",
        action="store_true",
        help="exit non-zero when any mutant survives",
    )
    return parser.parse_args()


def apply_mutation(path: Path, find: str, replace: str) -> str:
    original = path.read_text()
    if find not in original:
        raise RuntimeError(f"mutation anchor not found in {path}: {find}")
    path.write_text(original.replace(find, replace, 1))
    return original


def run_test(test_filter: str) -> tuple[int, float, str, str]:
    started = time.perf_counter()
    completed = subprocess.run(
        [
            "cargo",
            "test",
            "--manifest-path",
            str(AXIOMC_MANIFEST),
            "-p",
            "axiomc",
            test_filter,
        ],
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    return completed.returncode, elapsed_ms, completed.stdout, completed.stderr


def run_mutant(mutant: Mutant) -> dict[str, object]:
    original = apply_mutation(mutant.file, mutant.find, mutant.replace)
    try:
        returncode, elapsed_ms, stdout, stderr = run_test(mutant.test_filter)
    finally:
        mutant.file.write_text(original)

    status = "survived" if returncode == 0 else "killed"
    return {
        "name": mutant.name,
        "area": mutant.area,
        "file": str(mutant.file.relative_to(REPO_ROOT)),
        "test_filter": mutant.test_filter,
        "status": status,
        "returncode": returncode,
        "duration_ms": round(elapsed_ms, 3),
        "stdout_tail": stdout[-2000:],
        "stderr_tail": stderr[-2000:],
    }


def write_report(report: dict[str, object], output: Path) -> None:
    output = output if output.is_absolute() else REPO_ROOT / output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")


def main() -> int:
    args = parse_args()
    results = [run_mutant(mutant) for mutant in MUTANTS]
    survivors = [result for result in results if result["status"] == "survived"]
    report = {
        "schema_version": "axiom.stage1.mutation-smoke.v1",
        "mutants": results,
        "summary": {
            "total": len(results),
            "killed": len(results) - len(survivors),
            "survived": len(survivors),
        },
        "survivors": [
            {
                "name": result["name"],
                "area": result["area"],
                "file": result["file"],
                "test_filter": result["test_filter"],
            }
            for result in survivors
        ],
    }
    write_report(report, args.output)
    print(json.dumps(report, indent=2, sort_keys=True))
    if survivors and args.fail_on_survivors:
        return 1
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as error:
        print(f"mutation smoke failed: {error}", file=sys.stderr)
        raise SystemExit(2)
