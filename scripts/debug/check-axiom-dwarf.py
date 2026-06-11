#!/usr/bin/env python3
"""Check whether a debug manifest has native Axiom DWARF evidence."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


DWARF_LINE_MARKERS = (
    b".debug_line",
    b".zdebug_line",
    b"__debug_line",
)
DWARF_INFO_MARKERS = (
    b".debug_info",
    b".zdebug_info",
    b"__debug_info",
)


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


def read_binary(path: Path) -> bytes:
    try:
        return path.read_bytes()
    except FileNotFoundError:
        raise SystemExit(f"binary not found: {path}") from None


def binary_has_marker(binary: bytes, markers: tuple[bytes, ...]) -> bool:
    return any(marker in binary for marker in markers)


def binary_has_axiom_source_path(binary: bytes) -> bool:
    return b".ax" in binary


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

    binary = read_binary(binary_path)
    missing: list[str] = []
    if not binary_has_marker(binary, DWARF_LINE_MARKERS):
        missing.append("DWARF line section marker")
    if not binary_has_marker(binary, DWARF_INFO_MARKERS):
        missing.append("DWARF info section marker")
    if not binary_has_axiom_source_path(binary):
        missing.append(".ax source path marker")

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
