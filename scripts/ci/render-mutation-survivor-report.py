#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from collections import defaultdict
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]


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


def render_report(payload: dict[str, Any]) -> str:
    schema = payload.get("schema_version", "unknown")
    summary = payload.get("summary", {})
    survivors = sorted(
        survivor_entries(payload),
        key=lambda entry: (
            str(entry.get("file", "")),
            str(entry.get("test_filter", "")),
            str(entry.get("name", "")),
        ),
    )

    lines = [
        "## Mutation Survivor Report",
        "",
        f"- Source schema: `{schema}`",
        f"- Total mutants: `{summary.get('total', 'unknown')}`",
        f"- Killed: `{summary.get('killed', 'unknown')}`",
        f"- Survived: `{len(survivors)}`",
        "",
    ]
    if not survivors:
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
            lines.extend(
                [
                    f"- Survivor: `{name}`",
                    f"  Function/test focus: `{test_filter}`",
                    f"  Area: `{area}`",
                    f"  Recommended fixture: `{fixture}`",
                ]
            )
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
