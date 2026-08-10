#!/usr/bin/env python3
"""Validate policy-bound public contracts and report deterministic AxiOM drift."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections import Counter
from datetime import date
from pathlib import Path
from typing import Any

from compatibility_v1_common import SEMVER, reject_rust_detail
from json_schema_v1 import validate_draft_2020_12


ROOT = Path(os.environ.get("AXIOM_CHECKOUT_PATH", Path(__file__).resolve().parents[2])).resolve()
PUBLIC_CONTRACT_SCHEMA = "axiom.public_contract.v1"
POLICY_SCHEMA = "axiom.compatibility_policy.v1"
REPORT_SCHEMA = "axiom.compatibility_report.v1"
POLICY_SCHEMA_FILE = ROOT / "stage1/schemas/axiom-compatibility-policy-v1.schema.json"
KINDS = ("language", "stdlib", "cli", "package", "abi", "schema", "artifact")
REPORT_KINDS = KINDS
KIND_RANK = {kind: index for index, kind in enumerate(KINDS)}
STABILITIES = {"experimental", "stable", "deprecated"}
EDITION_STATUSES = {"experimental", "supported", "deprecated"}
AXIOM_ID = re.compile(r"^axiom://[A-Za-z0-9._~:/#@!$&'()*+,;=%-]+$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
CONTRACT_KEYS = {
    "schema_version",
    "policy_version",
    "contract_version",
    "snapshot_id",
    "edition",
    "compiler",
    "surfaces",
    "migrations",
}
EDITION_KEYS = {"id", "status", "source", "migration", "replacement"}
COMPILER_KEYS = {"current", "minimum", "maximum", "source", "migration"}
SURFACE_KEYS = {
    "id",
    "kind",
    "version",
    "stability",
    "signature",
    "sources",
    "migration",
    "replacement",
    "deprecation",
}
SOURCE_KEYS = {"path", "role", "selector", "sha256"}
MIGRATION_KEYS = {"action", "replacement", "removed_in", "removed_on"}
DEPRECATION_KEYS = {"announced_on", "remove_after", "supported_editions"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old", required=True, type=Path)
    parser.add_argument("--new", required=True, type=Path)
    parser.add_argument(
        "--policy",
        type=Path,
        default=ROOT / "stage1/compatibility/policy-v1.json",
    )
    parser.add_argument(
        "--old-policy",
        type=Path,
        help="historical policy for --old; defaults to policy.json beside the old contract",
    )
    parser.add_argument(
        "--policy-schema",
        type=Path,
        default=POLICY_SCHEMA_FILE,
    )
    parser.add_argument(
        "--schema-file",
        type=Path,
        default=ROOT / "stage1/schemas/axiom-compatibility-report-v1.schema.json",
    )
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


def reject_unknown(value: dict[str, Any], allowed: set[str], label: str) -> None:
    unknown = sorted(set(value) - allowed)
    if unknown:
        raise ValueError(f"{label} contains unknown properties: {', '.join(unknown)}")


def require_string(
    value: Any,
    label: str,
    pattern: re.Pattern[str] | None = None,
) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{label} must be a non-empty string")
    if pattern is not None and not pattern.fullmatch(value):
        raise ValueError(f"{label} has invalid format: {value!r}")
    return value


def semver(value: Any, label: str) -> tuple[int, int, int]:
    return tuple(map(int, require_string(value, label, SEMVER).split(".")))  # type: ignore[return-value]


def canonical_date(value: Any, label: str) -> date:
    text = require_string(value, label)
    try:
        parsed = date.fromisoformat(text)
    except ValueError as error:
        raise ValueError(f"{label} must be an ISO 8601 date") from error
    if parsed.isoformat() != text:
        raise ValueError(f"{label} must be a canonical ISO 8601 date")
    return parsed


def reject_rust_capture(value: str, label: str) -> None:
    reject_rust_detail(value, label)


def validate_policy(
    policy: dict[str, Any],
    schema: dict[str, Any] | None = None,
) -> None:
    published_schema = load_json(POLICY_SCHEMA_FILE) if schema is None else schema
    try:
        validate_draft_2020_12(policy, published_schema)
    except ValueError as error:
        raise ValueError(f"policy schema violation: {error}") from error
    if policy.get("schema_version") != POLICY_SCHEMA:
        raise ValueError(f"policy must use {POLICY_SCHEMA}")
    semver(policy.get("policy_version"), "policy.policy_version")
    release = policy.get("release_state")
    if not isinstance(release, dict):
        raise ValueError("policy.release_state must be an object")
    if release.get("phase") != "pre_1_0_unreleased" or release.get(
        "published_compiler_releases"
    ) is not False:
        raise ValueError("policy must honestly record the pre-1.0 unreleased compiler state")
    semver_rules = policy.get("semver")
    if not isinstance(semver_rules, dict) or set(semver_rules) != STABILITIES:
        raise ValueError("policy.semver must define stable, experimental, and deprecated rules")
    expected_bumps = {
        "stable": ("major", "minor", "patch"),
        "experimental": ("any_higher", "any_higher", "any_higher"),
        "deprecated": ("major", "minor", "patch"),
    }
    for stability, (breaking, additive, compatible) in expected_bumps.items():
        rule = semver_rules.get(stability)
        if not isinstance(rule, dict):
            raise ValueError(f"policy.semver.{stability} must be an object")
        if (
            rule.get("breaking"),
            rule.get("additive"),
            rule.get("compatible"),
        ) != (breaking, additive, compatible):
            raise ValueError(f"policy.semver.{stability} has unsupported bump semantics")
        require_string(rule.get("requirements"), f"policy.semver.{stability}.requirements")
    deprecation = policy.get("deprecation")
    if not isinstance(deprecation, dict) or deprecation != {
        "minimum_days": 180,
        "minimum_supported_editions": 2,
        "removal_requires_major": True,
        "migration_required": True,
        "replacement_required": True,
    }:
        raise ValueError(
            "policy.deprecation must require 180 days, two supported editions, a major removal, migration, and replacement"
        )
    editions = policy.get("editions")
    if not isinstance(editions, dict) or editions.get("selection") != "not_implemented":
        raise ValueError("policy must record that manifest edition selection is not implemented")
    require_string(editions.get("implementation_status"), "policy.editions.implementation_status")
    if editions.get("lifecycle") != [
        "experimental",
        "supported",
        "deprecated",
        "removed",
    ]:
        raise ValueError(
            "policy.editions.lifecycle must be exactly experimental, supported, deprecated, removed"
        )
    compiler = policy.get("compiler_support")
    if not isinstance(compiler, dict):
        raise ValueError("policy.compiler_support must be an object")
    minimum = semver(compiler.get("minimum"), "policy.compiler_support.minimum")
    maximum = semver(compiler.get("maximum"), "policy.compiler_support.maximum")
    current = semver(compiler.get("current"), "policy.compiler_support.current")
    if not minimum <= current <= maximum:
        raise ValueError("policy current compiler must lie inside its support range")
    expected_line = f"{current[0]}.{current[1]}.x"
    if compiler.get("maintenance_line") != expected_line:
        raise ValueError(
            f"policy compiler maintenance_line must be {expected_line} for the current compiler"
        )
    previous = compiler.get("previous_supported")
    if not isinstance(previous, dict) or previous.get("status") != "unavailable":
        raise ValueError("policy must not invent a previous supported compiler")
    require_string(previous.get("reason"), "policy.compiler_support.previous_supported.reason")
    supported_editions = editions.get("supported")
    if not isinstance(supported_editions, list):
        raise ValueError("policy.editions.supported must be an array")
    supported_ids = [
        row.get("id") for row in supported_editions if isinstance(row, dict)
    ]
    if supported_ids != sorted(supported_ids) or len(supported_ids) != len(set(supported_ids)):
        raise ValueError("policy.editions.supported IDs must be unique and deterministically sorted")
    edition_rows = {
        row.get("id"): row
        for row in supported_editions
        if isinstance(row, dict) and isinstance(row.get("id"), str)
    }
    current_edition = editions.get("current")
    if current_edition not in edition_rows:
        raise ValueError("policy current edition must appear in editions.supported")
    support_matrix = policy.get("support_matrix")
    if not isinstance(support_matrix, list):
        raise ValueError("policy.support_matrix must be an array")
    expected_support_matrix = [
        {
            "compiler": compiler.get("current"),
            "edition": current_edition,
            "edition_status": "policy_only_not_selectable",
            "maintenance": "development",
            "status": "current",
        },
        {
            "compiler": "none",
            "edition": current_edition,
            "edition_status": "policy_only_not_selectable",
            "maintenance": "none",
            "status": "unavailable",
        },
    ]
    if support_matrix != expected_support_matrix:
        raise ValueError(
            "policy.support_matrix must exactly model the unique current compiler/edition and unavailable previous compiler"
        )
    evolution = policy.get("evolution")
    required_evolution = {
        "language",
        "stdlib",
        "cli",
        "package_manifest",
        "lockfile",
        "schema",
        "abi",
        "artifact",
    }
    if not isinstance(evolution, dict) or set(evolution) != required_evolution:
        raise ValueError("policy.evolution does not cover every public contract domain")
    cli = evolution["cli"]
    schema = evolution["schema"]
    abi = evolution["abi"]
    for value, keys, label in (
        (cli, {"identity", "additive", "breaking", "machine_output", "human_output"}, "cli"),
        (
            schema,
            {
                "identity",
                "additive",
                "breaking",
                "producer_compatibility",
                "consumer_compatibility",
            },
            "schema",
        ),
        (abi, {"identity", "additive", "breaking", "physical_layout"}, "abi"),
    ):
        if not isinstance(value, dict) or set(value) != keys:
            raise ValueError(f"policy.evolution.{label} is incomplete")


def validate_source(value: Any, label: str) -> tuple[str, str, str, str]:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    reject_unknown(value, SOURCE_KEYS, label)
    path = require_string(value.get("path"), f"{label}.path")
    if Path(path).is_absolute() or ".." in Path(path).parts:
        raise ValueError(f"{label}.path must be repository-relative")
    role = require_string(value.get("role"), f"{label}.role", re.compile(r"^[a-z][a-z0-9_]*$"))
    selector = require_string(value.get("selector"), f"{label}.selector")
    digest = require_string(value.get("sha256"), f"{label}.sha256", SHA256)
    return path, role, selector, digest


def validate_migration(value: Any, label: str, *, removal: bool) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    allowed = MIGRATION_KEYS if removal else {"action", "replacement"}
    reject_unknown(value, allowed, label)
    require_string(value.get("action"), f"{label}.action")
    reject_rust_capture(value["action"], f"{label}.action")
    if "replacement" in value:
        require_string(value["replacement"], f"{label}.replacement", AXIOM_ID)
    if removal:
        semver(value.get("removed_in"), f"{label}.removed_in")
        canonical_date(value.get("removed_on"), f"{label}.removed_on")
        require_string(value.get("replacement"), f"{label}.replacement", AXIOM_ID)
    return value


def validate_deprecation(value: Any, label: str, policy: dict[str, Any]) -> None:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    reject_unknown(value, DEPRECATION_KEYS, label)
    announced = canonical_date(value.get("announced_on"), f"{label}.announced_on")
    remove_after = canonical_date(value.get("remove_after"), f"{label}.remove_after")
    minimum_days = policy["deprecation"]["minimum_days"]
    if (remove_after - announced).days < minimum_days:
        raise ValueError(f"{label} must retain the surface for at least {minimum_days} days")
    editions = value.get("supported_editions")
    minimum_editions = policy["deprecation"]["minimum_supported_editions"]
    if (
        not isinstance(editions, list)
        or len(editions) < minimum_editions
        or len(editions) != len(set(editions))
        or not all(isinstance(item, str) and re.fullmatch(r"[0-9]{4}", item) for item in editions)
    ):
        raise ValueError(
            f"{label}.supported_editions must name at least {minimum_editions} unique editions"
        )
    if editions != sorted(editions):
        raise ValueError(f"{label}.supported_editions must be deterministically sorted")
    governed_editions = {
        row["id"]
        for row in policy["editions"]["supported"]
        if isinstance(row, dict) and isinstance(row.get("id"), str)
    }
    unknown_editions = sorted(set(editions) - governed_editions)
    if unknown_editions:
        raise ValueError(
            f"{label}.supported_editions are absent from selected policy history: "
            + ", ".join(unknown_editions)
        )


def validate_contract(
    payload: dict[str, Any],
    label: str,
    policy: dict[str, Any],
    *,
    current_policy: bool = False,
) -> dict[str, dict[str, Any]]:
    reject_unknown(payload, CONTRACT_KEYS, label)
    if payload.get("schema_version") != PUBLIC_CONTRACT_SCHEMA:
        raise ValueError(f"{label} must use {PUBLIC_CONTRACT_SCHEMA}")
    if payload.get("policy_version") != policy.get("policy_version"):
        raise ValueError(f"{label}.policy_version must match the selected policy")
    semver(payload.get("contract_version"), f"{label}.contract_version")
    require_string(payload.get("snapshot_id"), f"{label}.snapshot_id", AXIOM_ID)

    edition = payload.get("edition")
    if not isinstance(edition, dict):
        raise ValueError(f"{label}.edition must be an object")
    reject_unknown(edition, EDITION_KEYS, f"{label}.edition")
    require_string(edition.get("id"), f"{label}.edition.id", re.compile(r"^[0-9]{4}$"))
    if edition.get("status") not in EDITION_STATUSES:
        raise ValueError(f"{label}.edition.status must be one of {sorted(EDITION_STATUSES)}")
    validate_source(edition.get("source"), f"{label}.edition.source")
    if edition.get("status") == "deprecated":
        validate_migration(edition.get("migration"), f"{label}.edition.migration", removal=False)
        replacement = require_string(
            edition.get("replacement"),
            f"{label}.edition.replacement",
            re.compile(r"^[0-9]{4}$"),
        )
        if replacement == edition["id"]:
            raise ValueError(f"{label}.edition.replacement must name a different edition")
    elif "replacement" in edition:
        raise ValueError(f"{label}.edition.replacement is only valid for deprecated editions")

    compiler = payload.get("compiler")
    if not isinstance(compiler, dict):
        raise ValueError(f"{label}.compiler must be an object")
    reject_unknown(compiler, COMPILER_KEYS, f"{label}.compiler")
    current = semver(compiler.get("current"), f"{label}.compiler.current")
    minimum = semver(compiler.get("minimum"), f"{label}.compiler.minimum")
    maximum = semver(compiler.get("maximum"), f"{label}.compiler.maximum")
    if not minimum <= current <= maximum:
        raise ValueError(f"{label}.compiler.current must lie inside minimum..maximum")
    validate_source(compiler.get("source"), f"{label}.compiler.source")
    if "migration" in compiler:
        validate_migration(compiler["migration"], f"{label}.compiler.migration", removal=False)
    if current_policy:
        selected = policy["compiler_support"]
        for field in ("current", "minimum", "maximum"):
            if compiler[field] != selected[field]:
                raise ValueError(
                    f"{label}.compiler.{field} must match selected policy.compiler_support.{field}"
                )
        policy_editions = {
            row["id"]: row for row in policy["editions"]["supported"] if isinstance(row, dict)
        }
        selected_edition = policy["editions"]["current"]
        if edition["id"] != selected_edition:
            raise ValueError(f"{label}.edition.id must match selected policy current edition")
        if edition["status"] != policy_editions[selected_edition]["status"]:
            raise ValueError(f"{label}.edition.status must match selected policy edition status")

    migrations = payload.get("migrations")
    if not isinstance(migrations, dict):
        raise ValueError(f"{label}.migrations must be an object")
    for identifier, migration in sorted(migrations.items()):
        require_string(identifier, f"{label}.migrations key", AXIOM_ID)
        validate_migration(migration, f"{label}.migrations[{identifier!r}]", removal=True)

    surfaces = payload.get("surfaces")
    if not isinstance(surfaces, list) or not surfaces:
        raise ValueError(f"{label}.surfaces must be a non-empty array")
    indexed: dict[str, dict[str, Any]] = {}
    previous_order: tuple[int, str] | None = None
    for index, surface in enumerate(surfaces):
        prefix = f"{label}.surfaces[{index}]"
        if not isinstance(surface, dict):
            raise ValueError(f"{prefix} must be an object")
        reject_unknown(surface, SURFACE_KEYS, prefix)
        identifier = require_string(surface.get("id"), f"{prefix}.id", AXIOM_ID)
        if identifier in indexed:
            raise ValueError(f"{label} duplicates public surface {identifier}")
        kind = surface.get("kind")
        if kind not in KINDS:
            raise ValueError(f"{prefix}.kind must be one of {list(KINDS)}")
        order = (KIND_RANK[kind], identifier)
        if previous_order is not None and previous_order > order:
            raise ValueError(f"{label}.surfaces must be deterministically sorted by kind and id")
        previous_order = order
        version = semver(surface.get("version"), f"{prefix}.version")
        stability = surface.get("stability")
        if stability not in STABILITIES:
            raise ValueError(f"{prefix}.stability must be one of {sorted(STABILITIES)}")
        if stability in {"stable", "deprecated"} and version[0] < 1:
            raise ValueError(f"{prefix} stable/deprecated surfaces must start at version 1.0.0 or later")
        signature = require_string(surface.get("signature"), f"{prefix}.signature")
        reject_rust_capture(identifier, f"{prefix}.id")
        reject_rust_capture(signature, f"{prefix}.signature")
        sources = surface.get("sources")
        if not isinstance(sources, list) or not sources:
            raise ValueError(f"{prefix}.sources must be a non-empty array")
        source_keys = [
            validate_source(item, f"{prefix}.sources[{source_index}]")
            for source_index, item in enumerate(sources)
        ]
        if source_keys != sorted(source_keys) or len(source_keys) != len(set(source_keys)):
            raise ValueError(f"{prefix}.sources must be unique and deterministically sorted")
        if stability == "deprecated":
            migration = validate_migration(
                surface.get("migration"),
                f"{prefix}.migration",
                removal=False,
            )
            replacement = require_string(
                surface.get("replacement"),
                f"{prefix}.replacement",
                AXIOM_ID,
            )
            if migration.get("replacement") not in {None, replacement}:
                raise ValueError(f"{prefix}.migration replacement contradicts surface replacement")
            validate_deprecation(surface.get("deprecation"), f"{prefix}.deprecation", policy)
        elif "replacement" in surface or "deprecation" in surface:
            raise ValueError(
                f"{prefix}.replacement/deprecation are only valid for deprecated surfaces"
            )
        elif "migration" in surface:
            validate_migration(surface["migration"], f"{prefix}.migration", removal=False)
        indexed[identifier] = surface
    for identifier, surface in indexed.items():
        if surface["stability"] == "deprecated":
            replacement = surface["replacement"]
            if replacement == identifier or replacement not in indexed:
                raise ValueError(
                    f"{label} deprecated surface {identifier} replacement must name a different surviving surface"
                )
            seen = {identifier}
            cursor = replacement
            while indexed[cursor]["stability"] == "deprecated":
                if cursor in seen:
                    raise ValueError(f"{label} deprecated replacement graph contains a cycle")
                seen.add(cursor)
                cursor = indexed[cursor]["replacement"]
                if cursor not in indexed:
                    raise ValueError(
                        f"{label} deprecated replacement chain must terminate at a surviving surface"
                    )
            if indexed[cursor]["stability"] == "deprecated":
                raise ValueError(
                    f"{label} deprecated replacement chain must terminate at a non-deprecated surface"
                )
    counts = Counter(surface["kind"] for surface in surfaces)
    missing = [kind for kind in KINDS if counts[kind] == 0]
    if missing:
        raise ValueError(f"{label} missing required surface kinds: {', '.join(missing)}")
    return indexed


def surface_semantics(surface: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in surface.items() if key != "sources"}


def migration_action(value: Any) -> str | None:
    return value.get("action") if isinstance(value, dict) else None


def bump_kind(old: tuple[int, int, int], new: tuple[int, int, int]) -> str:
    if new <= old:
        return "none"
    if new[0] > old[0]:
        return "major"
    if new[1] > old[1]:
        return "minor"
    return "patch"


def classify_modified(
    old: dict[str, Any],
    new: dict[str, Any],
) -> tuple[str, str, str | None]:
    old_version = semver(old["version"], "old version")
    new_version = semver(new["version"], "new version")
    bump = bump_kind(old_version, new_version)
    if bump == "none":
        raise ValueError(f"changed public surface {new['id']} must increase its version")
    old_stability = old["stability"]
    new_stability = new["stability"]
    signature_changed = old["kind"] != new["kind"] or old["signature"] != new["signature"]
    migration = migration_action(new.get("migration"))

    if old_stability == "deprecated":
        if signature_changed:
            raise ValueError(f"deprecated public surface {new['id']} must retain its signature")
        if new_stability != "deprecated":
            raise ValueError(f"deprecated public surface {new['id']} cannot return to {new_stability}")
    if old_stability == "stable" and new_stability == "experimental":
        raise ValueError(f"stable public surface {new['id']} cannot become experimental")
    if old_stability != "deprecated" and new_stability == "deprecated":
        if signature_changed:
            raise ValueError(f"public surface {new['id']} cannot change signature while deprecating")
        expected = "minor" if old_stability == "stable" else "any_higher"
        if expected == "minor" and bump != "minor":
            raise ValueError(f"deprecating stable public surface {new['id']} requires a minor bump")
        return "deprecated", "deprecated", migration
    if signature_changed:
        if old_stability == "stable" and bump != "major":
            raise ValueError(f"stable signature change {new['id']} requires a major bump")
        return "modified", "breaking", migration
    if bump == "major":
        return "modified", "breaking", migration
    if old_stability == "experimental" and new_stability == "stable":
        return "modified", "additive", migration
    if bump == "minor":
        return "modified", "additive", migration
    return "modified", "compatible", migration


def change_record(
    change: str,
    severity: str,
    surface: dict[str, Any],
    *,
    old: dict[str, Any] | None = None,
    migration: str | None = None,
    replacement: str | None = None,
) -> dict[str, Any]:
    result: dict[str, Any] = {
        "change": change,
        "description": f"{change} {surface['kind']} surface {surface['id']}",
        "migration": migration,
        "severity": severity,
        "surface_id": surface["id"],
        "surface_kind": surface["kind"],
    }
    if old is not None:
        result["old_version"] = old["version"]
    if change != "removed":
        result["new_version"] = surface["version"]
    if replacement is not None:
        result["replacement"] = replacement
    return result


def compatibility_report(
    old_path: Path,
    new_path: Path,
    policy_path: Path,
    old_policy_path: Path | None = None,
    policy_schema_path: Path = POLICY_SCHEMA_FILE,
) -> dict[str, Any]:
    new_policy = load_json(policy_path)
    historical_policy_path = old_policy_path
    if historical_policy_path is None:
        adjacent_policy = old_path.parent / "policy.json"
        historical_policy_path = adjacent_policy if adjacent_policy.is_file() else policy_path
    old_policy = load_json(historical_policy_path)
    policy_schema = load_json(policy_schema_path)
    validate_policy(old_policy, policy_schema)
    validate_policy(new_policy, policy_schema)
    old_policy_version = semver(old_policy["policy_version"], "old policy.policy_version")
    new_policy_version = semver(new_policy["policy_version"], "new policy.policy_version")
    if new_policy_version < old_policy_version:
        raise ValueError("new policy_version must not be lower than old policy_version")
    old_policy_semantics = {key: value for key, value in old_policy.items() if key != "transition"}
    new_policy_semantics = {key: value for key, value in new_policy.items() if key != "transition"}
    policy_drift = old_policy_semantics != new_policy_semantics
    if policy_drift and new_policy_version <= old_policy_version:
        raise ValueError("policy semantic drift requires an increased new policy_version")
    transition = new_policy.get("transition")
    if policy_drift:
        if not isinstance(transition, dict):
            raise ValueError(
                "unclassified policy semantic drift is breaking and requires transition metadata"
            )
        if transition.get("from") != old_policy["policy_version"]:
            raise ValueError("policy.transition.from must equal the old policy_version")
        policy_severity = transition.get("severity")
        if policy_severity not in {"compatible", "additive", "deprecated", "breaking"}:
            raise ValueError("policy.transition.severity is invalid")
        policy_migration = transition.get("migration")
        if policy_severity in {"breaking", "deprecated"}:
            require_string(policy_migration, "policy.transition.migration")
        elif policy_migration is not None:
            raise ValueError(
                "policy.transition.migration is only allowed for breaking or deprecated drift"
            )
    else:
        if transition is not None:
            raise ValueError("policy.transition requires semantic policy drift")
        policy_severity = "compatible"
        policy_migration = None
    old_payload = load_json(old_path)
    new_payload = load_json(new_path)
    old_surfaces = validate_contract(old_payload, "old", old_policy, current_policy=True)
    new_surfaces = validate_contract(new_payload, "new", new_policy, current_policy=True)
    old_contract_version = semver(old_payload["contract_version"], "old.contract_version")
    new_contract_version = semver(new_payload["contract_version"], "new.contract_version")
    changes: list[dict[str, Any]] = []
    removed_surface_ids = set(old_surfaces) - set(new_surfaces)
    migration_ids = set(new_payload["migrations"])
    if migration_ids != removed_surface_ids:
        missing = sorted(removed_surface_ids - migration_ids)
        unused = sorted(migration_ids - removed_surface_ids)
        details = []
        if missing:
            details.append("missing: " + ", ".join(missing))
        if unused:
            details.append("unused: " + ", ".join(unused))
        raise ValueError(
            "new.migrations keys must exactly equal removed public surfaces"
            + (f" ({'; '.join(details)})" if details else "")
        )

    for identifier in sorted(new_surfaces.keys() - old_surfaces.keys()):
        surface = new_surfaces[identifier]
        if migration_action(surface.get("migration")):
            raise ValueError(f"added public surface {identifier} must not declare migration")
        changes.append(
            change_record(
                "added",
                "additive",
                surface,
                migration=migration_action(surface.get("migration")),
            )
        )
    for identifier in sorted(old_surfaces.keys() - new_surfaces.keys()):
        old_surface = old_surfaces[identifier]
        if old_surface["stability"] != "deprecated":
            raise ValueError(
                f"removed public surface {identifier} must be deprecated before removal"
            )
        removal = new_payload["migrations"].get(identifier)
        if not isinstance(removal, dict):
            raise ValueError(
                f"removed public surface {identifier} requires structured new-contract removal metadata"
            )
        removed_in = semver(removal.get("removed_in"), f"removal {identifier}.removed_in")
        if removal.get("removed_in") != new_payload["contract_version"]:
            raise ValueError(
                f"removed public surface {identifier} removed_in must equal new.contract_version"
            )
        removed_on = canonical_date(
            removal.get("removed_on"),
            f"removal {identifier}.removed_on",
        )
        remove_after = canonical_date(
            old_surface["deprecation"]["remove_after"],
            f"old surface {identifier}.deprecation.remove_after",
        )
        if removed_on < remove_after:
            raise ValueError(
                f"removed public surface {identifier} cannot be removed before remove_after"
            )
        if new_contract_version[0] <= old_contract_version[0]:
            raise ValueError(
                f"removed public surface {identifier} requires a major contract_version bump"
            )
        if removal.get("replacement") != old_surface.get("replacement"):
            raise ValueError(f"removed public surface {identifier} must preserve its replacement")
        if removal["replacement"] not in new_surfaces:
            raise ValueError(
                f"removed public surface {identifier} replacement must survive in the new contract"
            )
        changes.append(
            change_record(
                "removed",
                "breaking",
                old_surface,
                old=old_surface,
                migration=removal["action"],
                replacement=removal["replacement"],
            )
        )
    for identifier in sorted(old_surfaces.keys() & new_surfaces.keys()):
        old_surface = old_surfaces[identifier]
        new_surface = new_surfaces[identifier]
        if surface_semantics(old_surface) == surface_semantics(new_surface):
            continue
        change, severity, migration = classify_modified(old_surface, new_surface)
        if severity in {"breaking", "deprecated"} and not migration:
            raise ValueError(f"{severity} public surface {identifier} requires a migration action")
        if severity not in {"breaking", "deprecated"} and migration:
            raise ValueError(
                f"{severity} public surface {identifier} must not declare migration"
            )
        changes.append(
            change_record(
                change,
                severity,
                new_surface,
                old=old_surface,
                migration=migration,
                replacement=new_surface.get("replacement"),
            )
        )

    old_compiler = old_payload["compiler"]
    new_compiler = new_payload["compiler"]
    old_range = (
        semver(old_compiler["minimum"], "old compiler minimum"),
        semver(old_compiler["maximum"], "old compiler maximum"),
    )
    new_range = (
        semver(new_compiler["minimum"], "new compiler minimum"),
        semver(new_compiler["maximum"], "new compiler maximum"),
    )
    compiler_migration = migration_action(new_compiler.get("migration"))
    narrowed = new_range[0] > old_range[0] or new_range[1] < old_range[1]
    expanded = new_range[0] < old_range[0] or new_range[1] > old_range[1]
    compiler_severity = "breaking" if narrowed else "additive" if expanded else "compatible"
    compiler_changed = (
        old_compiler["current"],
        old_compiler["minimum"],
        old_compiler["maximum"],
    ) != (
        new_compiler["current"],
        new_compiler["minimum"],
        new_compiler["maximum"],
    )
    if compiler_changed:
        if compiler_severity == "breaking" and not compiler_migration:
            raise ValueError("a narrowed compiler support range requires new.compiler.migration")
        if compiler_severity != "breaking" and compiler_migration:
            raise ValueError(
                "new.compiler.migration is only allowed for a breaking compiler support change"
            )
    elif compiler_migration:
        raise ValueError("new.compiler.migration requires a breaking compiler support change")

    old_edition = old_payload["edition"]
    new_edition = new_payload["edition"]
    edition_migration = migration_action(new_edition.get("migration"))
    edition_rank = {"experimental": 0, "supported": 1, "deprecated": 2}
    if (
        old_edition["id"] == new_edition["id"]
        and edition_rank[new_edition["status"]] < edition_rank[old_edition["status"]]
    ):
        raise ValueError(
            f"edition {old_edition['id']} cannot regress from {old_edition['status']} to {new_edition['status']}"
        )
    if old_edition["id"] != new_edition["id"]:
        if not edition_migration:
            raise ValueError("an edition change requires new.edition.migration")
        edition = {
            "migration": edition_migration,
            "new": new_edition["id"],
            "old": old_edition["id"],
            "severity": "breaking",
        }
    elif old_edition["status"] != "deprecated" and new_edition["status"] == "deprecated":
        if not edition_migration:
            raise ValueError("a deprecated edition requires new.edition.migration")
        edition = {
            "migration": edition_migration,
            "new": new_edition["id"],
            "old": old_edition["id"],
            "replacement": new_edition.get("replacement"),
            "severity": "deprecated",
        }
    else:
        edition = {
            "migration": edition_migration,
            "new": new_edition["id"],
            "old": old_edition["id"],
            "severity": "compatible",
        }

    rank = {"breaking": 0, "deprecated": 1, "additive": 2, "compatible": 3}
    changes.sort(
        key=lambda item: (
            rank[item["severity"]],
            REPORT_KINDS.index(item["surface_kind"]),
            item["surface_id"],
            item["change"],
        )
    )
    summary = {
        severity: sum(item["severity"] == severity for item in changes)
        for severity in ("breaking", "additive", "deprecated", "compatible")
    }
    edition_drift = (
        old_edition["id"],
        old_edition["status"],
    ) != (
        new_edition["id"],
        new_edition["status"],
    )
    has_drift = bool(changes) or edition_drift or compiler_changed or policy_drift
    if new_contract_version < old_contract_version:
        raise ValueError("new.contract_version must not be lower than old.contract_version")
    if has_drift and new_contract_version <= old_contract_version:
        raise ValueError("semantic drift requires an increased new.contract_version")
    if has_drift:
        severities = {item["severity"] for item in changes}
        severities.add(edition["severity"])
        if compiler_changed:
            severities.add(compiler_severity)
        if policy_drift:
            severities.add(policy_severity)
        if old_contract_version[0] > 0:
            if "breaking" in severities and new_contract_version[0] <= old_contract_version[0]:
                raise ValueError("breaking drift requires a major contract_version bump")
            if (
                "breaking" not in severities
                and ({"additive", "deprecated"} & severities)
                and not (
                    new_contract_version[0] > old_contract_version[0]
                    or new_contract_version[1] > old_contract_version[1]
                )
            ):
                raise ValueError("additive/deprecated drift requires a minor contract_version bump")
        elif "breaking" in severities and not (
            new_contract_version[0] > old_contract_version[0]
            or new_contract_version[1] > old_contract_version[1]
        ):
            raise ValueError("pre-1.0 breaking drift requires a minor contract_version bump")
    return {
        "changes": changes,
        "command": "compatibility-report",
        "compiler": {
            "migration": compiler_migration,
            "new": {
                "current": new_compiler["current"],
                "maximum": new_compiler["maximum"],
                "minimum": new_compiler["minimum"],
            },
            "old": {
                "current": old_compiler["current"],
                "maximum": old_compiler["maximum"],
                "minimum": old_compiler["minimum"],
            },
            "severity": compiler_severity,
        },
        "contracts": {
            "new": new_payload["contract_version"],
            "old": old_payload["contract_version"],
        },
        "edition": edition,
        "new": new_payload["snapshot_id"],
        "ok": True,
        "old": old_payload["snapshot_id"],
        "policies": {
            "new": new_policy["policy_version"],
            "old": old_policy["policy_version"],
            "severity": policy_severity,
            "migration": policy_migration,
        },
        "schema_version": REPORT_SCHEMA,
        "summary": summary,
    }


def main() -> int:
    args = parse_args()
    try:
        schema = load_json(args.schema_file)
        success_properties = (
            schema.get("$defs", {}).get("success", {}).get("properties", {})
        )
        failure_properties = (
            schema.get("$defs", {}).get("failure", {}).get("properties", {})
        )
        if (
            success_properties.get("schema_version", {}).get("const") != REPORT_SCHEMA
            or failure_properties.get("schema_version", {}).get("const") != REPORT_SCHEMA
        ):
            raise ValueError(f"published report schema {args.schema_file} does not pin {REPORT_SCHEMA}")
        report = compatibility_report(
            args.old,
            args.new,
            args.policy,
            args.old_policy,
            args.policy_schema,
        )
        try:
            validate_draft_2020_12(report, schema)
        except ValueError as error:
            raise ValueError(f"generated report schema violation: {error}") from error
    except ValueError as error:
        failure = {
            "command": "compatibility-report",
            "error": str(error),
            "ok": False,
            "schema_version": REPORT_SCHEMA,
        }
        print(
            json.dumps(failure, indent=2, sort_keys=True)
            if args.json
            else f"compatibility report: fail\n- {error}"
        )
        return 1
    print(
        json.dumps(report, indent=2, sort_keys=True)
        if args.json
        else f"compatibility report: pass ({len(report['changes'])} changes)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
