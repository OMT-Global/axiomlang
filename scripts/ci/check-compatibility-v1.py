#!/usr/bin/env python3
"""Validate public contracts and report deterministic AxiOM compatibility drift."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


PUBLIC_CONTRACT_SCHEMA = "axiom.public_contract.v1"
REPORT_SCHEMA = "axiom.compatibility_report.v1"
KINDS = {"compiler", "language", "stdlib", "cli", "package", "abi", "schema", "artifact"}
STABILITIES = {"experimental", "stable", "deprecated"}
EDITION_STATUSES = {"experimental", "supported", "deprecated"}
SEMVER = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
AXIOM_ID = re.compile(r"^axiom://[A-Za-z0-9._~:/#@!$&'()*+,;=%-]+$")
CONTRACT_KEYS = {"schema_version", "edition", "compiler", "surfaces", "migrations"}
EDITION_KEYS = {"id", "status", "migration"}
COMPILER_KEYS = {"minimum", "maximum", "migration"}
SURFACE_KEYS = {"id", "kind", "version", "stability", "signature", "migration", "replacement"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old", required=True, type=Path)
    parser.add_argument("--new", required=True, type=Path)
    parser.add_argument("--schema-file", type=Path, default=Path("stage1/schemas/axiom-compatibility-report-v1.schema.json"))
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def load_json(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read {path}: {error}") from error
    if not isinstance(payload, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return payload


def require_string(value: Any, label: str, pattern: re.Pattern[str] | None = None) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{label} must be a non-empty string")
    if pattern is not None and not pattern.fullmatch(value):
        raise ValueError(f"{label} has invalid format: {value!r}")
    return value


def semver(value: Any, label: str) -> tuple[int, int, int]:
    return tuple(map(int, require_string(value, label, SEMVER).split(".")))  # type: ignore[return-value]


def reject_unknown_properties(value: dict[str, Any], allowed: set[str], label: str) -> None:
    unknown = sorted(set(value) - allowed)
    if unknown:
        raise ValueError("{} contains unknown properties: {}".format(label, ", ".join(unknown)))


def validate_contract(payload: dict[str, Any], label: str) -> dict[str, dict[str, Any]]:
    reject_unknown_properties(payload, CONTRACT_KEYS, label)
    if payload.get("schema_version") != PUBLIC_CONTRACT_SCHEMA:
        raise ValueError(f"{label} must use {PUBLIC_CONTRACT_SCHEMA}")
    edition = payload.get("edition")
    if not isinstance(edition, dict):
        raise ValueError(f"{label}.edition must be an object")
    reject_unknown_properties(edition, EDITION_KEYS, f"{label}.edition")
    require_string(edition.get("id"), f"{label}.edition.id", re.compile(r"^[0-9]{4}$"))
    if edition.get("status") not in EDITION_STATUSES:
        raise ValueError(f"{label}.edition.status must be one of {sorted(EDITION_STATUSES)}")
    if edition.get("status") == "deprecated":
        require_string(edition.get("migration"), f"{label}.edition.migration")
    compiler = payload.get("compiler")
    if not isinstance(compiler, dict):
        raise ValueError(f"{label}.compiler must be an object")
    reject_unknown_properties(compiler, COMPILER_KEYS, f"{label}.compiler")
    if semver(compiler.get("minimum"), f"{label}.compiler.minimum") > semver(compiler.get("maximum"), f"{label}.compiler.maximum"):
        raise ValueError(f"{label}.compiler minimum must not exceed maximum")
    if "migration" in compiler:
        require_string(compiler["migration"], f"{label}.compiler.migration")
    migrations = payload.get("migrations", {})
    if not isinstance(migrations, dict):
        raise ValueError(f"{label}.migrations must be an object")
    for identifier, action in migrations.items():
        require_string(identifier, f"{label}.migrations key", AXIOM_ID)
        require_string(action, f"{label}.migrations[{identifier!r}]")
    surfaces = payload.get("surfaces")
    if not isinstance(surfaces, list) or not surfaces:
        raise ValueError(f"{label}.surfaces must be a non-empty array")
    indexed: dict[str, dict[str, Any]] = {}
    for index, surface in enumerate(surfaces):
        prefix = f"{label}.surfaces[{index}]"
        if not isinstance(surface, dict):
            raise ValueError(f"{prefix} must be an object")
        reject_unknown_properties(surface, SURFACE_KEYS, prefix)
        identifier = require_string(surface.get("id"), f"{prefix}.id", AXIOM_ID)
        if identifier in indexed:
            raise ValueError(f"{label} duplicates public surface {identifier}")
        if surface.get("kind") not in KINDS:
            raise ValueError(f"{prefix}.kind must be one of {sorted(KINDS)}")
        semver(surface.get("version"), f"{prefix}.version")
        if surface.get("stability") not in STABILITIES:
            raise ValueError(f"{prefix}.stability must be one of {sorted(STABILITIES)}")
        require_string(surface.get("signature"), f"{prefix}.signature")
        if surface.get("stability") == "deprecated":
            require_string(surface.get("migration"), f"{prefix}.migration")
            require_string(surface.get("replacement"), f"{prefix}.replacement", AXIOM_ID)
        indexed[identifier] = surface
    return indexed


def surface_change(change: str, severity: str, surface: dict[str, Any], *, old: dict[str, Any] | None = None, migration: str | None = None) -> dict[str, Any]:
    result: dict[str, Any] = {
        "change": change,
        "severity": severity,
        "surface_kind": surface["kind"],
        "surface_id": surface["id"],
        "description": f"{change} {surface['kind']} surface {surface['id']}",
        "migration": migration,
    }
    if old is not None:
        result["old_version"] = old["version"]
    if change != "removed":
        result["new_version"] = surface["version"]
    return result


def classify_modified(old: dict[str, Any], new: dict[str, Any]) -> tuple[str, str, str | None]:
    if old["kind"] != new["kind"] or old["signature"] != new["signature"]:
        return "modified", "breaking", new.get("migration")
    old_version, new_version = semver(old["version"], "old version"), semver(new["version"], "new version")
    if new_version < old_version:
        return "modified", "breaking", new.get("migration")
    if old["stability"] != "deprecated" and new["stability"] == "deprecated":
        return "deprecated", "deprecated", new.get("migration")
    if new_version[0] > old_version[0]:
        return "modified", "breaking", new.get("migration")
    if new_version > old_version or old["stability"] != new["stability"]:
        return "modified", "additive", new.get("migration")
    return "modified", "compatible", new.get("migration")


def compatibility_report(old_path: Path, new_path: Path) -> dict[str, Any]:
    old_payload, new_payload = load_json(old_path), load_json(new_path)
    old_surfaces = validate_contract(old_payload, "old")
    new_surfaces = validate_contract(new_payload, "new")
    changes: list[dict[str, Any]] = []
    for identifier in sorted(new_surfaces.keys() - old_surfaces.keys()):
        surface = new_surfaces[identifier]
        changes.append(surface_change("added", "additive", surface, migration=surface.get("migration")))
    for identifier in sorted(old_surfaces.keys() - new_surfaces.keys()):
        old_surface = old_surfaces[identifier]
        migration = new_payload.get("migrations", {}).get(identifier)
        if not migration:
            raise ValueError(f"removed public surface {identifier} requires a new-contract migration note")
        changes.append(surface_change("removed", "breaking", old_surface, old=old_surface, migration=migration))
    for identifier in sorted(old_surfaces.keys() & new_surfaces.keys()):
        old_surface, new_surface = old_surfaces[identifier], new_surfaces[identifier]
        if old_surface != new_surface:
            change, severity, migration = classify_modified(old_surface, new_surface)
            if severity in {"breaking", "deprecated"} and not migration:
                raise ValueError(f"{severity} public surface {identifier} requires a migration note")
            changes.append(surface_change(change, severity, new_surface, old=old_surface, migration=migration))
    old_compiler, new_compiler = old_payload["compiler"], new_payload["compiler"]
    old_range = (semver(old_compiler["minimum"], "old compiler minimum"), semver(old_compiler["maximum"], "old compiler maximum"))
    new_range = (semver(new_compiler["minimum"], "new compiler minimum"), semver(new_compiler["maximum"], "new compiler maximum"))
    if old_range != new_range:
        narrowed = new_range[0] > old_range[0] or new_range[1] < old_range[1]
        severity = "breaking" if narrowed else "additive"
        migration = new_compiler.get("migration")
        if severity == "breaking" and not migration:
            raise ValueError("a narrowed compiler support range requires new.compiler.migration")
        changes.append({
            "change": "modified",
            "severity": severity,
            "surface_kind": "compiler",
            "surface_id": "axiom://compiler/support-range",
            "old_version": old_compiler["minimum"],
            "new_version": new_compiler["minimum"],
            "description": f"compiler support range changed from {old_compiler['minimum']}..{old_compiler['maximum']} to {new_compiler['minimum']}..{new_compiler['maximum']}",
            "migration": migration,
        })
    old_edition, new_edition = old_payload["edition"], new_payload["edition"]
    if old_edition["id"] != new_edition["id"]:
        migration = new_edition.get("migration")
        if not migration:
            raise ValueError("an edition change requires new.edition.migration")
        edition = {"old": old_edition["id"], "new": new_edition["id"], "severity": "breaking", "migration": migration}
    elif old_edition["status"] != "deprecated" and new_edition["status"] == "deprecated":
        edition = {"old": old_edition["id"], "new": new_edition["id"], "severity": "deprecated", "migration": new_edition.get("migration")}
    else:
        edition = {"old": old_edition["id"], "new": new_edition["id"], "severity": "compatible", "migration": new_edition.get("migration")}
    rank = {"breaking": 0, "deprecated": 1, "additive": 2, "compatible": 3}
    changes.sort(key=lambda change: (rank[change["severity"]], change["surface_kind"], change["surface_id"], change["change"]))
    summary = {severity: sum(change["severity"] == severity for change in changes) for severity in ("breaking", "additive", "deprecated", "compatible")}
    return {"schema_version": REPORT_SCHEMA, "ok": True, "command": "compatibility-report", "old": str(old_path), "new": str(new_path), "edition": edition, "summary": summary, "changes": changes}


def main() -> int:
    args = parse_args()
    try:
        schema = load_json(args.schema_file)
        if schema.get("properties", {}).get("schema_version", {}).get("const") != REPORT_SCHEMA:
            raise ValueError(f"published report schema {args.schema_file} does not pin {REPORT_SCHEMA}")
        report = compatibility_report(args.old, args.new)
    except ValueError as error:
        report = {"schema_version": REPORT_SCHEMA, "ok": False, "command": "compatibility-report", "error": str(error)}
        print(json.dumps(report, indent=2, sort_keys=True) if args.json else f"compatibility report: fail\n- {error}")
        return 1
    print(json.dumps(report, indent=2, sort_keys=True) if args.json else f"compatibility report: pass ({len(report['changes'])} changes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
