#!/usr/bin/env python3
"""Check whether a debug manifest has native Axiom DWARF evidence."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


DWARF_LINE_SECTIONS = (
    ".debug_line",
    ".zdebug_line",
    "__debug_line",
)
DWARF_INFO_SECTIONS = (
    ".debug_info",
    ".zdebug_info",
    "__debug_info",
)
SECTION_LINE = re.compile(r"^\s*(?P<section>[._A-Za-z0-9]+)\s+(?P<size>\d+)\b")


def load_json(path: Path) -> dict[str, Any]:
    try:
        with path.open("r", encoding="utf-8") as handle:
            payload = json.load(handle)
    except FileNotFoundError:
        raise SystemExit(f"debug manifest not found: {path}") from None
    except json.JSONDecodeError as error:
        raise SystemExit(f"invalid JSON in {path}: {error}") from None
    if not isinstance(payload, dict):
        raise SystemExit(f"debug manifest must be a JSON object: {path}")
    return payload


def manifest_binary_path(manifest_path: Path, manifest: dict[str, Any]) -> Path:
    value = manifest.get("binary")
    if not isinstance(value, str) or not value:
        raise SystemExit(f"manifest does not contain a binary path: {manifest_path}")
    path = Path(value)
    if path.is_absolute():
        return path
    return manifest_path.parent / path


def native_debug_claims_axiom_dwarf(manifest: dict[str, Any]) -> bool:
    native_debug = manifest.get("native_debug")
    if not isinstance(native_debug, dict):
        raise SystemExit("manifest does not contain a native_debug object")
    value = native_debug.get("axiom_dwarf")
    if not isinstance(value, bool):
        raise SystemExit("manifest native_debug.axiom_dwarf must be a boolean")
    return value


def require_binary(path: Path) -> None:
    if not path.is_file():
        raise SystemExit(f"binary not found: {path}")


def dwarf_tool() -> str:
    override = os.environ.get("AXIOM_DWARFDUMP")
    if override:
        return override
    for candidate in ("llvm-dwarfdump", "dwarfdump"):
        found = shutil.which(candidate)
        if found:
            return found
    raise SystemExit("dwarfdump tool not found; install llvm-dwarfdump or dwarfdump")


def run_dwarfdump(binary_path: Path, *args: str) -> subprocess.CompletedProcess[str]:
    command = [dwarf_tool(), *args, str(binary_path)]
    return subprocess.run(
        command,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def dwarf_section_sizes(binary_path: Path) -> dict[str, int]:
    result = run_dwarfdump(binary_path, "--show-section-sizes")
    if result.returncode != 0:
        raise SystemExit(
            "failed to inspect DWARF section sizes: "
            + (result.stderr.strip() or result.stdout.strip())
        )
    sections: dict[str, int] = {}
    for line in result.stdout.splitlines():
        match = SECTION_LINE.match(line)
        if not match:
            continue
        sections[match.group("section")] = int(match.group("size"))
    return sections


def has_nonempty_section(sections: dict[str, int], names: tuple[str, ...]) -> bool:
    return any(sections.get(name, 0) > 0 for name in names)


def dwarf_sources(binary_path: Path) -> str:
    result = run_dwarfdump(binary_path, "--show-sources")
    if result.returncode != 0:
        raise SystemExit(
            "failed to inspect DWARF source paths: "
            + (result.stderr.strip() or result.stdout.strip())
        )
    return result.stdout


def dwarf_sources_include_axiom_path(sources: str) -> bool:
    return any(token.endswith(".ax") or ".ax:" in token for token in re.split(r"\s+", sources))


def command_verify(args: argparse.Namespace) -> int:
    manifest_path = Path(args.manifest)
    manifest = load_json(manifest_path)
    binary_path = manifest_binary_path(manifest_path, manifest)
    claims_axiom_dwarf = native_debug_claims_axiom_dwarf(manifest)
    if not claims_axiom_dwarf:
        print(
            "native_debug.axiom_dwarf is false; binary is not claimed to contain native Axiom DWARF",
            file=sys.stderr,
        )
        return 1

    require_binary(binary_path)
    sections = dwarf_section_sizes(binary_path)
    sources = dwarf_sources(binary_path)
    missing: list[str] = []
    if not has_nonempty_section(sections, DWARF_LINE_SECTIONS):
        missing.append("non-empty DWARF line section")
    if not has_nonempty_section(sections, DWARF_INFO_SECTIONS):
        missing.append("non-empty DWARF info section")
    if not dwarf_sources_include_axiom_path(sources):
        missing.append(".ax source path in DWARF sources")

    if missing:
        print(
            f"native Axiom DWARF claim is missing evidence: {', '.join(missing)}",
            file=sys.stderr,
        )
        return 1

    if args.json:
        print(
            json.dumps(
                {
                    "binary": str(binary_path),
                    "manifest": str(manifest_path),
                    "ok": True,
                    "requirements": [
                        "native_debug.axiom_dwarf",
                        "dwarf_line_section",
                        "dwarf_info_section",
                        "ax_source_path",
                    ],
                },
                sort_keys=True,
            )
        )
    else:
        print(f"native Axiom DWARF evidence present: {binary_path}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Verify that a debug manifest and binary contain native Axiom DWARF evidence.",
    )
    subcommands = parser.add_subparsers(dest="command", required=True)

    verify = subcommands.add_parser(
        "verify",
        help="Fail closed unless the manifest and binary prove native Axiom DWARF evidence.",
    )
    verify.add_argument(
        "--manifest",
        required=True,
        help="Path to a <artifact>.debug-manifest.json file.",
    )
    verify.add_argument("--json", action="store_true", help="Emit a JSON result on success.")
    verify.set_defaults(func=command_verify)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
