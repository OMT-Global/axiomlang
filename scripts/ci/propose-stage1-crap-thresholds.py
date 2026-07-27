#!/usr/bin/env python3
"""Propose non-blocking CRAP thresholds for stage1 Rust hotspots."""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_SOURCE_ROOT = REPO_ROOT / "stage1/crates/axiomc/src"
DEFAULT_THRESHOLD = 30.0
FN_RE = re.compile(
    r"^(?P<indent>\s*)(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|const|unsafe)\s+)*fn\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\b"
)
DECISION_RE = re.compile(r"\b(if|else\s+if|match|while|for|loop)\b|&&|\|\||\?|=>")


@dataclass(frozen=True)
class FunctionMetric:
    name: str
    path: Path
    ordinal: int
    line: int
    end_line: int
    lines: int
    complexity: int
    coverage: float | None

    @property
    def crap(self) -> float | None:
        if self.coverage is None:
            return None
        uncovered = 1.0 - self.coverage
        return (self.complexity**2 * uncovered**3) + self.complexity


def code_chars(line: str) -> str:
    """Return a line with string/char literals and line comments neutralized."""
    out: list[str] = []
    index = 0
    in_string = False
    in_char = False
    escape = False
    while index < len(line):
        ch = line[index]
        nxt = line[index + 1] if index + 1 < len(line) else ""
        if not in_string and not in_char and ch == "/" and nxt == "/":
            break
        if escape:
            escape = False
            out.append(" ")
        elif ch == "\\" and (in_string or in_char):
            escape = True
            out.append(" ")
        elif ch == '"' and not in_char:
            in_string = not in_string
            out.append(" ")
        elif ch == "'" and not in_string:
            if nxt and not (nxt.isalpha() or nxt == "_"):
                in_char = not in_char
            out.append(" ")
        elif in_string or in_char:
            out.append(" ")
        else:
            out.append(ch)
        index += 1
    return "".join(out)


def count_delta(line: str) -> int:
    code = code_chars(line)
    return code.count("{") - code.count("}")


def cyclomatic_complexity(lines: list[str]) -> int:
    return 1 + sum(len(DECISION_RE.findall(code_chars(line))) for line in lines)


def function_ranges(path: Path) -> list[tuple[str, int, int, list[str]]]:
    text = path.read_text(encoding="utf-8").splitlines()
    ranges: list[tuple[str, int, int, list[str]]] = []
    index = 0
    while index < len(text):
        match = FN_RE.match(text[index])
        if not match:
            index += 1
            continue

        start = index + 1
        cursor = index
        brace_depth = 0
        seen_open = False
        body: list[str] = []
        while cursor < len(text):
            line = text[cursor]
            body.append(line)
            if "{" in code_chars(line):
                seen_open = True
            brace_depth += count_delta(line)
            if seen_open and brace_depth <= 0:
                break
            cursor += 1

        ranges.append((match.group("name"), start, min(cursor + 1, len(text)), body))
        index = cursor + 1
    return ranges


def is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
    except ValueError:
        return False
    return True


def is_rust_stdlib_source(path: Path) -> bool:
    """Return whether an external LCOV source is from rustc's standard-library tree."""
    marker = ("lib", "rustlib", "src", "rust", "library")
    parts = path.parts
    return path.suffix == ".rs" and any(
        parts[index : index + len(marker)] == marker
        for index in range(len(parts) - len(marker) + 1)
    )


