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
    line: int
    end_line: int
    lines: int
    complexity: int
    coverage: float

    @property
    def crap(self) -> float:
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


def collect_metrics(source_root: Path, default_coverage: float) -> list[FunctionMetric]:
    metrics: list[FunctionMetric] = []
    for path in sorted(source_root.rglob("*.rs")):
        for name, start, end, body in function_ranges(path):
            metrics.append(
                FunctionMetric(
                    name=name,
                    path=path,
                    line=start,
                    end_line=end,
                    lines=end - start + 1,
                    complexity=cyclomatic_complexity(body),
                    coverage=default_coverage,
                )
            )
    return metrics


def proposal(
    metrics: list[FunctionMetric],
    threshold: float,
    max_hotspots: int,
    source_root: Path,
) -> dict:
    hotspots = sorted(metrics, key=lambda metric: metric.crap, reverse=True)[:max_hotspots]
    return {
        "schema_version": "axiom.stage1.crap-threshold-proposal.v1",
        "blocking": False,
        "source_root": str(source_root),
        "threshold": threshold,
        "inputs": {
            "coverage": "defaulted until coverage artifacts are wired into extended validation",
            "complexity": "heuristic branch-token scan over stage1 Rust sources",
        },
        "summary": {
            "functions_scanned": len(metrics),
            "hotspots_over_threshold": sum(1 for metric in metrics if metric.crap > threshold),
            "max_crap": round(max((metric.crap for metric in metrics), default=0.0), 2),
        },
        "hotspots": [
            {
                "function": metric.name,
                "path": str(metric.path),
                "line": metric.line,
                "end_line": metric.end_line,
                "lines": metric.lines,
                "complexity": metric.complexity,
                "coverage": metric.coverage,
                "crap": round(metric.crap, 2),
                "over_threshold": metric.crap > threshold,
            }
            for metric in hotspots
        ],
        "proposed_policy": {
            "warn_threshold": threshold,
            "blocking_threshold": None,
            "enable_blocking_by": "rerun with --enforce after coverage artifacts and baselines are stable",
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, default=DEFAULT_SOURCE_ROOT)
    parser.add_argument("--threshold", type=float, default=DEFAULT_THRESHOLD)
    parser.add_argument("--default-coverage", type=float, default=0.0)
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

    metrics = collect_metrics(args.source_root, args.default_coverage)
    if not metrics:
        print(f"error: no Rust functions discovered under source root: {args.source_root}", file=sys.stderr)
        return 2

    report = proposal(metrics, args.threshold, args.max_hotspots, args.source_root)
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
