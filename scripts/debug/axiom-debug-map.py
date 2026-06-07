#!/usr/bin/env python3
"""Resolve generated-Rust debug lines back to Axiom source spans."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


def load_json(path: Path) -> dict[str, Any]:
    try:
        with path.open("r", encoding="utf-8") as handle:
            payload = json.load(handle)
    except FileNotFoundError:
        raise SystemExit(f"debug artifact not found: {path}") from None
    except json.JSONDecodeError as error:
        raise SystemExit(f"invalid JSON in {path}: {error}") from None
    if not isinstance(payload, dict):
        raise SystemExit(f"debug artifact must be a JSON object: {path}")
    return payload


def debug_map_path(args: argparse.Namespace) -> Path:
    if args.debug_map:
        return Path(args.debug_map)
    if args.manifest:
        manifest_path = Path(args.manifest)
        manifest = load_json(manifest_path)
        path = manifest.get("debug_map")
        if not isinstance(path, str) or not path:
            raise SystemExit(f"manifest does not contain a debug_map path: {manifest_path}")
        return Path(path)
    raise SystemExit("provide --debug-map or --manifest")


def resolve_span(debug_map: dict[str, Any], generated_line: int) -> dict[str, Any] | None:
    mappings = debug_map.get("mappings")
    if not isinstance(mappings, list):
        raise SystemExit("debug map does not contain a mappings array")
    for mapping in mappings:
        if not isinstance(mapping, dict):
            continue
        if mapping.get("generated_line") == generated_line:
            source = mapping.get("source")
            line = mapping.get("line")
            column = mapping.get("column")
            if not isinstance(source, str) or not isinstance(line, int) or not isinstance(column, int):
                raise SystemExit("debug map contains a malformed source mapping")
            return {"source": source, "line": line, "column": column}
    return None


def command_resolve(args: argparse.Namespace) -> int:
    map_path = debug_map_path(args)
    debug_map = load_json(map_path)
    span = resolve_span(debug_map, args.generated_line)
    if span is None:
        print(f"no Axiom span for generated line {args.generated_line}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(span, sort_keys=True))
    else:
        print(f"{span['source']}:{span['line']}:{span['column']}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Resolve axiomc --debug generated-Rust lines to Axiom source spans.",
    )
    subcommands = parser.add_subparsers(dest="command", required=True)

    resolve = subcommands.add_parser(
        "resolve",
        help="Resolve a generated Rust DWARF line through a .debug-map.json sidecar.",
    )
    source = resolve.add_mutually_exclusive_group(required=True)
    source.add_argument("--debug-map", help="Path to <artifact>.debug-map.json")
    source.add_argument("--manifest", help="Path to <artifact>.debug-manifest.json")
    resolve.add_argument(
        "--generated-line",
        required=True,
        type=int,
        help="Generated Rust line reported by LLDB/GDB/readelf.",
    )
    resolve.add_argument("--json", action="store_true", help="Emit a JSON span object.")
    resolve.set_defaults(func=command_resolve)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
