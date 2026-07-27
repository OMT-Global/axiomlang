#!/usr/bin/env python3
"""Run the bounded, exact-head stage1 coverage quality gate."""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import shlex
import signal
import subprocess
import sys
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Sequence


SCRIPT_PATH = "scripts/ci/run-stage1-quality-gate.py"
SOURCE_PREFIX = "stage1/crates/axiomc/src"
MANIFEST_PATH = "stage1/Cargo.toml"
DEFAULT_POLICY = "stage1/quality/quality-policy-v1.json"
DEFAULT_LCOV_OUTPUT = ".axiom-build/reports/stage1-coverage.lcov"
DEFAULT_REPORT_OUTPUT = ".axiom-build/reports/stage1-quality-report.json"
REQUIRED_TOOL_VERSION = "0.8.5"
DEFAULT_BUDGET_SECONDS = 600.0
SKIPPED_TEST = "tests::check_properties_runs_property_only_tests"
EXACT_COMMIT_RE = re.compile(r"[0-9a-f]{40}")
GOVERNING_ISSUE = {
    "number": 1463,
    "url": "https://github.com/OMT-Global/axiomlang/issues/1463",
}
INTERRUPT_SIGNALS = {signal.SIGINT, signal.SIGTERM}
ALLOWED_LCOV_PREFIXES = {
    "BRDA",
    "BRF",
    "BRH",
    "DA",
    "FN",
    "FNDA",
    "FNF",
    "FNH",
    "LH",
    "LF",
}


@dataclass(frozen=True)
class ProcessOutcome:
    status: str
    returncode: int | None
    stdout: str
    stderr: str
    duration_seconds: float


@dataclass(frozen=True)
class LcovData:
    coverage: dict[str, dict[int, int]]
    normalized: str


class GateError(RuntimeError):
    def __init__(
        self,
        failure_class: str,
        code: str,
        message: str,
        *,
        path: str = SCRIPT_PATH,
        start_line: int = 1,
        end_line: int = 1,
        semantic_area: str = "quality.gate",
    ) -> None:
        super().__init__(message)
        self.failure_class = failure_class
        self.code = code
        self.path = path
        self.start_line = start_line
        self.end_line = end_line
        self.semantic_area = semantic_area


class QualityInterrupted(RuntimeError):
    pass


def positive_seconds(value: str) -> float:
    parsed = float(value)
    if not math.isfinite(parsed) or parsed <= 0:
        raise argparse.ArgumentTypeError(
            "budget must be a finite positive number of seconds"
        )
    return parsed


def exact_commit(value: str) -> str:
    if EXACT_COMMIT_RE.fullmatch(value) is None:
        raise argparse.ArgumentTypeError(
            "expected head must be a 40-character lowercase Git commit"
        )
    return value


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    parser.add_argument(
        "--expected-head",
        type=exact_commit,
        default=os.environ.get("AXIOM_QUALIFICATION_HEAD_SHA"),
        help="exact lowercase HEAD, or AXIOM_QUALIFICATION_HEAD_SHA",
    )
    parser.add_argument(
        "--comparison-head",
        type=exact_commit,
        default=os.environ.get("AXIOM_QUALIFICATION_BASE_SHA") or None,
        help="optional exact comparison commit, or AXIOM_QUALIFICATION_BASE_SHA",
    )
    parser.add_argument("--policy", type=Path, default=Path(DEFAULT_POLICY))
    parser.add_argument(
        "--lcov-output", type=Path, default=Path(DEFAULT_LCOV_OUTPUT)
    )
    parser.add_argument("--output", type=Path, default=Path(DEFAULT_REPORT_OUTPUT))
    parser.add_argument(
        "--budget-seconds",
        type=positive_seconds,
        default=DEFAULT_BUDGET_SECONDS,
    )
    return parser.parse_args(argv)


def is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
    except ValueError:
        return False
    return True


def resolve_repo_path(root: Path, value: Path, label: str) -> Path:
    candidate = value if value.is_absolute() else root / value
    resolved = candidate.resolve()
    if not is_within(resolved, root):
        raise GateError(
            "input",
            "path_escape",
            f"{label} escapes repository root: {value}",
        )
    return resolved


def repo_relative(root: Path, path: Path) -> str:
    return path.resolve().relative_to(root).as_posix()


