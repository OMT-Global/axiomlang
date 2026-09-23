#!/usr/bin/env python3
"""Run a deterministic, bounded parser/recovery fuzz smoke profile."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence


SCHEMA_VERSION = "axiom.stage1.parser_fuzz.v1"
DEFAULT_CORPUS = Path("stage1/fuzz/parser-corpus")
DEFAULT_CASES = 64
DEFAULT_TIMEOUT_MS = 2000
DEFAULT_BUDGET_SECONDS = 90.0
EXACT_COMMIT_RE = re.compile(r"[0-9a-f]{40}")
FAILURE_STATUSES = {"timeout", "crash", "invalid_output"}
MUTATION_INSERTIONS = (
    "\n",
    " ",
    "()",
    "{}",
    "[]",
    "\"unterminated",
    "match ",
    "if ",
    "while ",
    "property(\"fuzz\", ",
    "import \"std/testing.ax\"\n",
)


@dataclass(frozen=True)
class CorpusEntry:
    name: str
    source: str


@dataclass(frozen=True)
class Execution:
    status: str
    exit_code: int | None
    duration_ms: int
    stderr: str
    payload: dict[str, Any] | None


class FuzzInputError(ValueError):
    pass


class DeterministicRng:
    def __init__(self, seed: int) -> None:
        self.state = seed & ((1 << 64) - 1)

    def next_u64(self) -> int:
        self.state = (self.state + 0x9E3779B97F4A7C15) & ((1 << 64) - 1)
        value = self.state
        value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & ((1 << 64) - 1)
        value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & ((1 << 64) - 1)
        return value ^ (value >> 31)

    def below(self, bound: int) -> int:
        if bound <= 0:
            raise ValueError("random bound must be positive")
        return self.next_u64() % bound


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--axiomc", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, default=DEFAULT_CORPUS)
    parser.add_argument("--seed", type=int, default=1463)
    parser.add_argument("--cases", type=positive_int, default=DEFAULT_CASES)
    parser.add_argument("--timeout-ms", type=positive_int, default=DEFAULT_TIMEOUT_MS)
    parser.add_argument(
        "--budget-seconds", type=positive_seconds, default=DEFAULT_BUDGET_SECONDS
    )
    parser.add_argument("--expected-head", type=exact_commit, default=None)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(argv)


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("value must be positive")
    return parsed


def positive_seconds(value: str) -> float:
    parsed = float(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("budget must be positive")
    return parsed


def exact_commit(value: str) -> str:
    if EXACT_COMMIT_RE.fullmatch(value) is None:
        raise argparse.ArgumentTypeError("expected a 40-character lowercase commit")
    return value


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def git_head(root: Path) -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        raise FuzzInputError(f"cannot resolve checkout HEAD: {result.stderr.strip()}")
    head = result.stdout.strip()
    if EXACT_COMMIT_RE.fullmatch(head) is None:
        raise FuzzInputError(f"checkout HEAD is not an exact commit: {head!r}")
    return head


def load_corpus(root: Path, corpus_path: Path) -> tuple[list[CorpusEntry], str]:
    directory = corpus_path if corpus_path.is_absolute() else root / corpus_path
    if not directory.is_dir():
        raise FuzzInputError(f"parser corpus directory is missing: {directory}")
    entries: list[CorpusEntry] = []
    digest = hashlib.sha256()
    for path in sorted(directory.glob("*.ax")):
        source = path.read_text(encoding="utf-8")
        if not source:
            raise FuzzInputError(f"parser corpus entry is empty: {path}")
        name = path.relative_to(directory).as_posix()
        entries.append(CorpusEntry(name, source))
        digest.update(name.encode("utf-8"))
        digest.update(b"\0")
        digest.update(source.encode("utf-8"))
        digest.update(b"\0")
    if not entries:
        raise FuzzInputError(f"parser corpus has no .ax entries: {directory}")
    return entries, digest.hexdigest()


def mutate(entry: CorpusEntry, rng: DeterministicRng) -> str:
    source = entry.source
    operation = rng.below(6)
    if operation == 0:
        return source
    if operation == 1:
        insertion = MUTATION_INSERTIONS[rng.below(len(MUTATION_INSERTIONS))]
        position = rng.below(len(source) + 1)
        source = source[:position] + insertion + source[position:]
    elif operation == 2 and source:
        start = rng.below(len(source))
        width = min(1 + rng.below(12), len(source) - start)
        source = source[:start] + source[start + width :]
    elif operation == 3 and source:
        start = rng.below(len(source))
        width = min(1 + rng.below(16), len(source) - start)
        fragment = source[start : start + width]
        source = source[:start] + fragment + fragment + source[start + width :]
    elif operation == 4:
        source = "(" * (1 + rng.below(3)) + source + ")" * rng.below(2)
    else:
        source = source + "\n" + ("}" * (1 + rng.below(3)))
    return source[:8192]


def run_parser(
    axiomc: Path, source: str, timeout_ms: int, root: Path
) -> Execution:
    with tempfile.TemporaryDirectory(prefix="axiom-parser-fuzz-") as directory:
        project_path = Path(directory) / "project"
        source_path = project_path / "src/main.ax"
        source_path.parent.mkdir(parents=True)
        source_path.write_text(source, encoding="utf-8")
        (project_path / "axiom.toml").write_text(
            "[package]\n"
            "name = \"parser-fuzz-case\"\n"
            "version = \"0.1.0\"\n\n"
            "[build]\n"
            "entry = \"src/main.ax\"\n"
            "out_dir = \"dist\"\n",
            encoding="utf-8",
        )
        started = time.monotonic()
        try:
            completed = subprocess.run(
                [str(axiomc), "parse", "--json", str(project_path)],
                cwd=root,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                timeout=timeout_ms / 1000,
            )
        except subprocess.TimeoutExpired as error:
            duration_ms = int((time.monotonic() - started) * 1000)
            return Execution(
                "timeout", None, duration_ms, str(error.stderr or "")[-512:], None
            )
        duration_ms = int((time.monotonic() - started) * 1000)
        if completed.returncode < 0 or completed.returncode > 1:
            return Execution(
                "crash",
                completed.returncode,
                duration_ms,
                completed.stderr[-512:],
                None,
            )
        try:
            payload = json.loads(completed.stdout)
        except json.JSONDecodeError:
            return Execution(
                "invalid_output",
                completed.returncode,
                duration_ms,
                completed.stderr[-512:],
                None,
            )
        if (
            not isinstance(payload, dict)
            or payload.get("schema_version") != "axiom.stage1.v1"
            or payload.get("command") != "parse"
            or not isinstance(payload.get("ok"), bool)
        ):
            return Execution(
                "invalid_output",
                completed.returncode,
                duration_ms,
                completed.stderr[-512:],
                None,
            )
        return Execution(
            "accepted" if payload["ok"] else "diagnostic",
            completed.returncode,
            duration_ms,
            completed.stderr[-512:],
            payload,
        )


def minimize_failure(
    axiomc: Path,
    source: str,
    target_status: str,
    timeout_ms: int,
    root: Path,
    deadline: float,
) -> str:
    current = source
    candidates: list[str] = []
    lines = current.splitlines(keepends=True)
    if len(lines) > 1:
        for index in range(min(4, len(lines))):
            candidates.append("".join(lines[:index] + lines[index + 1 :]))
    for divisor in (2, 3, 4):
        width = len(current) // divisor
        if width:
            candidates.append(current[width:])
            candidates.append(current[:-width])
    for candidate in candidates[:8]:
        if time.monotonic() >= deadline or not candidate:
            break
        result = run_parser(axiomc, candidate, timeout_ms, root)
        if result.status == target_status and len(candidate) < len(current):
            current = candidate
    return current


def case_record(
    *,
    case_id: str,
    case_seed: int,
    entry: CorpusEntry,
    source: str,
    execution: Execution,
) -> dict[str, Any]:
    record: dict[str, Any] = {
        "case_id": case_id,
        "seed": case_seed,
        "corpus_entry": entry.name,
        "source_sha256": hashlib.sha256(source.encode("utf-8")).hexdigest(),
        "input_bytes": len(source.encode("utf-8")),
        "status": execution.status,
        "exit_code": execution.exit_code,
        "duration_ms": execution.duration_ms,
    }
    if execution.stderr:
        record["stderr"] = execution.stderr
    return record


def run_profile(args: argparse.Namespace) -> tuple[dict[str, Any], int]:
    root = repo_root()
    axiomc = args.axiomc if args.axiomc.is_absolute() else root / args.axiomc
    axiomc = axiomc.resolve()
    if not axiomc.is_file() or not os.access(axiomc, os.X_OK):
        raise FuzzInputError(f"axiomc executable is unavailable: {axiomc}")
    head = git_head(root)
    if args.expected_head is not None and args.expected_head != head:
        raise FuzzInputError(
            f"checkout HEAD {head} does not match expected head {args.expected_head}"
        )
    entries, corpus_sha256 = load_corpus(root, args.corpus)
    output = args.output if args.output.is_absolute() else root / args.output
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    reproducer_dir = output.parent / "parser-fuzz-reproducers"
    reproducer_dir.mkdir(parents=True, exist_ok=True)
    deadline = time.monotonic() + args.budget_seconds
    records: list[dict[str, Any]] = []
    failures = 0
    for index in range(args.cases):
        if time.monotonic() >= deadline:
            break
        case_seed = (args.seed + index * 0x9E3779B9) & ((1 << 64) - 1)
        rng = DeterministicRng(case_seed)
        entry = entries[index % len(entries)]
        source = mutate(entry, rng)
        case_id = f"parser-{index:04d}-{case_seed:016x}"
        execution = run_parser(axiomc, source, args.timeout_ms, root)
        record = case_record(
            case_id=case_id,
            case_seed=case_seed,
            entry=entry,
            source=source,
            execution=execution,
        )
        if execution.status in FAILURE_STATUSES:
            failures += 1
            minimized = minimize_failure(
                axiomc,
                source,
                execution.status,
                args.timeout_ms,
                root,
                deadline,
            )
            reproducer = reproducer_dir / f"{case_id}.ax"
            reproducer.write_text(minimized, encoding="utf-8")
            record["reproducer"] = reproducer.relative_to(output.parent).as_posix()
            record["reproducer_bytes"] = len(minimized.encode("utf-8"))
        records.append(record)
    report = {
        "schema_version": SCHEMA_VERSION,
        "head_sha": head,
        "corpus_sha256": corpus_sha256,
        "seed": args.seed,
        "cases_requested": args.cases,
        "cases_executed": len(records),
        "timeout_ms": args.timeout_ms,
        "budget_seconds": args.budget_seconds,
        "failure_count": failures,
        "status": "passed" if failures == 0 and len(records) == args.cases else "failed",
        "cases": records,
    }
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return report, 0 if report["status"] == "passed" else 1


def main(argv: Sequence[str] | None = None) -> int:
    try:
        report, status = run_profile(parse_args(argv))
    except (FuzzInputError, OSError, ValueError) as error:
        print(f"parser fuzz smoke: fail\n- {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    return status


if __name__ == "__main__":
    raise SystemExit(main())
