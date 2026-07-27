#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import os
import re
import signal
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Sequence

REPO_ROOT = Path(__file__).resolve().parents[2]
AXIOMC_MANIFEST = REPO_ROOT / "stage1/Cargo.toml"
SCRIPT_PATH = "scripts/ci/run-mutation-rust-smoke.py"
SCHEMA_VERSION = "axiom.stage1.mutation-smoke.v1"
DEFAULT_PER_MUTANT_BUDGET_SECONDS = 90.0
DEFAULT_TOTAL_BUDGET_SECONDS = 300.0
GOVERNING_ISSUE = {
    "number": 1463,
    "url": "https://github.com/OMT-Global/axiomlang/issues/1463",
}
ERROR_STATUSES = {
    "baseline_failure",
    "timeout",
    "budget_exhausted",
    "missing_anchor",
    "duplicate_anchor",
    "stale_anchor",
    "execution_error",
}


@dataclass(frozen=True)
class Mutant:
    name: str
    area: str
    file: Path
    find: str
    replace: str
    test_filter: str


@dataclass(frozen=True)
class TestOutcome:
    status: str
    returncode: int | None
    duration_ms: float
    stdout: str = ""
    stderr: str = ""


class MutationAnchorError(RuntimeError):
    def __init__(self, status: str, message: str) -> None:
        super().__init__(message)
        self.status = status


class MutationInterrupted(RuntimeError):
    pass


INTERRUPT_SIGNALS = {signal.SIGINT, signal.SIGTERM}


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
        find="""format!("panic expects a string argument, got {}", message.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&message, env)?;
            Ok(Stmt::Panic {""",
        replace="""format!("panic accepts any argument, got {}", message.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&message, env)?;
            Ok(Stmt::Panic {""",
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


def positive_seconds(value: str) -> float:
    parsed = float(value)
    if not math.isfinite(parsed) or parsed <= 0:
        raise argparse.ArgumentTypeError("budget must be a finite positive number of seconds")
    return parsed


def exact_commit(value: str) -> str:
    if re.fullmatch(r"[0-9a-f]{40}", value) is None:
        raise argparse.ArgumentTypeError(
            "expected head must be a 40-character lowercase Git commit"
        )
    return value


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
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
    parser.add_argument(
        "--per-mutant-budget-seconds",
        type=positive_seconds,
        default=DEFAULT_PER_MUTANT_BUDGET_SECONDS,
    )
    parser.add_argument(
        "--total-budget-seconds",
        type=positive_seconds,
        default=DEFAULT_TOTAL_BUDGET_SECONDS,
    )
    parser.add_argument(
        "--mutant",
        action="append",
        choices=[mutant.name for mutant in MUTANTS],
        help="run only the named mutant; may be repeated",
    )
    parser.add_argument(
        "--expected-head",
        type=exact_commit,
        help="require this exact 40-character lowercase Git commit",
    )
    return parser.parse_args(argv)


def git_head(root: Path = REPO_ROOT) -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "HEAD^{commit}"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    head = completed.stdout.strip()
    if completed.returncode != 0 or re.fullmatch(r"[0-9a-f]{40}", head) is None:
        raise RuntimeError(f"cannot resolve exact Git HEAD: {completed.stderr.strip()}")
    return head


def require_clean_tracked_tree(root: Path = REPO_ROOT) -> None:
    completed = subprocess.run(
        ["git", "status", "--porcelain=v1", "--untracked-files=no"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"cannot inspect tracked checkout state: {completed.stderr.strip()}"
        )
    if completed.stdout.strip():
        raise RuntimeError(
            "tracked checkout must match HEAD before mutation: "
            + ", ".join(line[3:] for line in completed.stdout.splitlines())
        )


def mutation_contents(path: Path, find: str, replace: str) -> tuple[str, str]:
    original = path.read_text(encoding="utf-8")
    occurrences = original.count(find)
    if occurrences == 0:
        status = "stale_anchor" if replace in original else "missing_anchor"
        raise MutationAnchorError(status, f"{status.replace('_', ' ')} in {path}: {find}")
    if occurrences != 1:
        raise MutationAnchorError(
            "duplicate_anchor", f"mutation anchor occurs {occurrences} times in {path}: {find}"
        )
    return original, original.replace(find, replace, 1)


def block_interrupts() -> set[signal.Signals] | None:
    if hasattr(signal, "pthread_sigmask"):
        return signal.pthread_sigmask(signal.SIG_BLOCK, INTERRUPT_SIGNALS)
    return None


def restore_interrupt_mask(previous_mask: set[signal.Signals] | None) -> None:
    if previous_mask is not None:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)