def git(
    root: Path,
    arguments: Sequence[str],
    *,
    timeout_seconds: float = 30.0,
    allow_failure: bool = False,
) -> subprocess.CompletedProcess[str]:
    try:
        completed = subprocess.run(
            ["git", *arguments],
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
            timeout=timeout_seconds,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise GateError(
            "provenance", "git_unavailable", f"Git command failed: {error}"
        ) from error
    if completed.returncode != 0 and not allow_failure:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise GateError(
            "provenance",
            "git_failure",
            f"git {' '.join(arguments)} failed: {detail}",
        )
    return completed


def git_head(root: Path) -> str:
    head = git(root, ["rev-parse", "HEAD^{commit}"]).stdout.strip()
    if EXACT_COMMIT_RE.fullmatch(head) is None:
        raise GateError(
            "provenance", "invalid_head", f"Git returned a non-exact HEAD: {head!r}"
        )
    return head


def checkout_changes(
    root: Path, *, allowed_untracked: set[str] | None = None
) -> list[str]:
    completed = git(root, ["status", "--porcelain=v1", "--untracked-files=all"])
    allowed = allowed_untracked or set()
    changes: list[str] = []
    for line in completed.stdout.splitlines():
        if not line:
            continue
        if line.startswith("?? "):
            path = line[3:]
            if path.startswith('"'):
                try:
                    path = json.loads(path)
                except json.JSONDecodeError:
                    pass
            if path in allowed:
                continue
        changes.append(line)
    return changes


def require_clean_checkout(
    root: Path,
    phase: str,
    *,
    allowed_untracked: set[str] | None = None,
) -> None:
    changes = checkout_changes(root, allowed_untracked=allowed_untracked)
    if changes:
        raise GateError(
            "provenance",
            "dirty_checkout",
            f"checkout must match HEAD {phase}: {', '.join(changes)}",
        )


def require_untracked_output(root: Path, path: Path, label: str) -> None:
    relative = repo_relative(root, path)
    completed = git(
        root, ["ls-files", "--error-unmatch", "--", relative], allow_failure=True
    )
    if completed.returncode == 0:
        raise GateError(
            "input",
            "tracked_output_path",
            f"{label} must not overwrite a tracked file: {relative}",
        )


def clear_stale_lcov(path: Path) -> None:
    if not path.exists() and not path.is_symlink():
        return
    if path.is_symlink() or not path.is_file():
        raise GateError(
            "input",
            "invalid_lcov_output",
            f"LCOV output is not a regular file: {path}",
        )
    path.unlink()


def require_head_unchanged(root: Path, expected: str, phase: str) -> None:
    observed = git_head(root)
    if observed != expected:
        raise GateError(
            "provenance",
            "head_changed",
            f"HEAD moved from {expected} to {observed} {phase}",
        )


def require_commit_ancestor(root: Path, base: str, head: str) -> None:
    exists = git(
        root, ["cat-file", "-e", f"{base}^{{commit}}"], allow_failure=True
    )
    if exists.returncode != 0:
        raise GateError(
            "provenance",
            "comparison_unavailable",
            f"comparison head is unavailable: {base}",
        )
    ancestor = git(
        root, ["merge-base", "--is-ancestor", base, head], allow_failure=True
    )
    if ancestor.returncode == 1:
        raise GateError(
            "provenance",
            "comparison_not_ancestor",
            f"comparison head {base} is not an ancestor of HEAD {head}",
        )
    if ancestor.returncode != 0:
        raise GateError(
            "provenance",
            "git_failure",
            ancestor.stderr.strip() or "cannot prove comparison ancestry",
        )


def block_interrupts() -> set[signal.Signals] | None:
    if hasattr(signal, "pthread_sigmask"):
        return signal.pthread_sigmask(signal.SIG_BLOCK, INTERRUPT_SIGNALS)
    return None


def restore_interrupt_mask(previous: set[signal.Signals] | None) -> None:
    if previous is not None:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous)


def prepare_child_process() -> None:
    if hasattr(signal, "pthread_sigmask"):
        signal.pthread_sigmask(signal.SIG_UNBLOCK, INTERRUPT_SIGNALS)


