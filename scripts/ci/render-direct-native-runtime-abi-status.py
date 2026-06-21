#!/usr/bin/env python3
"""Render the direct-native runtime ABI status table from the JSON contract."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CONTRACT = REPO_ROOT / "stage1/runtime-abi/direct-native-v0.json"
DEFAULT_DOC = REPO_ROOT / "docs/direct-native-runtime-abi-v0.md"
START_MARKER = "<!-- direct-native-runtime-abi-status:start -->"
END_MARKER = "<!-- direct-native-runtime-abi-status:end -->"


def load_contract(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("contract root must be an object")
    return payload


def escape_cell(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def compact_text(value: object, limit: int = 150) -> str:
    if not isinstance(value, str):
        return ""
    text = re.sub(r"\s+", " ", value).strip()
    if not text:
        return ""
    sentence_end = text.find(". ")
    if 0 <= sentence_end < limit:
        text = text[: sentence_end + 1]
    if len(text) <= limit:
        return text
    return text[: limit - 3].rstrip() + "..."


def evidence_summary(row: dict[str, Any]) -> str:
    parts = []
    for field_name, label in (
        ("evidence", "evidence"),
        ("runtime_evidence", "runtime"),
        ("denial_evidence", "denial"),
    ):
        values = row.get(field_name, [])
        if isinstance(values, list) and values:
            parts.append(f"{label}:{len(values)}")
    return ", ".join(parts) if parts else "-"


def blocker_summary(row: dict[str, Any]) -> str:
    blockers = row.get("blockers", [])
    if not isinstance(blockers, list) or not blockers:
        return "-"
    return ", ".join(f"#{issue}" for issue in blockers)


def render_group(title: str, rows: list[dict[str, Any]]) -> list[str]:
    lines = [
        f"### {title}",
        "",
        "| Row | Status | Blockers | Evidence | Scope |",
        "| --- | --- | --- | --- | --- |",
    ]
    for row in sorted(rows, key=lambda item: item.get("id", "")):
        row_id = escape_cell(str(row.get("id", "")))
        status = escape_cell(str(row.get("status", "")))
        blockers = escape_cell(blocker_summary(row))
        evidence = escape_cell(evidence_summary(row))
        scope = escape_cell(compact_text(row.get("notes")))
        lines.append(f"| `{row_id}` | `{status}` | {blockers} | {evidence} | {scope} |")
    lines.append("")
    return lines


def render_status(contract: dict[str, Any]) -> str:
    value_rows = contract.get("value_features", [])
    capability_rows = contract.get("capability_shims", [])
    if not isinstance(value_rows, list):
        value_rows = []
    if not isinstance(capability_rows, list):
        capability_rows = []

    lines = [
        START_MARKER,
        "",
        "_Generated from `stage1/runtime-abi/direct-native-v0.json`; run "
        "`make stage1-direct-native-runtime-abi-test` after changing the contract._",
        "",
    ]
    lines.extend(render_group("Value Features", value_rows))
    lines.extend(render_group("Capability Shims", capability_rows))
    lines.append(END_MARKER)
    return "\n".join(lines) + "\n"


def doc_status_block(doc_text: str) -> str:
    start = doc_text.find(START_MARKER)
    end = doc_text.find(END_MARKER)
    if start == -1 or end == -1 or end < start:
        raise ValueError("document is missing direct-native runtime ABI status markers")
    end += len(END_MARKER)
    if end < len(doc_text) and doc_text[end] == "\n":
        end += 1
    return doc_text[start:end]


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Render or check the direct-native runtime ABI status table."
    )
    parser.add_argument("--contract", type=Path, default=DEFAULT_CONTRACT)
    parser.add_argument("--check-doc", type=Path, default=None)
    args = parser.parse_args()

    try:
        rendered = render_status(load_contract(args.contract))
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"failed to render direct-native runtime ABI status: {error}", file=sys.stderr)
        return 1

    if args.check_doc is None:
        print(rendered, end="")
        return 0

    try:
        current = doc_status_block(args.check_doc.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        print(f"failed to read direct-native runtime ABI status block: {error}", file=sys.stderr)
        return 1

    if current != rendered:
        print(
            "direct-native runtime ABI status table is stale; "
            "regenerate it from stage1/runtime-abi/direct-native-v0.json",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