def prepare_child_process() -> None:
    if hasattr(signal, "pthread_sigmask"):
        signal.pthread_sigmask(signal.SIG_UNBLOCK, INTERRUPT_SIGNALS)


def restore_source(path: Path, original: str) -> None:
    previous_mask = block_interrupts()
    try:
        path.write_text(original, encoding="utf-8")
    finally:
        restore_interrupt_mask(previous_mask)


def process_group_exists(process_group_id: int) -> bool:
    try:
        os.killpg(process_group_id, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def terminate_process_group(
    process: subprocess.Popen[str],
    *,
    grace_seconds: float = 5.0,
) -> tuple[str, str]:
    previous_mask = block_interrupts()
    try:
        if os.name == "posix":
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        elif process.poll() is None:
            process.terminate()
        try:
            stdout, stderr = process.communicate(timeout=grace_seconds)
        except subprocess.TimeoutExpired:
            if os.name == "posix":
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            elif process.poll() is None:
                process.kill()
            stdout, stderr = process.communicate()
        if os.name == "posix" and process_group_exists(process.pid):
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        return stdout, stderr
    finally:
        restore_interrupt_mask(previous_mask)


def run_command(
    command: Sequence[str],
    *,
    cwd: Path,
    timeout_seconds: float,
) -> TestOutcome:
    started = time.monotonic()
    ownership_mask = block_interrupts()
    try:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            start_new_session=os.name == "posix",
            preexec_fn=prepare_child_process if os.name == "posix" else None,
        )
    except OSError as error:
        restore_interrupt_mask(ownership_mask)
        return TestOutcome(
            "execution_error",
            None,
            (time.monotonic() - started) * 1000.0,
            stderr=str(error),
        )
    except BaseException:
        restore_interrupt_mask(ownership_mask)
        raise
    try:
        previous_mask = ownership_mask
        ownership_mask = None
        restore_interrupt_mask(previous_mask)
        stdout, stderr = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        stdout, stderr = terminate_process_group(process)
        return TestOutcome(
            "timeout",
            None,
            (time.monotonic() - started) * 1000.0,
            stdout,
            stderr,
        )
    except BaseException:
        terminate_process_group(process)
        raise
    finally:
        restore_interrupt_mask(ownership_mask)
    return TestOutcome(
        "passed" if process.returncode == 0 else "failed",
        process.returncode,
        (time.monotonic() - started) * 1000.0,
        stdout,
        stderr,
    )


def focused_test_marker(stdout: str, test_filter: str, result: str) -> bool:
    pattern = (
        rf"^test (?:\S*::)?{re.escape(test_filter)} "
        rf"\.\.\. {re.escape(result)}$"
    )
    return re.search(pattern, stdout, flags=re.MULTILINE) is not None


def run_test(test_filter: str, timeout_seconds: float) -> TestOutcome:
    command = [
        "cargo",
        "test",
        "--manifest-path",
        str(AXIOMC_MANIFEST),
        "-p",
        "axiomc",
        "--locked",
        "--lib",
        test_filter,
    ]
    outcome = run_command(
        command,
        cwd=REPO_ROOT,
        timeout_seconds=timeout_seconds,
    )
    if outcome.status in {"timeout", "execution_error"}:
        return outcome
    if outcome.returncode == 0 and focused_test_marker(
        outcome.stdout, test_filter, "ok"
    ):
        return outcome
    if (
        outcome.returncode is not None
        and outcome.returncode > 0
        and focused_test_marker(outcome.stdout, test_filter, "FAILED")
    ):
        return outcome
    detail = (
        f"focused test {test_filter!r} did not report "
        + ("success" if outcome.returncode == 0 else "an assertion failure")
    )
    return TestOutcome(
        "execution_error",
        outcome.returncode,
        outcome.duration_ms,
        outcome.stdout,
        (outcome.stderr + "\n" + detail).strip(),
    )


def reproducer(
    mutant: Mutant,
    head_sha: str,
    per_mutant_budget: float,
    total_budget: float,
) -> str:
    return (
        f"python3 {SCRIPT_PATH} --mutant {mutant.name} "
        f"--expected-head {head_sha} "
        f"--per-mutant-budget-seconds {per_mutant_budget:g} "
        f"--total-budget-seconds {total_budget:g} --fail-on-survivors"
    )