def process_group_exists(process_group_id: int) -> bool:
    try:
        os.killpg(process_group_id, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def terminate_process_group(
    process: subprocess.Popen[str], grace_seconds: float = 1.0
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


def run_process(
    command: Sequence[str],
    *,
    cwd: Path,
    timeout_seconds: float,
    env: dict[str, str] | None = None,
) -> ProcessOutcome:
    started = time.monotonic()
    ownership_mask = block_interrupts()
    try:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            start_new_session=os.name == "posix",
            preexec_fn=prepare_child_process if os.name == "posix" else None,
        )
    except OSError as error:
        restore_interrupt_mask(ownership_mask)
        return ProcessOutcome(
            "execution_error",
            None,
            "",
            str(error),
            time.monotonic() - started,
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
        return ProcessOutcome(
            "timeout", None, stdout, stderr, time.monotonic() - started
        )
    except BaseException:
        terminate_process_group(process)
        raise
    finally:
        restore_interrupt_mask(ownership_mask)
    return ProcessOutcome(
        "passed" if process.returncode == 0 else "failed",
        process.returncode,
        stdout,
        stderr,
        time.monotonic() - started,
    )


def remaining_budget(deadline: float) -> float:
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise GateError(
            "infrastructure", "budget_exhausted", "quality gate budget exhausted"
        )
    return remaining


def exact_keys(
    value: Any, required: set[str], location: str
) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise GateError("input", "invalid_policy", f"{location} must be an object")
    keys = set(value)
    missing = sorted(required - keys)
    extra = sorted(keys - required)
    if missing or extra:
        detail: list[str] = []
        if missing:
            detail.append(f"missing {missing}")
        if extra:
            detail.append(f"unexpected {extra}")
        raise GateError(
            "input",
            "invalid_policy",
            f"{location} has invalid properties: {', '.join(detail)}",
        )
    return value


def require_int(value: Any, location: str, minimum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise GateError(
            "input",
            "invalid_policy",
            f"{location} must be an integer >= {minimum}",
        )
    return value


def validate_fraction(value: Any, location: str) -> dict[str, int]:
    item = exact_keys(value, {"numerator", "denominator"}, location)
    numerator = require_int(item["numerator"], f"{location}.numerator", 0)
    denominator = require_int(item["denominator"], f"{location}.denominator", 1)
    if numerator > denominator:
        raise GateError(
            "input",
            "invalid_policy",
            f"{location} cannot exceed 1",
        )
    return {"numerator": numerator, "denominator": denominator}


def load_policy(path: Path) -> dict[str, Any]:
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise GateError(
            "input", "invalid_policy", f"cannot read quality policy {path}: {error}"
        ) from error
    root = exact_keys(
        raw,
        {"schemaVersion", "globalLineCoverageFloor", "changedLineCoverageFloor"},
        "policy",
    )
    if root["schemaVersion"] != "axiom.quality_policy.v1":
        raise GateError(
            "input",
            "invalid_policy",
            "policy.schemaVersion must be axiom.quality_policy.v1",
        )
    global_floor = validate_fraction(
        root["globalLineCoverageFloor"],
        "policy.globalLineCoverageFloor",
    )
    changed_floor = validate_fraction(
        root["changedLineCoverageFloor"],
        "policy.changedLineCoverageFloor",
    )
    if global_floor != {"numerator": 3, "denominator": 5}:
        raise GateError(
            "input",
            "invalid_policy",
            "policy.globalLineCoverageFloor must be fixed at 3/5",
        )
    if changed_floor != {"numerator": 3, "denominator": 5}:
        raise GateError(
            "input",
            "invalid_policy",
            "policy.changedLineCoverageFloor must be fixed at 3/5",
        )
    return {
        "schemaVersion": root["schemaVersion"],
        "globalLineCoverageFloor": global_floor,
        "changedLineCoverageFloor": changed_floor,
    }


def parse_lcov(path: Path, root: Path) -> LcovData:
    try:
        raw = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise GateError(
            "infrastructure",
            "invalid_lcov",
            f"cannot read generated LCOV: {error}",
        ) from error
    if not raw:
        raise GateError(
            "infrastructure",
            "truncated_lcov",
            "generated LCOV is empty",
        )
    coverage: dict[str, dict[int, int]] = {}
    records: list[tuple[str, list[str]]] = []
    current_path: str | None = None
    current_lines: dict[int, int] = {}
    current_record: list[str] = []
    summary_tags: set[str] = set()
    summary_values: dict[str, int] = {}
    for index, line in enumerate(raw.splitlines(), start=1):
        if "\x00" in line or "\r" in line:
            raise GateError(
                "infrastructure",
                "invalid_lcov",
                f"invalid character in LCOV line {index}",
            )
        if line.startswith("SF:"):
            if current_path is not None:
                raise GateError(
                    "infrastructure",
                    "truncated_lcov",
                    f"nested SF at LCOV line {index}",
                )
            source = line[3:]
            if not source:
                raise GateError(
                    "infrastructure",
                    "invalid_lcov",
                    f"empty SF at LCOV line {index}",
                )
            candidate = Path(source)
            resolved = (
                candidate.resolve()
                if candidate.is_absolute()
                else (root / candidate).resolve()
            )
            if not is_within(resolved, root):
                raise GateError(
                    "infrastructure",
                    "lcov_path_escape",
                    f"LCOV source path escapes repository root: {source}",
                )
            relative = resolved.relative_to(root).as_posix()
            if relative in coverage:
                raise GateError(
                    "infrastructure",
                    "duplicate_lcov_record",
                    f"duplicate LCOV source record: {relative}",
                )
            current_path = relative
            current_lines = {}
            current_record = [f"SF:{relative}"]
            summary_tags = set()
            summary_values = {}
            continue
        if line == "end_of_record":
            if current_path is None:
                raise GateError(
                    "infrastructure",
                    "invalid_lcov",
                    f"end_of_record without SF at LCOV line {index}",
                )
            if (
                "LH" in summary_values
                and "LF" in summary_values
                and summary_values["LH"] > summary_values["LF"]
            ):
                raise GateError(
                    "infrastructure",
                    "invalid_lcov",
                    f"LCOV LH exceeds LF for {current_path}",
                )
            current_record.append(line)
            coverage[current_path] = current_lines
            records.append((current_path, current_record))
            current_path = None
            current_lines = {}
            current_record = []
            summary_tags = set()
            summary_values = {}
            continue
        if current_path is None:
            raise GateError(
                "infrastructure",
                "invalid_lcov",
                f"record outside SF block at LCOV line {index}: {line!r}",
            )
        tag = line.partition(":")[0]
        if tag not in ALLOWED_LCOV_PREFIXES:
            raise GateError(
                "infrastructure",
                "invalid_lcov",
                f"unsupported LCOV record at line {index}: {line!r}",
            )
        if tag == "DA":
            match = re.fullmatch(r"DA:([1-9][0-9]*),([0-9]+)(?:,([^,\r\n]+))?", line)
            if match is None:
                raise GateError(
                    "infrastructure",
                    "invalid_lcov",
                    f"malformed DA record at LCOV line {index}: {line!r}",
                )
            source_line = int(match.group(1))
            hits = int(match.group(2))
            if source_line in current_lines:
                raise GateError(
                    "infrastructure",
                    "duplicate_lcov_record",
                    f"duplicate DA for {current_path}:{source_line}",
                )
            current_lines[source_line] = hits
        elif tag in {"LF", "LH", "FNF", "FNH", "BRF", "BRH"}:
            if tag in summary_tags or re.fullmatch(rf"{tag}:[0-9]+", line) is None:
                raise GateError(
                    "infrastructure",
                    "invalid_lcov",
                    f"malformed or duplicate {tag} at LCOV line {index}",
                )
            summary_tags.add(tag)
            summary_values[tag] = int(line.partition(":")[2])
        elif tag == "FN":
            if re.fullmatch(r"FN:[1-9][0-9]*,.+", line) is None:
                raise GateError(
                    "infrastructure",
                    "invalid_lcov",
                    f"malformed FN record at LCOV line {index}",
                )
        elif tag == "FNDA":
            if re.fullmatch(r"FNDA:[0-9]+,.+", line) is None:
                raise GateError(
                    "infrastructure",
                    "invalid_lcov",
                    f"malformed FNDA record at LCOV line {index}",
                )
        elif tag == "BRDA":
            if (
                re.fullmatch(
                    r"BRDA:[1-9][0-9]*,[^,]+,[^,]+,(?:[0-9]+|-)", line
                )
                is None
            ):
                raise GateError(
                    "infrastructure",
                    "invalid_lcov",
                    f"malformed BRDA record at LCOV line {index}",
                )
        current_record.append(line)
    if current_path is not None:
        raise GateError(
            "infrastructure",
            "truncated_lcov",
            f"LCOV record lacks end_of_record: {current_path}",
        )
    if not records:
        raise GateError(
            "infrastructure", "invalid_lcov", "generated LCOV contains no records"
        )
    normalized = "\n".join(
        line for _, record in sorted(records) for line in record
    ) + "\n"
    return LcovData(coverage=coverage, normalized=normalized)


def atomic_write_text(path: Path, contents: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.parent / f".{path.name}.{uuid.uuid4().hex}.tmp"
    try:
        with temporary.open("x", encoding="utf-8", newline="\n") as handle:
            handle.write(contents)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def changed_added_lines(root: Path, base: str, head: str) -> dict[str, set[int]]:
    completed = git(
        root,
        [
            "diff",
            "--unified=0",
            "--no-ext-diff",
            "--no-renames",
            base,
            head,
            "--",
            SOURCE_PREFIX,
        ],
    )
    current_path: str | None = None
    changed: dict[str, set[int]] = {}
    hunk_re = re.compile(
        r"^@@ -[0-9]+(?:,[0-9]+)? \+([0-9]+)(?:,([0-9]+))? @@"
    )
    for line in completed.stdout.splitlines():
        if line.startswith("+++ "):
            value = line[4:]
            if value == "/dev/null":
                current_path = None
            elif value.startswith("b/") and "\t" not in value:
                current_path = value[2:]
            else:
                raise GateError(
                    "provenance",
                    "invalid_git_diff",
                    f"unsupported diff path: {value!r}",
                )
        elif line.startswith("@@ "):
            match = hunk_re.match(line)
            if match is None:
                raise GateError(
                    "provenance",
                    "invalid_git_diff",
                    f"malformed diff hunk: {line!r}",
                )
            if current_path is not None and current_path.endswith(".rs"):
                start = int(match.group(1))
                count = int(match.group(2) or "1")
                changed.setdefault(current_path, set()).update(
                    range(start, start + count)
                )
    return changed


def semantic_area(path: str) -> str:
    relative = path.removeprefix(f"{SOURCE_PREFIX}/")
    first = relative.split("/", 1)[0]
    stem = Path(first).stem
    if stem in {"syntax", "diagnostics", "diagnostic_catalog"}:
        return "compiler.syntax"
    if stem in {
        "hir",
        "borrowck",
        "capabilities",
        "definitions",
        "expressions",
        "generics",
        "model",
        "ownership",
        "properties",
        "reachability",
        "signatures",
        "symbols",
        "types",
    }:
        return "compiler.hir"
    if stem == "mir":
        return "compiler.mir"
    if stem in {"codegen", "cranelift_backend"}:
        return "compiler.backend"
    if stem in {"lsp", "dap"}:
        return "compiler.services"
    if stem in {"project", "manifest", "registry", "lockfile"}:
        return "compiler.package_graph"
    return "compiler.stage1"


def reproducer(head: str, comparison: str | None, budget: float) -> str:
    command = [
        "python3",
        SCRIPT_PATH,
        "--expected-head",
        head,
    ]
    if comparison is not None:
        command.extend(["--comparison-head", comparison])
    command.extend(["--budget-seconds", f"{budget:g}"])
    return shlex.join(command)


def finding(
    *,
    code: str,
    message: str,
    reproducer_command: str,
    semantic_area_name: str,
    path: str,
    start_line: int,
    end_line: int,
) -> dict[str, Any]:
    return {
        "code": code,
        "message": message,
        "semanticArea": semantic_area_name,
        "path": path,
        "startLine": max(1, start_line),
        "endLine": max(max(1, start_line), end_line),
        "reproducer": reproducer_command,
        "governingIssue": 1463,
    }


def empty_report(
    *,
    head: str,
    base: str | None,
    target: str | None,
    observed_tool: str | None,
    budget: float,
    lcov_path: str | None,
    report_path: str,
) -> dict[str, Any]:
    return {
        "schemaVersion": "axiom.quality_report.v1",
        "headSha": head,
        "baseSha": base,
        "target": target,
        "tool": {
            "name": "cargo-llvm-cov",
            "requiredVersion": REQUIRED_TOOL_VERSION,
            "observedVersion": observed_tool,
        },
        "profile": {
            "manifest": MANIFEST_PATH,
            "package": "axiomc",
            "targets": ["lib", "bin:axiomc"],
            "locked": True,
            "testThreads": 1,
            "skippedTests": [SKIPPED_TEST],
            "budgetSeconds": budget,
        },
        "status": "failed",
        "failureClass": "infrastructure",
        "coverage": {
            "global": {
                "status": "not_evaluated",
                "coveredLines": 0,
                "totalLines": 0,
                "floor": None,
            },
            "changed": {
                "status": "not_evaluated",
                "coveredLines": 0,
                "totalLines": 0,
                "floor": None,
            },
        },
        "findings": [],
        "artifacts": {"lcov": lcov_path, "report": report_path},
        "reproducer": reproducer(head, base, budget),
        "governingIssue": GOVERNING_ISSUE,
    }


def fraction_passes(covered: int, total: int, floor: dict[str, int]) -> bool:
    return covered * floor["denominator"] >= total * floor["numerator"]


def evaluate_quality(
    *,
    root: Path,
    head: str,
    comparison: str | None,
    policy: dict[str, Any],
    lcov: LcovData,
    report: dict[str, Any],
) -> None:
    global_floor = policy["globalLineCoverageFloor"]
    changed_floor = policy["changedLineCoverageFloor"]
    source_coverage = {
        path: lines
        for path, lines in lcov.coverage.items()
        if path.startswith(f"{SOURCE_PREFIX}/") and path.endswith(".rs")
    }
    global_total = sum(len(lines) for lines in source_coverage.values())
    global_covered = sum(
        sum(hits > 0 for hits in lines.values())
        for lines in source_coverage.values()
    )
    global_ok = global_total > 0 and fraction_passes(
        global_covered, global_total, global_floor
    )
    report["coverage"]["global"] = {
        "status": "passed" if global_ok else "failed",
        "coveredLines": global_covered,
        "totalLines": global_total,
        "floor": global_floor,
    }
    if not global_ok:
        report["findings"].append(
            finding(
                code="global_coverage_regression",
                message=(
                    f"global line coverage {global_covered}/{global_total} "
                    f"is below {global_floor['numerator']}/{global_floor['denominator']}"
                ),
                reproducer_command=report["reproducer"],
                semantic_area_name="compiler.stage1",
                path=SOURCE_PREFIX,
                start_line=1,
                end_line=1,
            )
        )

    changed_executable: dict[str, list[int]] = {}
    if comparison is None:
        changed_total = 0
        changed_covered = 0
        changed_status = "not_applicable"
        changed_ok = True
    else:
        changed_lines = changed_added_lines(root, comparison, head)
        for path, lines in sorted(changed_lines.items()):
            executable = sorted(lines.intersection(source_coverage.get(path, {})))
            if executable:
                changed_executable[path] = executable
        changed_total = sum(len(lines) for lines in changed_executable.values())
        changed_covered = sum(
            sum(source_coverage[path][line] > 0 for line in lines)
            for path, lines in changed_executable.items()
        )
        if changed_total == 0:
            changed_status = "not_applicable"
            changed_ok = True
        else:
            changed_ok = fraction_passes(
                changed_covered, changed_total, changed_floor
            )
            changed_status = "passed" if changed_ok else "failed"
    report["coverage"]["changed"] = {
        "status": changed_status,
        "coveredLines": changed_covered,
        "totalLines": changed_total,
        "floor": changed_floor,
    }
    if not changed_ok:
        for path, lines in sorted(changed_executable.items()):
            file_covered = sum(source_coverage[path][line] > 0 for line in lines)
            report["findings"].append(
                finding(
                    code="changed_coverage_regression",
                    message=(
                        f"changed executable lines contribute {file_covered}/{len(lines)} "
                        f"coverage; aggregate is {changed_covered}/{changed_total}, below "
                        f"{changed_floor['numerator']}/{changed_floor['denominator']}"
                    ),
                    reproducer_command=report["reproducer"],
                    semantic_area_name=semantic_area(path),
                    path=path,
                    start_line=min(lines),
                    end_line=max(lines),
                )
            )

    quality_failed = not global_ok or not changed_ok
    report["status"] = "failed" if quality_failed else "passed"
    report["failureClass"] = "quality" if quality_failed else None


def validate_report_semantics(report: dict[str, Any]) -> None:
    coverage = report.get("coverage")
    if not isinstance(coverage, dict):
        raise GateError(
            "infrastructure",
            "invalid_report_semantics",
            "quality report coverage must be an object",
        )
    for name in ("global", "changed"):
        result = coverage.get(name)
        if not isinstance(result, dict):
            raise GateError(
                "infrastructure",
                "invalid_report_semantics",
                f"quality report coverage.{name} must be an object",
            )
        covered = result.get("coveredLines")
        total = result.get("totalLines")
        if (
            isinstance(covered, bool)
            or not isinstance(covered, int)
            or isinstance(total, bool)
            or not isinstance(total, int)
            or covered < 0
            or total < 0
            or covered > total
        ):
            raise GateError(
                "infrastructure",
                "invalid_report_semantics",
                f"quality report coverage.{name} has impossible line counts",
            )
        if result.get("status") in {"not_applicable", "not_evaluated"} and (
            covered != 0 or total != 0
        ):
            raise GateError(
                "infrastructure",
                "invalid_report_semantics",
                f"quality report coverage.{name} has counts for an unevaluated state",
            )
        if result.get("status") in {"passed", "failed"}:
            floor = result.get("floor")
            if (
                total == 0
                or not isinstance(floor, dict)
                or set(floor) != {"numerator", "denominator"}
                or isinstance(floor.get("numerator"), bool)
                or not isinstance(floor.get("numerator"), int)
                or isinstance(floor.get("denominator"), bool)
                or not isinstance(floor.get("denominator"), int)
                or floor["numerator"] < 0
                or floor["denominator"] <= 0
                or floor["numerator"] > floor["denominator"]
            ):
                raise GateError(
                    "infrastructure",
                    "invalid_report_semantics",
                    f"quality report coverage.{name} has an invalid evaluated floor",
                )
            passes = fraction_passes(covered, total, floor)
            if (result["status"] == "passed") != passes:
                raise GateError(
                    "infrastructure",
                    "invalid_report_semantics",
                    f"quality report coverage.{name} status contradicts its floor",
                )
    findings = report.get("findings")
    if not isinstance(findings, list):
        raise GateError(
            "infrastructure",
            "invalid_report_semantics",
            "quality report findings must be an array",
        )
    for item in findings:
        if (
            not isinstance(item, dict)
            or not isinstance(item.get("startLine"), int)
            or not isinstance(item.get("endLine"), int)
            or item["startLine"] > item["endLine"]
        ):
            raise GateError(
                "infrastructure",
                "invalid_report_semantics",
                "quality report finding has a reversed or invalid source span",
            )
    if report.get("baseSha") is None and coverage["changed"].get("status") not in {
        "not_applicable",
        "not_evaluated",
    }:
        raise GateError(
            "infrastructure",
            "invalid_report_semantics",
            "changed coverage cannot be evaluated without a comparison head",
        )
    if report.get("failureClass") == "quality":
        if not (
            coverage["global"].get("status") == "failed"
            or coverage["changed"].get("status") == "failed"
        ):
            raise GateError(
                "infrastructure",
                "invalid_report_semantics",
                "quality failure has no failed coverage result",
            )
        if not isinstance(report.get("artifacts", {}).get("lcov"), str):
            raise GateError(
                "infrastructure",
                "invalid_report_semantics",
                "quality failure must retain its LCOV artifact",
            )
    if report.get("status") == "passed":
        if (
            report.get("failureClass") is not None
            or findings
            or not isinstance(report.get("artifacts", {}).get("lcov"), str)
            or coverage["global"].get("status") != "passed"
            or coverage["changed"].get("status")
            not in {"passed", "not_applicable"}
        ):
            raise GateError(
                "infrastructure",
                "invalid_report_semantics",
                "passed quality report has contradictory state",
            )
    elif (
        report.get("status") != "failed"
        or report.get("failureClass") is None
        or not findings
    ):
        raise GateError(
            "infrastructure",
            "invalid_report_semantics",
            "failed quality report must have a failure class and finding",
        )


ProcessRunner = Callable[..., ProcessOutcome]


def execute_gate(
    args: argparse.Namespace, *, process_runner: ProcessRunner = run_process
) -> int:
    root = args.repo_root.resolve()
    if not root.is_dir():
        print(f"error: repository root does not exist: {root}", file=sys.stderr)
        return 2
    try:
        policy_path = resolve_repo_path(root, args.policy, "quality policy")
        lcov_output = resolve_repo_path(root, args.lcov_output, "LCOV output")
        report_output = resolve_repo_path(root, args.output, "report output")
        if lcov_output == report_output:
            raise GateError(
                "input",
                "output_collision",
                "LCOV and report output paths must be distinct",
            )
        head = git_head(root)
    except GateError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    initial_comparison = (
        args.comparison_head
        if isinstance(args.comparison_head, str)
        and EXACT_COMMIT_RE.fullmatch(args.comparison_head) is not None
        else None
    )
    report = empty_report(
        head=head,
        base=initial_comparison,
        target=None,
        observed_tool=None,
        budget=args.budget_seconds,
        lcov_path=None,
        report_path=repo_relative(root, report_output),
    )
    deadline = time.monotonic() + args.budget_seconds
    failure: GateError | None = None
    report_write_error: OSError | None = None
    lcov_was_published = False
    report_output_safe = False
    temporary_lcov: Path | None = None
    publication_mask: set[signal.Signals] | None = None
    publication_mask_acquired = False
    allowed_outputs = {
        repo_relative(root, lcov_output),
        repo_relative(root, report_output),
    }
    previous_handlers: dict[int, signal.Handlers] = {}

    def interrupt(signum: int, _frame: object) -> None:
        raise QualityInterrupted(f"quality gate interrupted by signal {signum}")

    def record_error(error: GateError) -> None:
        nonlocal failure
        failure = error
        report["status"] = "failed"
        report["failureClass"] = error.failure_class
        if not any(item["code"] == error.code for item in report["findings"]):
            report["findings"].append(
                finding(
                    code=error.code,
                    message=str(error),
                    reproducer_command=report["reproducer"],
                    semantic_area_name=error.semantic_area,
                    path=error.path,
                    start_line=error.start_line,
                    end_line=error.end_line,
                )
            )

    for signum in (signal.SIGINT, signal.SIGTERM):
        previous_handlers[signum] = signal.getsignal(signum)
        signal.signal(signum, interrupt)
    try:
        require_untracked_output(root, lcov_output, "LCOV output")
        require_untracked_output(root, report_output, "report output")
        report_output_safe = True
        clear_stale_lcov(lcov_output)

        expected = args.expected_head
        if expected is None:
            raise GateError(
                "provenance",
                "expected_head_required",
                "--expected-head or AXIOM_QUALIFICATION_HEAD_SHA is required",
            )
        if EXACT_COMMIT_RE.fullmatch(expected) is None:
            raise GateError(
                "provenance",
                "invalid_expected_head",
                "expected head must be an exact lowercase commit",
            )
        if head != expected:
            raise GateError(
                "provenance",
                "wrong_head",
                f"HEAD {head} does not match expected head {expected}",
            )
        comparison = args.comparison_head
        if (
            comparison is not None
            and (
                not isinstance(comparison, str)
                or EXACT_COMMIT_RE.fullmatch(comparison) is None
            )
        ):
            raise GateError(
                "provenance",
                "invalid_comparison_head",
                "comparison head must be an exact lowercase commit",
            )
        require_clean_checkout(
            root,
            "before quality measurement",
            allowed_untracked=allowed_outputs,
        )
        policy = load_policy(policy_path)
        report["coverage"]["global"]["floor"] = policy[
            "globalLineCoverageFloor"
        ]
        report["coverage"]["changed"]["floor"] = policy[
            "changedLineCoverageFloor"
        ]
        if comparison is not None:
            require_commit_ancestor(root, comparison, head)

        version_outcome = process_runner(
            ["cargo", "llvm-cov", "--version"],
            cwd=root,
            timeout_seconds=remaining_budget(deadline),
        )
        if version_outcome.status == "timeout":
            raise GateError(
                "infrastructure",
                "tool_version_timeout",
                "cargo-llvm-cov version check timed out",
            )
        if version_outcome.status != "passed":
            detail = version_outcome.stderr.strip() or version_outcome.stdout.strip()
            raise GateError(
                "infrastructure",
                "tool_missing",
                f"cargo-llvm-cov is unavailable: {detail}",
            )
        version_match = re.fullmatch(
            r"cargo-llvm-cov ([0-9]+\.[0-9]+\.[0-9]+)",
            version_outcome.stdout.strip(),
        )
        observed_version = version_match.group(1) if version_match else None
        if observed_version != REQUIRED_TOOL_VERSION:
            raise GateError(
                "infrastructure",
                "tool_version_mismatch",
                f"cargo-llvm-cov {REQUIRED_TOOL_VERSION} required; observed "
                f"{version_outcome.stdout.strip()!r}",
            )
        report["tool"]["observedVersion"] = observed_version

        target_outcome = process_runner(
            ["rustc", "-vV"],
            cwd=root,
            timeout_seconds=remaining_budget(deadline),
        )
        if target_outcome.status != "passed":
            code = (
                "target_probe_timeout"
                if target_outcome.status == "timeout"
                else "target_probe_failed"
            )
            raise GateError(
                "infrastructure",
                code,
                target_outcome.stderr.strip() or "rustc target probe failed",
            )
        hosts = [
            line.removeprefix("host: ").strip()
            for line in target_outcome.stdout.splitlines()
            if line.startswith("host: ")
        ]
        if len(hosts) != 1 or not hosts[0]:
            raise GateError(
                "infrastructure",
                "target_probe_failed",
                "rustc -vV did not report exactly one host target",
            )
        report["target"] = hosts[0]

        lcov_output.parent.mkdir(parents=True, exist_ok=True)
        temporary_lcov = (
            lcov_output.parent
            / f".{lcov_output.name}.{uuid.uuid4().hex}.coverage.tmp"
        )
        if temporary_lcov.exists():
            raise GateError(
                "infrastructure",
                "stale_temporary_output",
                f"temporary LCOV output already exists: {temporary_lcov}",
            )
        command = [
            "cargo",
            "llvm-cov",
            "--manifest-path",
            str(root / MANIFEST_PATH),
            "-p",
            "axiomc",
            "--lib",
            "--bin",
            "axiomc",
            "--locked",
            "--ignore-filename-regex",
            "rustlib/src/rust/",
            "--lcov",
            "--output-path",
            str(temporary_lcov),
            "--",
            "--test-threads=1",
            "--skip",
            SKIPPED_TEST,
        ]
        coverage_env = os.environ.copy()
        coverage_env["RUST_MIN_STACK"] = "8388608"
        try:
            coverage_outcome = process_runner(
                command,
                cwd=root,
                timeout_seconds=remaining_budget(deadline),
                env=coverage_env,
            )
            if coverage_outcome.status == "timeout":
                raise GateError(
                    "infrastructure",
                    "coverage_timeout",
                    "cargo-llvm-cov exceeded the total quality gate budget",
                )
            if coverage_outcome.status != "passed":
                detail = (
                    coverage_outcome.stderr.strip()
                    or coverage_outcome.stdout.strip()
                    or f"exit {coverage_outcome.returncode}"
                )
                raise GateError(
                    "infrastructure",
                    "coverage_command_failed",
                    f"cargo-llvm-cov failed: {detail[-2000:]}",
                )
            if (
                not temporary_lcov.is_file()
                or temporary_lcov.is_symlink()
                or temporary_lcov.stat().st_size == 0
            ):
                raise GateError(
                    "infrastructure",
                    "coverage_output_missing",
                    "cargo-llvm-cov did not create a fresh non-empty LCOV output",
                )
            parsed_lcov = parse_lcov(temporary_lcov, root)
            measurement_outputs = allowed_outputs | {
                repo_relative(root, temporary_lcov)
            }
            require_head_unchanged(root, head, "after quality measurement")
            require_clean_checkout(
                root,
                "after quality measurement",
                allowed_untracked=measurement_outputs,
            )
            evaluate_quality(
                root=root,
                head=head,
                comparison=comparison,
                policy=policy,
                lcov=parsed_lcov,
                report=report,
            )
            require_head_unchanged(root, head, "before publishing LCOV")
            require_clean_checkout(
                root,
                "before publishing LCOV",
                allowed_untracked=measurement_outputs,
            )
            publication_mask = block_interrupts()
            publication_mask_acquired = True
            atomic_write_text(lcov_output, parsed_lcov.normalized)
            lcov_was_published = True
            report["artifacts"]["lcov"] = repo_relative(root, lcov_output)
        finally:
            if temporary_lcov is not None:
                try:
                    temporary_lcov.unlink()
                except FileNotFoundError:
                    pass
    except GateError as error:
        record_error(error)
    except QualityInterrupted as error:
        record_error(
            GateError(
                "infrastructure",
                "quality_gate_cancelled",
                str(error),
            )
        )
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception as error:
        record_error(
            GateError(
                "infrastructure",
                "unexpected_gate_error",
                f"unexpected quality gate error: {type(error).__name__}: {error}",
            )
        )
    finally:
        if not publication_mask_acquired:
            publication_mask = block_interrupts()
        try:
            try:
                require_head_unchanged(
                    root, head, "before publishing the report"
                )
                require_clean_checkout(
                    root,
                    "before publishing the report",
                    allowed_untracked=allowed_outputs,
                )
            except GateError as error:
                record_error(error)

            if report["failureClass"] not in {None, "quality"}:
                if lcov_was_published:
                    try:
                        lcov_output.unlink()
                    except FileNotFoundError:
                        pass
                lcov_was_published = False
            if not lcov_was_published:
                report["artifacts"]["lcov"] = None
            if report_output_safe:
                try:
                    validate_report_semantics(report)
                    atomic_write_text(
                        report_output,
                        json.dumps(report, indent=2, sort_keys=True) + "\n",
                    )
                except GateError as error:
                    global_floor = (
                        report.get("coverage", {}).get("global", {}).get("floor")
                    )
                    changed_floor = (
                        report.get("coverage", {})
                        .get("changed", {})
                        .get("floor")
                    )
                    report["coverage"] = {
                        "global": {
                            "status": "not_evaluated",
                            "coveredLines": 0,
                            "totalLines": 0,
                            "floor": global_floor,
                        },
                        "changed": {
                            "status": "not_evaluated",
                            "coveredLines": 0,
                            "totalLines": 0,
                            "floor": changed_floor,
                        },
                    }
                    report["findings"] = []
                    record_error(error)
                    validate_report_semantics(report)
                    atomic_write_text(
                        report_output,
                        json.dumps(report, indent=2, sort_keys=True) + "\n",
                    )
                except OSError as error:
                    report_write_error = error
        finally:
            try:
                for signum, handler in previous_handlers.items():
                    signal.signal(signum, handler)
            finally:
                restore_interrupt_mask(publication_mask)
    if report_write_error is not None:
        print(
            f"error: cannot write quality report: {report_write_error}",
            file=sys.stderr,
        )
        return 2
    if failure is not None:
        print(f"error: {failure}", file=sys.stderr)
    elif report["status"] == "failed":
        print(
            f"error: quality gate failed with {len(report['findings'])} finding(s)",
            file=sys.stderr,
        )
    return 0 if report["status"] == "passed" else 1


def main(argv: Sequence[str] | None = None) -> int:
    return execute_gate(parse_args(argv))


if __name__ == "__main__":
    raise SystemExit(main())