def parse_lcov(path: Path, source_root: Path) -> dict[Path, dict[int, int]]:
    """Read and validate LCOV line hit counts keyed by resolved source path."""
    coverage: dict[Path, dict[int, int]] = {}
    seen_sources: set[Path] = set()
    source_root = source_root.resolve()
    repo_root = REPO_ROOT.resolve()
    source_boundary = repo_root if is_within(source_root, repo_root) else source_root
    current: Path | None = None
    current_lines: dict[int, int] = {}
    declared_lines: int | None = None
    declared_hit_lines: int | None = None

    for line_number, raw_line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if raw_line.startswith("SF:"):
            if current is not None:
                raise ValueError(f"nested SF record at LCOV line {line_number}; previous record is truncated")
            source_text = raw_line.removeprefix("SF:")
            if not source_text:
                raise ValueError(f"empty LCOV source path at line {line_number}")
            candidate = Path(source_text)
            candidate = candidate.resolve() if candidate.is_absolute() else (REPO_ROOT / candidate).resolve()
            if not is_within(candidate, source_boundary) and not is_rust_stdlib_source(candidate):
                raise ValueError(
                    f"LCOV source path escapes allowed source boundary {source_boundary}: {candidate}"
                )
            if candidate in seen_sources:
                raise ValueError(f"duplicate LCOV source record: {candidate}")
            seen_sources.add(candidate)
            current = candidate
            current_lines = {}
            declared_lines = None
            declared_hit_lines = None
        elif raw_line.startswith("DA:"):
            if current is None:
                raise ValueError(f"DA record without an active SF record at LCOV line {line_number}")
            match = re.fullmatch(r"DA:(?P<line>\d+),(?P<hits>-?\d+)(?:,[^,\r\n]+)?", raw_line)
            if not match:
                raise ValueError(f"invalid LCOV data record {raw_line!r} at line {line_number}")
            source_line = int(match.group("line"))
            hits = int(match.group("hits"))
            if source_line < 1:
                raise ValueError(f"invalid LCOV source line {source_line} at LCOV line {line_number}")
            if hits < 0:
                raise ValueError(f"negative LCOV hit count {hits} at LCOV line {line_number}")
            if source_line in current_lines:
                existing = current_lines[source_line]
                detail = "conflicting" if existing != hits else "duplicate"
                raise ValueError(
                    f"{detail} LCOV DA record for {current}:{source_line} at LCOV line {line_number}"
                )
            current_lines[source_line] = hits
        elif raw_line.startswith("LF:"):
            if current is None:
                raise ValueError(f"LF record without an active SF record at LCOV line {line_number}")
            if declared_lines is not None or not re.fullmatch(r"LF:\d+", raw_line):
                raise ValueError(f"invalid or duplicate LCOV LF record {raw_line!r} at line {line_number}")
            declared_lines = int(raw_line.removeprefix("LF:"))
        elif raw_line.startswith("LH:"):
            if current is None:
                raise ValueError(f"LH record without an active SF record at LCOV line {line_number}")
            if declared_hit_lines is not None or not re.fullmatch(r"LH:\d+", raw_line):
                raise ValueError(f"invalid or duplicate LCOV LH record {raw_line!r} at line {line_number}")
            declared_hit_lines = int(raw_line.removeprefix("LH:"))
        elif raw_line == "end_of_record":
            if current is None:
                raise ValueError(f"end_of_record without an active SF record at LCOV line {line_number}")
            if (
                declared_lines is not None
                and declared_hit_lines is not None
                and declared_hit_lines > declared_lines
            ):
                raise ValueError(
                    f"invalid LCOV summary for {current}: LH {declared_hit_lines} exceeds LF {declared_lines}"
                )
            if is_within(current, source_boundary):
                coverage[current] = current_lines
            current = None
            current_lines = {}
            declared_lines = None
            declared_hit_lines = None

    if current is not None:
        raise ValueError(f"truncated LCOV source record without end_of_record: {current}")
    return coverage


def function_coverage(path: Path, start: int, end: int, lcov: dict[Path, dict[int, int]] | None) -> float | None:
    if lcov is None:
        return None
    lines = lcov.get(path.resolve())
    if lines is None:
        return None
    relevant = [hits for line, hits in lines.items() if start <= line <= end]
    if not relevant:
        return None
    return sum(hits > 0 for hits in relevant) / len(relevant)


def collect_metrics(source_root: Path, lcov: dict[Path, dict[int, int]] | None) -> list[FunctionMetric]:
    metrics: list[FunctionMetric] = []
    for path in sorted(source_root.rglob("*.rs")):
        ordinals: dict[str, int] = {}
        for name, start, end, body in function_ranges(path):
            ordinal = ordinals.get(name, 0) + 1
            ordinals[name] = ordinal
            metrics.append(
                FunctionMetric(
                    name=name,
                    path=path,
                    ordinal=ordinal,
                    line=start,
                    end_line=end,
                    lines=end - start + 1,
                    complexity=cyclomatic_complexity(body),
                    coverage=function_coverage(path, start, end, lcov),
                )
            )
    return metrics