def result_record(
    mutant: Mutant,
    head_sha: str,
    per_mutant_budget: float,
    total_budget: float,
    outcome: TestOutcome,
    *,
    phase: str,
    baseline_duration_ms: float = 0.0,
) -> dict[str, object]:
    return {
        "name": mutant.name,
        "area": mutant.area,
        "file": str(mutant.file.relative_to(REPO_ROOT)),
        "test_filter": mutant.test_filter,
        "status": outcome.status,
        "phase": phase,
        "returncode": outcome.returncode,
        "baseline_duration_ms": round(baseline_duration_ms, 3),
        "duration_ms": round(outcome.duration_ms, 3),
        "stdout_tail": outcome.stdout[-2000:],
        "stderr_tail": outcome.stderr[-2000:],
        "reproducer": reproducer(mutant, head_sha, per_mutant_budget, total_budget),
    }


def run_mutant(
    mutant: Mutant,
    *,
    head_sha: str,
    timeout_seconds: float,
    total_limited: bool,
    per_mutant_budget: float,
    total_budget: float,
    baseline_duration_ms: float,
    test_runner: Callable[[str, float], TestOutcome] = run_test,
) -> dict[str, object]:
    try:
        original, mutated = mutation_contents(
            mutant.file, mutant.find, mutant.replace
        )
    except MutationAnchorError as error:
        return result_record(
            mutant,
            head_sha,
            per_mutant_budget,
            total_budget,
            TestOutcome(error.status, None, 0.0, stderr=str(error)),
            phase="mutation",
            baseline_duration_ms=baseline_duration_ms,
        )
    try:
        mutant.file.write_text(mutated, encoding="utf-8")
        outcome = test_runner(mutant.test_filter, timeout_seconds)
    finally:
        restore_source(mutant.file, original)
    if outcome.status == "timeout" and total_limited:
        outcome = TestOutcome(
            "budget_exhausted",
            outcome.returncode,
            outcome.duration_ms,
            outcome.stdout,
            outcome.stderr,
        )
    elif outcome.status == "passed":
        outcome = TestOutcome(
            "survived",
            outcome.returncode,
            outcome.duration_ms,
            outcome.stdout,
            outcome.stderr,
        )
    elif outcome.status == "failed":
        outcome = TestOutcome(
            "killed",
            outcome.returncode,
            outcome.duration_ms,
            outcome.stdout,
            outcome.stderr,
        )
    return result_record(
        mutant,
        head_sha,
        per_mutant_budget,
        total_budget,
        outcome,
        phase="mutation",
        baseline_duration_ms=baseline_duration_ms,
    )


def build_report(
    *,
    head_sha: str,
    per_mutant_budget: float,
    total_budget: float,
    results: list[dict[str, object]],
    fail_on_survivors: bool,
    fatal_error: str | None = None,
) -> dict[str, object]:
    counts = {
        status: sum(result["status"] == status for result in results)
        for status in ("killed", "survived", *sorted(ERROR_STATUSES))
    }
    blocking = sum(counts[status] for status in ERROR_STATUSES)
    if fail_on_survivors:
        blocking += counts["survived"]
    survivors = [
        {
            key: result[key]
            for key in ("name", "area", "file", "test_filter", "reproducer")
        }
        for result in results
        if result["status"] == "survived"
    ]
    return {
        "schema_version": SCHEMA_VERSION,
        "governing_issue": GOVERNING_ISSUE,
        "head_sha": head_sha,
        "status": "failed" if fatal_error or blocking else "passed",
        "budgets": {
            "per_mutant_seconds": per_mutant_budget,
            "total_seconds": total_budget,
        },
        "fatal_error": fatal_error,
        "mutants": results,
        "summary": {"total": len(results), **counts, "blocking": blocking},
        "survivors": survivors,
    }


