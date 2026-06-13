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
FNV_OFFSET_BASIS = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3
TOOL_KIND_LLVM = "llvm"
TOOL_KIND_GENERIC = "generic"


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


def read_binary_bytes(path: Path) -> bytes:
    if not path.is_file():
        raise SystemExit(f"binary not found: {path}")
    return path.read_bytes()


def hash_bytes(value: bytes) -> str:
    hash_value = FNV_OFFSET_BASIS
    for byte in value:
        hash_value ^= byte
        hash_value = (hash_value * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
    return f"{hash_value:016x}"


def verify_manifest_binary_hash(manifest: dict[str, Any], binary_bytes: bytes) -> bool:
    expected = manifest.get("binary_hash")
    if not isinstance(expected, str) or not expected:
        print("manifest binary_hash is missing or invalid", file=sys.stderr)
        return False
    actual = hash_bytes(binary_bytes)
    if expected != actual:
        print(
            f"manifest binary_hash mismatch: expected {expected}, actual {actual}",
            file=sys.stderr,
        )
        return False
    return True


def dwarf_tool() -> str:
    override = os.environ.get("AXIOM_DWARFDUMP")
    if override:
        return override
    for candidate in ("llvm-dwarfdump", "dwarfdump"):
        found = shutil.which(candidate)
        if found:
            return found
    raise SystemExit("dwarfdump tool not found; install llvm-dwarfdump or dwarfdump")


def dwarf_tool_kind(tool_path: str) -> str:
    tool_name = Path(tool_path).name
    return TOOL_KIND_LLVM if tool_name.startswith("llvm-dwarfdump") else TOOL_KIND_GENERIC


def run_dwarfdump(tool_path: str, binary_path: Path, *args: str) -> subprocess.CompletedProcess[str]:
    command = [tool_path, *args, str(binary_path)]
    return subprocess.run(
        command,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def tool_section_args(tool_kind: str) -> tuple[str, str]:
    if tool_kind == TOOL_KIND_LLVM:
        return ("--show-section-sizes", "--show-sources")
    return ("--debug-line", "--debug-info")


def dwarf_section_output(tool_path: str, binary_path: Path) -> tuple[str, str]:
    tool_kind = dwarf_tool_kind(tool_path)
    section_arg, source_arg = tool_section_args(tool_kind)
    section_result = run_dwarfdump(tool_path, binary_path, section_arg)
    if section_result.returncode != 0:
        raise SystemExit(
            "failed to inspect DWARF section sizes: "
            + (section_result.stderr.strip() or section_result.stdout.strip())
        )
    source_result = run_dwarfdump(tool_path, binary_path, source_arg)
    if source_result.returncode != 0:
        raise SystemExit(
            "failed to inspect DWARF source paths: "
            + (source_result.stderr.strip() or source_result.stdout.strip())
        )
    return section_result.stdout, source_result.stdout


def dwarf_sources_include_axiom_path(sources: str) -> bool:
    return ".ax" in sources


def section_present(output: str, section_names: tuple[str, ...], tool_kind: str) -> bool:
    if tool_kind == TOOL_KIND_LLVM:
        return any(
            re.search(rf"^\s*{re.escape(section)}\s+\d+\b", output, re.MULTILINE)
            for section in section_names
        )
    return any(
        re.search(rf"^\s*{re.escape(section)}(?:\b|[:\[])", output, re.MULTILINE)
        for section in section_names
    )


def command_verify(args: argparse.Namespace) -> int:
    manifest_path = Path(args.manifest)
    manifest = load_json(manifest_path)
    binary_path = manifest_binary_path(manifest_path, manifest)
    binary_bytes = read_binary_bytes(binary_path)
    if not verify_manifest_binary_hash(manifest, binary_bytes):
        return 1

    claims_axiom_dwarf = native_debug_claims_axiom_dwarf(manifest)
    if not claims_axiom_dwarf:
        print(
            "native_debug.axiom_dwarf is false; binary is not claimed to contain native Axiom DWARF",
            file=sys.stderr,
        )
        return 1

    tool_path = dwarf_tool()
    tool_kind = dwarf_tool_kind(tool_path)
    section_output, source_output = dwarf_section_output(tool_path, binary_path)
    source_text = source_output if tool_kind == TOOL_KIND_LLVM else section_output + "\n" + source_output
    missing: list[str] = []
    if not section_present(section_output, DWARF_LINE_SECTIONS, tool_kind):
        missing.append("non-empty DWARF line section")
    if not section_present(section_output, DWARF_INFO_SECTIONS, tool_kind):
        missing.append("non-empty DWARF info section")
    if not dwarf_sources_include_axiom_path(source_text):
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