def proposal(
    metrics: list[FunctionMetric],
    threshold: float,
    max_hotspots: int,
    source_root: Path,
    lcov_path: Path | None,
    enforce: bool,
) -> dict:
    repo_root = REPO_ROOT.resolve()
    source_root = source_root.resolve()
    use_repo_relative_paths = is_within(source_root, repo_root)

    def display_path(path: Path) -> str:
        resolved = path.resolve()
        if use_repo_relative_paths and is_within(resolved, repo_root):
            return resolved.relative_to(repo_root).as_posix()
        return resolved.as_posix()

    measured = [metric for metric in metrics if metric.crap is not None]
    hotspots = sorted(
        measured,
        key=lambda metric: (-(metric.crap or 0.0), display_path(metric.path), metric.line, metric.name),
    )[:max_hotspots]
    return {
        "schema_version": "axiom.stage1.crap-threshold-proposal.v1",
        "blocking": enforce,
        "source_root": display_path(source_root),
        "threshold": threshold,
        "inputs": {
            "coverage": {
                "source": "lcov" if lcov_path else "unmeasured",
                "path": display_path(lcov_path) if lcov_path else None,
                "measured_functions": len(measured),
                "unmeasured_functions": len(metrics) - len(measured),
            },
            "complexity": "heuristic branch-token scan over stage1 Rust sources",
        },
        "summary": {
            "functions_scanned": len(metrics),
            "functions_with_coverage": len(measured),
            "functions_without_coverage": len(metrics) - len(measured),
            "hotspots_over_threshold": sum(1 for metric in measured if (metric.crap or 0.0) > threshold),
            "max_crap": round(max((metric.crap or 0.0 for metric in measured), default=0.0), 2),
        },
        "hotspots": [
            {
                "function": metric.name,
                "identity": f"{display_path(metric.path)}::{metric.name}#{metric.ordinal}",
                "path": display_path(metric.path),
                "line": metric.line,
                "end_line": metric.end_line,
                "lines": metric.lines,
                "complexity": metric.complexity,
                "coverage": metric.coverage,
                "crap": round(metric.crap or 0.0, 2),
                "over_threshold": (metric.crap or 0.0) > threshold,
            }
            for metric in hotspots
        ],
        "proposed_policy": {
            "warn_threshold": threshold,
            "blocking_enabled": enforce,
            "blocking_threshold": threshold if enforce else None,
            "status": "legacy --enforce mode active" if enforce else "advisory proposal only",
            "enable_blocking_by": (
                None
                if enforce
                else "rerun with --enforce after coverage artifacts and baselines are stable"
            ),
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, default=DEFAULT_SOURCE_ROOT)
    parser.add_argument("--threshold", type=float, default=DEFAULT_THRESHOLD)
    parser.add_argument("--lcov", type=Path, default=None, help="LCOV line coverage report")
    parser.add_argument("--max-hotspots", type=int, default=20)
    parser.add_argument("--output", type=Path, default=None)
    parser.add_argument("--enforce", action="store_true")
    args = parser.parse_args()

    if not args.source_root.exists():
        print(f"error: source root does not exist: {args.source_root}", file=sys.stderr)
        return 2
    if not args.source_root.is_dir():
        print(f"error: source root is not a directory: {args.source_root}", file=sys.stderr)
        return 2

    if args.lcov and not args.lcov.is_file():
        print(f"error: LCOV report does not exist: {args.lcov}", file=sys.stderr)
        return 2
    if args.enforce and not args.lcov:
        print("error: --enforce requires --lcov coverage evidence", file=sys.stderr)
        return 2
    source_root = args.source_root.resolve()
    if args.max_hotspots < 0:
        print("error: --max-hotspots must be non-negative", file=sys.stderr)
        return 2
    try:
        lcov = parse_lcov(args.lcov, source_root) if args.lcov else None
    except (OSError, ValueError) as error:
        print(f"error: failed to parse LCOV report: {error}", file=sys.stderr)
        return 2
    metrics = collect_metrics(source_root, lcov)
    if not metrics:
        print(f"error: no Rust functions discovered under source root: {args.source_root}", file=sys.stderr)
        return 2

    report = proposal(metrics, args.threshold, args.max_hotspots, source_root, args.lcov, args.enforce)
    payload = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(payload, encoding="utf-8")
    else:
        print(payload, end="")

    if args.enforce and report["summary"]["hotspots_over_threshold"] > 0:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