def run_profile(
    mutants: Sequence[Mutant],
    *,
    head_sha: str,
    per_mutant_budget: float,
    total_budget: float,
    fail_on_survivors: bool,
    test_runner: Callable[[str, float], TestOutcome] = run_test,
    clock: Callable[[], float] = time.monotonic,
) -> dict[str, object]:
    started = clock()
    results: list[dict[str, object]] = []
    for mutant in mutants:
        mutant_started = clock()
        remaining_total = total_budget - (mutant_started - started)
        if remaining_total <= 0:
            outcome = TestOutcome(
                "budget_exhausted", None, 0.0, stderr="total mutation budget exhausted"
            )
            results.append(
                result_record(
                    mutant,
                    head_sha,
                    per_mutant_budget,
                    total_budget,
                    outcome,
                    phase="baseline",
                )
            )
            continue
        baseline_timeout = min(per_mutant_budget, remaining_total)
        baseline = test_runner(mutant.test_filter, baseline_timeout)
        if baseline.status != "passed":
            if baseline.status == "failed":
                baseline = TestOutcome(
                    "baseline_failure",
                    baseline.returncode,
                    baseline.duration_ms,
                    baseline.stdout,
                    baseline.stderr,
                )
            elif (
                baseline.status == "timeout"
                and remaining_total <= per_mutant_budget
            ):
                baseline = TestOutcome(
                    "budget_exhausted",
                    baseline.returncode,
                    baseline.duration_ms,
                    baseline.stdout,
                    baseline.stderr,
                )
            results.append(
                result_record(
                    mutant,
                    head_sha,
                    per_mutant_budget,
                    total_budget,
                    baseline,
                    phase="baseline",
                    baseline_duration_ms=baseline.duration_ms,
                )
            )
            continue
        now = clock()
        remaining_total = total_budget - (now - started)
        remaining_mutant = per_mutant_budget - (now - mutant_started)
        if remaining_total <= 0 or remaining_mutant <= 0:
            results.append(
                result_record(
                    mutant,
                    head_sha,
                    per_mutant_budget,
                    total_budget,
                    TestOutcome(
                        "budget_exhausted",
                        None,
                        0.0,
                        stderr=(
                            "total mutation budget exhausted after baseline"
                            if remaining_total <= 0
                            else "per-mutant budget exhausted by baseline"
                        ),
                    ),
                    phase="mutation",
                    baseline_duration_ms=baseline.duration_ms,
                )
            )
            continue
        timeout = min(remaining_mutant, remaining_total)
        results.append(
            run_mutant(
                mutant,
                head_sha=head_sha,
                timeout_seconds=timeout,
                total_limited=remaining_total <= remaining_mutant,
                per_mutant_budget=per_mutant_budget,
                total_budget=total_budget,
                baseline_duration_ms=baseline.duration_ms,
                test_runner=test_runner,
            )
        )
    return build_report(
        head_sha=head_sha,
        per_mutant_budget=per_mutant_budget,
        total_budget=total_budget,
        results=results,
        fail_on_survivors=fail_on_survivors,
    )


def write_report(report: dict[str, object], output: Path) -> None:
    output = output if output.is_absolute() else REPO_ROOT / output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def report_exit_code(report: dict[str, object]) -> int:
    summary = report["summary"]
    if report["fatal_error"] or any(summary[status] for status in ERROR_STATUSES):
        return 2
    return 1 if summary["blocking"] else 0


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    observed_head = "0" * 40
    previous_handlers: dict[int, signal.Handlers] = {}

    def interrupt(signum: int, _frame: object) -> None:
        raise MutationInterrupted(f"mutation smoke interrupted by signal {signum}")

    for signum in (signal.SIGINT, signal.SIGTERM):
        previous_handlers[signum] = signal.getsignal(signum)
        signal.signal(signum, interrupt)
    try:
        try:
            observed_head = git_head()
            if args.expected_head is not None and args.expected_head != observed_head:
                raise RuntimeError(
                    f"expected exact head {args.expected_head}, observed {observed_head}"
                )
            require_clean_tracked_tree()
            selected = (
                [mutant for mutant in MUTANTS if mutant.name in set(args.mutant)]
                if args.mutant
                else list(MUTANTS)
            )
            report = run_profile(
                selected,
                head_sha=observed_head,
                per_mutant_budget=args.per_mutant_budget_seconds,
                total_budget=args.total_budget_seconds,
                fail_on_survivors=args.fail_on_survivors,
            )
            require_clean_tracked_tree()
        except RuntimeError as error:
            report = build_report(
                head_sha=observed_head,
                per_mutant_budget=args.per_mutant_budget_seconds,
                total_budget=args.total_budget_seconds,
                results=[],
                fail_on_survivors=args.fail_on_survivors,
                fatal_error=str(error),
            )
    finally:
        for signum, handler in previous_handlers.items():
            signal.signal(signum, handler)
    write_report(report, args.output)
    print(json.dumps(report, indent=2, sort_keys=True))
    return report_exit_code(report)


if __name__ == "__main__":
    raise SystemExit(main())
