#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from collections import defaultdict
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
NON_BLOCKING_SUMMARY_KEYS = {"total", "killed", "survived", "blocking"}
NON_BLOCKING_MUTANT_STATUSES = {"killed", "survived"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render mutation survivors as stable Markdown for issue comments."
    )
    parser.add_argument("--input", type=Path, required=True, help="mutation smoke JSON")
    parser.add_argument("--output", type=Path, help="Markdown output path")
    return parser.parse_args()


def slug(value: str) -> str:
    normalized = re.sub(r"[^a-zA-Z0-9]+", "_", value.strip().lower())
    return normalized.strip("_") or "survivor"


def survivor_entries(payload: dict[str, Any]) -> list[dict[str, Any]]:
    if "survivors" in payload and isinstance(payload["survivors"], list):
        return [entry for entry in payload["survivors"] if isinstance(entry, dict)]
    mutants = payload.get("mutants", [])
    if isinstance(mutants, list):
        return [
            entry
            for entry in mutants
            if isinstance(entry, dict) and entry.get("status") == "survived"
        ]
    return []


def recommended_fixture(entry: dict[str, Any]) -> str:
    area = slug(str(entry.get("area", "stage1")))
    name = slug(str(entry.get("name", entry.get("test_filter", "survivor"))))
    return f"{area}_{name}_survivor_test.ax"


def render_governing_issue(payload: dict[str, Any]) -> str:
    issue = payload.get("governing_issue")
    if not isinstance(issue, dict):
        return "unknown"
    number = issue.get("number")
    url = issue.get("url")
    if isinstance(number, int) and isinstance(url, str) and url:
        return f"[#{number}]({url})"
    if isinstance(number, int):
        return f"#{number}"
    return "unknown"


def nonnegative_count(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int) and value >= 0:
        return value
    if isinstance(value, str) and value.isdigit():
        return int(value)
    return None


def blocking_outcome_counts(
    payload: dict[str, Any], summary: dict[str, Any]
) -> dict[str, int]:
    mutants = payload.get("mutants", [])
    mutant_entries = mutants if isinstance(mutants, list) else []
    statuses = {
        key
        for key in summary
        if isinstance(key, str) and key not in NON_BLOCKING_SUMMARY_KEYS
    }
    statuses.update(
        str(entry.get("status"))
        for entry in mutant_entries
        if isinstance(entry, dict)
        and isinstance(entry.get("status"), str)
        and entry.get("status") not in NON_BLOCKING_MUTANT_STATUSES
    )
    counts: dict[str, int] = {}
    for status in sorted(statuses):
        summary_count = nonnegative_count(summary.get(status))
        mutant_count = sum(
            isinstance(entry, dict) and entry.get("status") == status
            for entry in mutant_entries
        )
        count = max(summary_count or 0, mutant_count)
        if count:
            counts[status] = count
    return counts


def blocking_entries(payload: dict[str, Any]) -> list[dict[str, Any]]:
    mutants = payload.get("mutants", [])
    if not isinstance(mutants, list):
        return []
    return sorted(
        (
            entry
            for entry in mutants
            if isinstance(entry, dict)
            and entry.get("status") not in NON_BLOCKING_MUTANT_STATUSES
        ),
        key=lambda entry: (
            str(entry.get("file", "")),
            str(entry.get("test_filter", "")),
            str(entry.get("name", "")),
        ),
    )


def render_inline(value: Any) -> str:
    return str(value).replace("`", r"\`").replace("\r", " ").replace("\n", " ")


def render_report(payload: dict[str, Any]) -> str:
    schema = payload.get("schema_version", "unknown")
    raw_summary = payload.get("summary", {})
    summary = raw_summary if isinstance(raw_summary, dict) else {}
    survivors = sorted(
        survivor_entries(payload),
        key=lambda entry: (
            str(entry.get("file", "")),
            str(entry.get("test_filter", "")),
            str(entry.get("name", "")),
        ),
    )
    outcome_counts = blocking_outcome_counts(payload, summary)
    blocked_mutants = blocking_entries(payload)
    categorized_blocking = sum(outcome_counts.values())
    recorded_blocking = nonnegative_count(summary.get("blocking"))
    blocking_count = max(recorded_blocking or 0, categorized_blocking)
    fatal_error = payload.get("fatal_error")
    has_fatal_error = fatal_error is not None and str(fatal_error).strip() != ""
    raw_status = payload.get("status")
    if isinstance(raw_status, str) and raw_status.strip():
        overall_status = raw_status.strip()
    elif has_fatal_error or blocking_count:
        overall_status = "failed"
    elif not survivors:
        overall_status = "passed (legacy report)"
    else:
        overall_status = "unknown (legacy report)"
    is_blocking = (
        overall_status.lower() == "failed"
        or has_fatal_error
        or blocking_count > 0
    )

    lines = [
        "## Mutation Survivor Report",
        "",
        f"- Source schema: `{schema}`",
        f"- Overall status: `{render_inline(overall_status)}`",
        f"- Total mutants: `{summary.get('total', 'unknown')}`",
        f"- Killed: `{summary.get('killed', 'unknown')}`",
        f"- Survived: `{len(survivors)}`",
        f"- Blocking count: `{blocking_count}`",
        (
            f"- Fatal error: `{render_inline(fatal_error)}`"
            if has_fatal_error
            else "- Fatal error: `none`"
        ),
        f"- Governing issue: {render_governing_issue(payload)}",
        "",
    ]
    if outcome_counts:
        lines.extend(["### Blocking outcomes", ""])
        for status, count in outcome_counts.items():
            lines.append(f"- `{status}`: `{count}`")
        lines.append("")
    if blocked_mutants:
        lines.extend(["### Blocking details", ""])
        for mutant in blocked_mutants:
            lines.append(
                f"- `{render_inline(mutant.get('name', 'unnamed'))}` "
                f"(`{render_inline(mutant.get('status', 'unknown'))}`, "
                f"`{render_inline(mutant.get('file', 'unknown'))}`)"
            )
            reproducer = mutant.get("reproducer")
            if isinstance(reproducer, str) and reproducer.strip():
                lines.append(f"  Reproducer: `{render_inline(reproducer)}`")
        lines.append("")

    if not survivors:
        if is_blocking:
            lines.append(
                "No survivors were reported, but mutation qualification is blocked. "
                "Follow-up is required before treating this as a clean result."
            )
        else:
            lines.append("No survivors were reported. No follow-up fixtures are recommended.")
        return "\n".join(lines) + "\n"

    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for survivor in survivors:
        grouped[str(survivor.get("file", "unknown"))].append(survivor)

    for file_path in sorted(grouped):
        lines.append(f"### `{file_path}`")
        for survivor in grouped[file_path]:
            name = survivor.get("name", "unnamed")
            test_filter = survivor.get("test_filter", "unknown")
            area = survivor.get("area", "unknown")
            fixture = recommended_fixture(survivor)
            reproducer = survivor.get("reproducer")
            lines.extend(
                [
                    f"- Survivor: `{name}`",
                    f"  Function/test focus: `{test_filter}`",
                    f"  Area: `{area}`",
                    f"  Recommended fixture: `{fixture}`",
                ]
            )
            if isinstance(reproducer, str) and reproducer.strip():
                lines.append(f"  Reproducer: `{render_inline(reproducer)}`")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    args = parse_args()
    input_path = args.input if args.input.is_absolute() else REPO_ROOT / args.input
    payload = json.loads(input_path.read_text())
    report = render_report(payload)
    if args.output:
        output = args.output if args.output.is_absolute() else REPO_ROOT / args.output
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(report)
    print(report, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
