#!/usr/bin/env python3
"""Derive the current target-neutral AxiOM public contract from governed sources."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any

from compatibility_v1_common import SEMVER, reject_rust_detail


ROOT = Path(__file__).resolve().parents[2]
INVENTORY = ROOT / "stage1/compatibility/source-inventory-v1.json"
POLICY = ROOT / "stage1/compatibility/policy-v1.json"
CURRENT = ROOT / "stage1/compatibility/fixtures/current/contract.json"
KINDS = ("language", "stdlib", "cli", "package", "abi", "schema", "artifact")
KIND_RANK = {kind: index for index, kind in enumerate(KINDS)}
AXIOM_ID = re.compile(r"^axiom://[A-Za-z0-9._~:/#@!$&'()*+,;=%-]+$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, default=INVENTORY)
    parser.add_argument("--policy", type=Path, default=POLICY)
    parser.add_argument("--output", type=Path, default=CURRENT)
    parser.add_argument("--check", action="store_true", help="fail unless --output is byte-identical")
    parser.add_argument("--json", action="store_true", help="print the generated contract")
    return parser.parse_args()


def load_object(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read {path}: {error}") from error
    if not isinstance(payload, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return payload


def relative(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError as error:
        raise ValueError(f"source path escapes repository: {path}") from error


def source(path: Path, role: str, selector: str) -> dict[str, str]:
    if not path.is_file():
        raise ValueError(f"required public-contract source is missing: {relative(path)}")
    return {
        "path": relative(path),
        "role": role,
        "selector": selector,
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }


def canonical(payload: dict[str, Any]) -> bytes:
    return (json.dumps(payload, indent=2, sort_keys=True) + "\n").encode("utf-8")


def semantic_digest(value: Any) -> str:
    encoded = json.dumps(value, separators=(",", ":"), sort_keys=True).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def manifest_semantic_projection(
    schema: dict[str, Any],
    parser_contract: dict[str, Any],
) -> dict[str, Any]:
    if set(parser_contract) != {
        "schema_version",
        "dependency_version_pattern",
        "test_kinds",
        "test_capabilities",
    }:
        raise ValueError("manifest parser contract must be a closed semantic object")
    if (
        parser_contract.get("schema_version")
        != "axiom.compatibility_manifest_parser.v1"
    ):
        raise ValueError("manifest parser contract has an unsupported schema_version")
    try:
        test_properties = schema["properties"]["tests"]["items"]["properties"]
        dependency_pattern = schema["properties"]["dependencies"][
            "additionalProperties"
        ]["oneOf"][1]["properties"]["version"]["pattern"]
    except (KeyError, IndexError, TypeError) as error:
        raise ValueError("manifest schema lacks parser parity fields") from error
    test_capabilities = parser_contract.get("test_capabilities")
    if not isinstance(test_capabilities, dict) or set(test_capabilities) != {
        "mode",
        "names",
    }:
        raise ValueError("manifest parser test_capabilities contract is invalid")
    if (
        test_properties["kind"].get("enum") != parser_contract.get("test_kinds")
        or test_capabilities.get("mode") != "unsupported_empty_only"
        or test_properties["capabilities"].get("maxItems") != 0
        or test_properties["capabilities"]["items"].get("enum")
        != test_capabilities.get("names")
        or dependency_pattern != parser_contract.get("dependency_version_pattern")
    ):
        raise ValueError("manifest schema contradicts the governed parser contract")
    return {
        "parser_contract": parser_contract,
        "schema": {
            key: value
            for key, value in schema.items()
            if key not in {"$schema", "title", "description"}
        },
    }


def abi_semantic_projection(
    contract: dict[str, Any],
    readiness: dict[str, Any],
) -> dict[str, Any]:
    if set(contract) != {
        "schema_version",
        "abi_id",
        "value_features",
        "capability_shims",
    }:
        raise ValueError("logical ABI contract must be a closed semantic object")
    if contract.get("schema_version") != "axiom.compatibility_abi_surface.v1":
        raise ValueError("logical ABI contract has an unsupported schema_version")
    abi_id = contract.get("abi_id")
    if not isinstance(abi_id, str) or not AXIOM_ID.fullmatch(abi_id):
        raise ValueError("logical ABI contract requires an AxiOM abi_id")
    reject_rust_detail(abi_id, "logical ABI abi_id")
    value_features = contract.get("value_features")
    capability_shims = contract.get("capability_shims")
    if not isinstance(value_features, list) or not isinstance(capability_shims, list):
        raise ValueError("logical ABI contract must define value_features and capability_shims")
    projection: list[dict[str, Any]] = []
    for category, rows in (
        ("value_feature", value_features),
        ("capability_shim", capability_shims),
    ):
        for index, row in enumerate(rows):
            if not isinstance(row, dict):
                raise ValueError(f"logical ABI {category}[{index}] must be an object")
            expected_keys = (
                {"id", "logical_semantics"}
                if category == "value_feature"
                else {"id", "capability", "logical_semantics"}
            )
            if set(row) != expected_keys:
                raise ValueError(f"logical ABI {category}[{index}] fields drifted")
            identifier = row.get("id")
            if not isinstance(identifier, str) or not identifier:
                raise ValueError(f"logical ABI {category}[{index}] requires a semantic ID")
            meaning = row.get("logical_semantics")
            if not isinstance(meaning, str) or not meaning:
                raise ValueError(f"logical ABI {identifier} requires logical_semantics")
            reject_rust_detail(identifier, f"logical ABI {identifier} id")
            reject_rust_detail(meaning, f"logical ABI {identifier} logical_semantics")
            projected = {
                "category": category,
                "id": identifier,
                "logical_semantics": meaning,
            }
            if category == "capability_shim":
                capability = row.get("capability")
                if capability is not None and (
                    not isinstance(capability, str) or not capability
                ):
                    raise ValueError(
                        f"logical ABI capability shim {identifier} has an invalid capability"
                    )
                if capability is not None:
                    reject_rust_detail(capability, f"logical ABI {identifier} capability")
                projected["capability"] = capability
            projection.append(projected)
    projection.sort(key=lambda row: (row["id"], row["category"]))
    identifiers = [row["id"] for row in projection]
    if len(identifiers) != len(set(identifiers)):
        raise ValueError("logical ABI semantic IDs must be unique across row categories")

    readiness_projection: list[dict[str, Any]] = []
    for category, key in (
        ("value_feature", "value_features"),
        ("capability_shim", "capability_shims"),
    ):
        rows = readiness.get(key)
        if not isinstance(rows, list):
            raise ValueError(f"direct-native readiness must define {key}")
        for index, row in enumerate(rows):
            if not isinstance(row, dict):
                raise ValueError(f"direct-native readiness {key}[{index}] must be an object")
            identifier = row.get("id")
            if not isinstance(identifier, str) or not identifier:
                raise ValueError(f"direct-native readiness {key}[{index}] requires an ID")
            projected = {"category": category, "id": identifier}
            if category == "capability_shim":
                capability = row.get("capability")
                if capability is not None and (
                    not isinstance(capability, str) or not capability
                ):
                    raise ValueError(
                        f"direct-native readiness capability shim {identifier} has an invalid capability"
                    )
                projected["capability"] = capability
            readiness_projection.append(projected)
    readiness_projection.sort(key=lambda row: (row["id"], row["category"]))
    if len(readiness_projection) != len(
        {(row["category"], row["id"]) for row in readiness_projection}
    ):
        raise ValueError("direct-native readiness ABI rows must be unique")
    parity_projection = [
        {
            key: row[key]
            for key in ("category", "id", "capability")
            if key in row
        }
        for row in projection
    ]
    if readiness_projection != parity_projection:
        raise ValueError(
            "direct-native readiness ABI IDs/categories/capabilities contradict the logical ABI contract"
        )
    return {
        "abi_id": abi_id,
        "rows": projection,
        "schema_version": contract["schema_version"],
    }


def artifact_semantic_projection(payload: dict[str, Any]) -> dict[str, Any]:
    projection = copy.deepcopy(
        {
            key: value
            for key, value in payload.items()
            if key not in {"$schema", "title", "description"}
        }
    )
    try:
        kinds = projection["$defs"]["artifact"]["properties"]["kind"]["enum"]
    except (KeyError, TypeError) as error:
        raise ValueError("artifact schema must enumerate semantic artifact kinds") from error
    if not isinstance(kinds, list) or not all(isinstance(kind, str) and kind for kind in kinds):
        raise ValueError("artifact schema must enumerate semantic artifact kinds")
    target_neutral = sorted(
        kind for kind in kinds if not kind.startswith("legacy_generated_")
    )
    if not target_neutral:
        raise ValueError("artifact schema must publish a target-neutral artifact kind")
    projection["$defs"]["artifact"]["properties"]["kind"]["enum"] = target_neutral
    return projection


def require_semver(value: Any, label: str) -> str:
    if not isinstance(value, str) or not SEMVER.fullmatch(value):
        raise ValueError(f"{label} must be canonical SemVer")
    return value


def kebab_case(value: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "-", value).lower()


def rust_enum_variants(path: Path, enum_name: str) -> list[str]:
    """Read only public variant names for parity; never expose Rust types/layout."""
    text = path.read_text(encoding="utf-8")
    match = re.search(rf"(?m)^enum\s+{re.escape(enum_name)}\s*\{{", text)
    if match is None:
        raise ValueError(f"CLI implementation source has no enum {enum_name}")
    depth = 1
    position = match.end()
    body_end = position
    while position < len(text) and depth:
        char = text[position]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
        position += 1
        body_end = position - 1
    if depth:
        raise ValueError(f"CLI implementation enum {enum_name} is not balanced")
    body = text[match.end() : body_end]
    variants = [
        kebab_case(name)
        for name in re.findall(r"(?m)^    ([A-Z][A-Za-z0-9]*)\b", body)
    ]
    if not variants:
        raise ValueError(f"CLI implementation enum {enum_name} has no variants")
    return sorted(variants)


def inventory_entries(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    if payload.get("schema_version") != "axiom.compatibility_source_inventory.v1":
        raise ValueError("source inventory must use axiom.compatibility_source_inventory.v1")
    entries = payload.get("sources")
    if not isinstance(entries, list):
        raise ValueError("source inventory sources must be an array")
    indexed: dict[str, dict[str, Any]] = {}
    observed_kinds: set[str] = set()
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            raise ValueError(f"source inventory sources[{index}] must be an object")
        extractor = entry.get("extractor")
        kind = entry.get("kind")
        path = entry.get("path")
        if not all(isinstance(value, str) and value for value in (extractor, kind, path)):
            raise ValueError(f"source inventory sources[{index}] requires extractor, kind, and path")
        if extractor in indexed:
            raise ValueError(f"source inventory duplicates extractor {extractor}")
        if kind != "compiler" and kind not in KINDS:
            raise ValueError(f"source inventory has unknown kind {kind}")
        indexed[extractor] = entry
        observed_kinds.add(kind)
    missing = (set(KINDS) | {"compiler"}) - observed_kinds
    if missing:
        raise ValueError("source inventory missing required kinds: " + ", ".join(sorted(missing)))
    return indexed


def entry_path(entries: dict[str, dict[str, Any]], extractor: str) -> Path:
    try:
        path = entries[extractor]["path"]
    except KeyError as error:
        raise ValueError(f"source inventory missing extractor {extractor}") from error
    candidate = ROOT / path
    if not candidate.exists():
        raise ValueError(f"required public-contract source is missing: {path}")
    return candidate


def surface(
    identifier: str,
    kind: str,
    version: str,
    signature: str,
    sources: list[dict[str, str]],
    *,
    stability: str = "experimental",
    migration: dict[str, str] | None = None,
) -> dict[str, Any]:
    if not AXIOM_ID.fullmatch(identifier):
        raise ValueError(f"invalid AxiOM surface identity {identifier!r}")
    if kind not in KINDS:
        raise ValueError(f"invalid AxiOM surface kind {kind!r}")
    require_semver(version, f"{identifier}.version")
    reject_rust_detail(identifier, f"surface {identifier}.id")
    reject_rust_detail(signature, f"surface {identifier}.signature")
    ordered_sources = sorted(
        sources,
        key=lambda item: (item["path"], item["role"], item["selector"], item["sha256"]),
    )
    source_keys = [
        (item["path"], item["role"], item["selector"]) for item in ordered_sources
    ]
    if len(source_keys) != len(set(source_keys)):
        raise ValueError(f"surface {identifier} duplicates source provenance")
    result = {
        "id": identifier,
        "kind": kind,
        "signature": signature,
        "sources": ordered_sources,
        "stability": stability,
        "version": version,
    }
    if migration is not None:
        if set(migration) != {"action"} or not isinstance(migration["action"], str) or not migration["action"].strip():
            raise ValueError(f"{identifier}.migration must contain one non-empty action")
        result["migration"] = {"action": migration["action"].strip()}
    return result


def extract(inventory_path: Path, policy_path: Path) -> dict[str, Any]:
    inventory = load_object(inventory_path)
    policy = load_object(policy_path)
    entries = inventory_entries(inventory)
    contract_version = require_semver(inventory.get("contract_version"), "contract_version")
    version_policy = inventory.get("surface_versions")
    if not isinstance(version_policy, dict):
        raise ValueError("source inventory surface_versions must be an object")
    default_surface_version = require_semver(
        version_policy.get("default"), "surface_versions.default"
    )
    surface_version_overrides = version_policy.get("overrides")
    if not isinstance(surface_version_overrides, dict) or not all(
        isinstance(identifier, str) and isinstance(version, str)
        for identifier, version in surface_version_overrides.items()
    ):
        raise ValueError("surface_versions.overrides must map surface IDs to SemVer strings")
    for identifier, version in surface_version_overrides.items():
        if not AXIOM_ID.fullmatch(identifier):
            raise ValueError(f"invalid surface version override ID {identifier!r}")
        require_semver(version, f"surface_versions.overrides[{identifier!r}]")
    surface_migrations = inventory.get("surface_migrations", {})
    if not isinstance(surface_migrations, dict) or not all(
        isinstance(identifier, str) and isinstance(migration, dict)
        for identifier, migration in surface_migrations.items()
    ):
        raise ValueError("source inventory surface_migrations must map IDs to migration objects")

    def public_surface_version(identifier: str) -> str:
        return surface_version_overrides.get(identifier, default_surface_version)

    def public_surface_migration(identifier: str) -> dict[str, str] | None:
        migration = surface_migrations.get(identifier)
        return migration if migration is not None else None

    policy_version = require_semver(policy.get("policy_version"), "policy.policy_version")

    cargo_path = entry_path(entries, "cargo_workspace_version")
    cargo = tomllib.loads(cargo_path.read_text(encoding="utf-8"))
    try:
        compiler_version = require_semver(cargo["workspace"]["package"]["version"], "workspace compiler version")
    except (KeyError, TypeError) as error:
        raise ValueError("stage1/Cargo.toml must define workspace.package.version") from error
    support = policy.get("compiler_support")
    if not isinstance(support, dict):
        raise ValueError("policy.compiler_support must be an object")
    minimum = require_semver(support.get("minimum"), "policy.compiler_support.minimum")
    maximum = require_semver(support.get("maximum"), "policy.compiler_support.maximum")
    if compiler_version != support.get("current"):
        raise ValueError(
            f"policy current compiler {support.get('current')!r} does not match Cargo {compiler_version}"
        )
    if tuple(map(int, minimum.split("."))) > tuple(map(int, maximum.split("."))):
        raise ValueError("policy compiler minimum must not exceed maximum")
    compiler_source = source(cargo_path, "compiler_version", "workspace.package.version")

    edition_path = entry_path(entries, "current_edition")
    editions = policy.get("editions")
    if not isinstance(editions, dict):
        raise ValueError("policy.editions must be an object")
    edition_id = editions.get("current")
    supported = editions.get("supported")
    if not isinstance(edition_id, str) or not re.fullmatch(r"[0-9]{4}", edition_id):
        raise ValueError("policy current edition must be four digits")
    if not isinstance(supported, list):
        raise ValueError("policy supported editions must be an array")
    edition_row = next(
        (row for row in supported if isinstance(row, dict) and row.get("id") == edition_id),
        None,
    )
    if edition_row is None:
        raise ValueError("policy current edition must appear in editions.supported")
    edition_source = source(edition_path, "edition_policy", "editions.current")
    language_related = entries["current_edition"].get("related")
    if (
        not isinstance(language_related, list)
        or not language_related
        or not all(isinstance(path, str) and path for path in language_related)
    ):
        raise ValueError("language source entry must name governed semantic snapshots")
    language_semantics: list[dict[str, Any]] = []
    language_sources = [edition_source]
    for related_path in sorted(language_related):
        path = ROOT / related_path
        snapshot = load_object(path)
        language_semantics.append(snapshot)
        language_sources.append(
            source(path, "language_semantic_contract", "schema_version,contract and semantic fields")
        )

    surfaces: list[dict[str, Any]] = []
    surfaces.append(
        surface(
            f"axiom://language/edition/{edition_id}",
            "language",
            public_surface_version(f"axiom://language/edition/{edition_id}"),
            (
                f"edition {edition_id}; status={edition_row['status']}; "
                f"selection={editions['selection']}; coverage=governed_partial; "
                f"language_semantic_digest={semantic_digest(language_semantics)}"
            ),
            language_sources,
        )
    )

    stdlib_path = entry_path(entries, "stdlib_catalog")
    stdlib = load_object(stdlib_path)
    modules = stdlib.get("modules")
    if not isinstance(modules, list) or not modules:
        raise ValueError("stdlib catalog must contain modules")
    module_names: list[str] = []
    stdlib_semantics: list[dict[str, Any]] = []
    for module in modules:
        if not isinstance(module, dict) or not isinstance(module.get("name"), str):
            raise ValueError("stdlib catalog modules require semantic names")
        symbols = module.get("symbols")
        if not isinstance(symbols, list):
            raise ValueError(f"stdlib module {module['name']} symbols must be an array")
        module_id = module.get("module_id")
        if not isinstance(module_id, str) or not AXIOM_ID.fullmatch(module_id):
            raise ValueError(f"stdlib module {module['name']} requires an AxiOM module_id")
        projected_symbols: list[dict[str, str]] = []
        for symbol in symbols:
            if not isinstance(symbol, dict):
                raise ValueError(f"stdlib module {module['name']} has a non-object symbol")
            projection = {
                key: symbol.get(key)
                for key in ("name", "signature", "effect", "binding", "binding_kind")
            }
            if not all(isinstance(value, str) and value for value in projection.values()):
                raise ValueError(
                    f"stdlib module {module['name']} symbols require name, signature, effect, binding, and binding_kind"
                )
            projected_symbols.append(projection)  # type: ignore[arg-type]
        if projected_symbols != sorted(projected_symbols, key=lambda item: (item["name"], item["signature"])):
            raise ValueError(f"stdlib module {module['name']} symbols are not deterministically sorted")
        module_names.append(module["name"])
        stdlib_semantics.append(
            {
                "module_id": module_id,
                "name": module["name"],
                "capabilities": module.get("capabilities", []),
                "symbols": projected_symbols,
            }
        )
    if module_names != sorted(module_names):
        raise ValueError("stdlib catalog modules are not deterministically sorted")
    surfaces.append(
        surface(
            "axiom://stdlib/catalog",
            "stdlib",
            require_semver(stdlib.get("catalog_version"), "stdlib catalog version"),
            (
                f"{stdlib.get('schema_version')}; catalog_semantic_digest="
                f"{semantic_digest(stdlib_semantics)}; modules={len(modules)}; "
                f"symbols={sum(len(module['symbols']) for module in stdlib_semantics)}"
            ),
            [source(stdlib_path, "stdlib_catalog", "modules[*].name,symbols[*].signature")],
        )
    )

    cli_path = entry_path(entries, "cli_commands")
    cli_contract = load_object(cli_path)
    command_paths = cli_contract.get("command_paths")
    if not isinstance(command_paths, list) or not all(
        isinstance(path, str) and path for path in command_paths
    ):
        raise ValueError("CLI surface must contain semantic command_paths")
    if command_paths != sorted(command_paths) or len(command_paths) != len(set(command_paths)):
        raise ValueError("CLI command paths must be unique and deterministically sorted")

    related_value = entries["cli_commands"].get("related")
    schema_value = entries["cli_commands"].get("schema")
    implementation_value = entries["cli_commands"].get("implementation")
    if not all(
        isinstance(value, str) and value
        for value in (related_value, schema_value, implementation_value)
    ):
        raise ValueError("cli source entry must name related, schema, and implementation sources")
    ledger_path = ROOT / related_value
    cli_schema_path = ROOT / schema_value
    implementation_path = ROOT / implementation_value
    ledger = load_object(ledger_path)
    commands = ledger.get("commands")
    if not isinstance(commands, list) or not commands:
        raise ValueError("capability ledger must contain public commands")
    command_names = [command.get("name") for command in commands if isinstance(command, dict)]
    if len(command_names) != len(commands) or not all(isinstance(name, str) and name for name in command_names):
        raise ValueError("capability ledger commands require semantic names")
    if command_names != sorted(command_names) or len(command_names) != len(set(command_names)):
        raise ValueError("capability ledger commands must be unique and deterministically sorted")
    if sorted({path.split()[0] for path in command_paths}) != command_names:
        raise ValueError("CLI surface top-level commands drifted from the capability ledger")
    implementation_paths = set(rust_enum_variants(implementation_path, "Command"))
    nested_enums = cli_contract.get("nested_enums")
    if not isinstance(nested_enums, dict):
        raise ValueError("CLI surface nested_enums must be an object")
    for parent, enum_name in sorted(nested_enums.items()):
        if not isinstance(parent, str) or not isinstance(enum_name, str):
            raise ValueError("CLI nested enum mappings must contain strings")
        implementation_paths.update(
            f"{parent} {variant}"
            for variant in rust_enum_variants(implementation_path, enum_name)
        )
    if implementation_paths != set(command_paths):
        missing_paths = sorted(implementation_paths - set(command_paths))
        extra_paths = sorted(set(command_paths) - implementation_paths)
        raise ValueError(
            "CLI surface drifted from compiler command graph"
            f"; missing={missing_paths}; extra={extra_paths}"
        )
    cli_semantics = inventory.get("cli_semantics")
    if not isinstance(cli_semantics, dict):
        raise ValueError("source inventory cli_semantics must be an object")
    cli_flags = cli_contract.get("flags")
    if not isinstance(cli_flags, dict) or cli_flags.get("coverage") != "partial":
        raise ValueError("CLI flag coverage must explicitly record its current partial status")
    cli_migration = cli_contract.get("migration")
    if (
        not isinstance(cli_migration, dict)
        or set(cli_migration) != {"action"}
        or not isinstance(cli_migration.get("action"), str)
        or not cli_migration["action"].strip()
    ):
        raise ValueError("CLI surface migration must contain one non-empty action")
    cli_surface = surface(
        "axiom://cli/axiomc",
        "cli",
        public_surface_version("axiom://cli/axiomc"),
        (
            f"commands={','.join(command_paths)}; "
            f"command_semantic_digest={semantic_digest(cli_contract)}; "
            f"flags_status=experimental_partial; "
            f"flags={cli_semantics.get('flags')}; "
            f"exit={cli_semantics.get('exit_status')}; json={cli_semantics.get('json_envelope')}"
        ),
        [
            source(cli_path, "cli_public_surface", "command_paths,flags,exit_status,migration"),
            source(cli_schema_path, "cli_json_envelope", "$id,oneOf"),
            source(ledger_path, "cli_top_level_parity", "commands[*].name"),
            source(implementation_path, "cli_command_graph_parity", "Command and nested command variant names"),
        ],
    )
    cli_surface["migration"] = {"action": cli_migration["action"].strip()}
    surfaces.append(cli_surface)

    manifest_path = entry_path(entries, "package_manifest")
    manifest_schema = load_object(manifest_path)
    manifest_implementation_value = entries["package_manifest"].get("implementation")
    if not isinstance(manifest_implementation_value, str) or not manifest_implementation_value:
        raise ValueError("package manifest source entry must name its parser implementation")
    manifest_implementation = ROOT / manifest_implementation_value
    parser_contract_value = entries["package_manifest"].get("parser_contract")
    if not isinstance(parser_contract_value, str) or not parser_contract_value:
        raise ValueError("package manifest source entry must name its parser contract")
    parser_contract_path = ROOT / parser_contract_value
    parser_contract = load_object(parser_contract_path)
    manifest_properties = manifest_schema.get("properties")
    if not isinstance(manifest_properties, dict):
        raise ValueError("manifest schema must define properties")
    manifest_schema_source = source(
        manifest_path,
        "package_manifest_schema",
        "$id,properties",
    )
    manifest_parser_source = source(
        manifest_implementation,
        "package_manifest_parser_parity",
        "RawManifest and closed nested manifest structures",
    )
    manifest_projection = manifest_semantic_projection(
        manifest_schema,
        parser_contract,
    )
    manifest_surface = surface(
        "axiom://package/manifest",
        "package",
        public_surface_version("axiom://package/manifest"),
        (
            f"axiom.toml schema={manifest_schema.get('$id')}; "
            f"semantic_digest={semantic_digest(manifest_projection)}; "
            f"fields={','.join(sorted(manifest_properties))}; edition_selector=unavailable"
        ),
        [
            manifest_schema_source,
            source(
                parser_contract_path,
                "package_manifest_parser_contract",
                "dependency_version_pattern,test_kinds,test_capabilities",
            ),
            manifest_parser_source,
        ],
    )
    manifest_migration = entries["package_manifest"].get("migration")
    if (
        not isinstance(manifest_migration, dict)
        or set(manifest_migration) != {"action"}
        or not isinstance(manifest_migration.get("action"), str)
        or not manifest_migration["action"].strip()
    ):
        raise ValueError("package manifest source entry must contain one migration action")
    manifest_surface["migration"] = {"action": manifest_migration["action"].strip()}
    surfaces.append(manifest_surface)

    lock_path = entry_path(entries, "lockfile")
    lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    lock_version = lock.get("version")
    if not isinstance(lock_version, int) or lock_version < 1:
        raise ValueError("canonical axiom.lock fixture must declare a positive numeric version")
    lock_v2_schema_value = entries["lockfile"].get("schema_v2")
    lock_v2_fixture_value = entries["lockfile"].get("fixture_v2")
    lock_implementation_value = entries["lockfile"].get("implementation")
    if not all(
        isinstance(value, str) and value
        for value in (
            lock_v2_schema_value,
            lock_v2_fixture_value,
            lock_implementation_value,
        )
    ):
        raise ValueError(
            "lockfile source entry must name schema_v2, fixture_v2, and implementation"
        )
    lock_v2_schema_path = ROOT / lock_v2_schema_value
    lock_v2_fixture_path = ROOT / lock_v2_fixture_value
    lock_implementation_path = ROOT / lock_implementation_value
    lock_v2_schema = load_object(lock_v2_schema_path)
    lock_v2_fixture = load_object(lock_v2_fixture_path)
    if lock_v2_fixture.get("version") != 2:
        raise ValueError("canonical axiom.lock v2 fixture must declare version 2")
    lock_v2_properties = lock_v2_schema.get("properties")
    if not isinstance(lock_v2_properties, dict):
        raise ValueError("axiom.lock v2 schema must define top-level properties")
    expected_v2_fields = ["compatibility", "edge", "package", "registry", "roots", "version"]
    if sorted(lock_v2_properties) != expected_v2_fields:
        raise ValueError("axiom.lock v2 schema has an unexpected top-level field set")
    lock_v2_projection = {
        "schema": lock_v2_schema,
        "fixture": lock_v2_fixture,
    }
    lock_surface = surface(
        "axiom://package/lockfile",
        "package",
        public_surface_version("axiom://package/lockfile"),
        (
            f"axiom.lock formats={lock_version},2; "
            "v1_records=package(name,version,source); "
            f"v2_fields={','.join(expected_v2_fields)}; "
            f"v2_semantic_digest={semantic_digest(lock_v2_projection)}"
        ),
        [
            source(lock_path, "lockfile_v1_format", "version,package[*]"),
            source(
                lock_v2_schema_path,
                "lockfile_v2_schema",
                "$id,required,properties,$defs",
            ),
            source(
                lock_v2_fixture_path,
                "lockfile_v2_fixture",
                "version,compatibility,roots,registry,package,edge",
            ),
            source(
                lock_implementation_path,
                "lockfile_runtime_parity",
                "LockfileV2 and validate_lockfile_v2",
            ),
        ],
    )
    lock_migration = entries["lockfile"].get("migration")
    if (
        not isinstance(lock_migration, dict)
        or set(lock_migration) != {"action"}
        or not isinstance(lock_migration.get("action"), str)
        or not lock_migration["action"].strip()
    ):
        raise ValueError("lockfile source entry must contain one non-empty migration action")
    lock_surface["migration"] = {"action": lock_migration["action"].strip()}
    surfaces.append(lock_surface)

    abi_path = entry_path(entries, "logical_abi")
    abi = load_object(abi_path)
    readiness_value = entries["logical_abi"].get("readiness")
    if not isinstance(readiness_value, str) or not readiness_value:
        raise ValueError("logical ABI source entry must name readiness parity evidence")
    readiness_path = ROOT / readiness_value
    readiness = load_object(readiness_path)
    abi_projection = abi_semantic_projection(abi, readiness)
    abi_ids = [row["id"] for row in abi_projection["rows"]]
    abi_surface = surface(
        "axiom://abi/direct-native",
        "abi",
        public_surface_version("axiom://abi/direct-native"),
        (
            f"{abi.get('schema_version')}; abi_id={abi.get('abi_id')}; semantic_digest="
            f"{semantic_digest(abi_projection)}; "
            f"semantic_rows={','.join(sorted(abi_ids))}"
        ),
        [
            source(
                abi_path,
                "logical_runtime_abi",
                "abi_id,value_features[*].(id,logical_semantics),capability_shims[*].(id,capability,logical_semantics)",
            ),
            source(
                readiness_path,
                "runtime_abi_readiness_parity",
                "value_features[*].id,capability_shims[*].(id,capability)",
            ),
        ],
    )
    abi_migration = public_surface_migration("axiom://abi/direct-native")
    if abi_migration is not None:
        abi_surface["migration"] = abi_migration
    surfaces.append(abi_surface)

    schema_roots = [
        entry_path(entries, "published_schemas"),
        entry_path(entries, "published_compiler_contract_schemas"),
    ]
    schema_files = sorted(
        (
            schema_path
            for schema_root in schema_roots
            for schema_path in schema_root.glob("*.schema.json")
        ),
        key=relative,
    )
    if not schema_files:
        raise ValueError("published schema roots contain no schemas")
    observed_schema_ids: set[str] = set()
    observed_surface_ids: set[str] = set()
    for schema_path in schema_files:
        schema = load_object(schema_path)
        schema_id = schema.get("$id")
        if not isinstance(schema_id, str) or not schema_id:
            raise ValueError(f"published schema {relative(schema_path)} has no $id")
        if schema_id in observed_schema_ids:
            raise ValueError(f"published schemas duplicate $id {schema_id}")
        observed_schema_ids.add(schema_id)
        semantic_name = schema_path.name.removesuffix(".schema.json")
        identifier = f"axiom://schema/{semantic_name}"
        if identifier in observed_surface_ids:
            raise ValueError(f"published schemas duplicate derived surface ID {identifier}")
        observed_surface_ids.add(identifier)
        schema_version = (
            schema.get("properties", {})
            .get("schema_version", {})
            .get("const")
            if isinstance(schema.get("properties"), dict)
            else None
        )
        semantic_schema = {
            key: value
            for key, value in schema.items()
            if key not in {"$schema", "title", "description"}
        }
        signature = (
            f"published schema id={schema_id}; "
            f"semantic_digest={semantic_digest(semantic_schema)}"
        )
        if isinstance(schema_version, str):
            signature += f"; envelope={schema_version}"
        surfaces.append(
            surface(
                identifier,
                "schema",
                public_surface_version(identifier),
                signature,
                [source(schema_path, "published_schema", "$id,properties.schema_version.const")],
                migration=public_surface_migration(identifier),
            )
        )

    artifact_path = entry_path(entries, "artifact_envelope")
    artifact_schema = load_object(artifact_path)
    artifact_projection = artifact_semantic_projection(artifact_schema)
    target_neutral_artifact_kinds = artifact_projection["$defs"]["artifact"]["properties"][
        "kind"
    ]["enum"]
    surfaces.append(
        surface(
            "axiom://artifact/envelope",
            "artifact",
            public_surface_version("axiom://artifact/envelope"),
            (
                f"schema={artifact_schema.get('$id')}; "
                f"semantic_digest={semantic_digest(artifact_projection)}; "
                f"target_neutral_kinds={','.join(target_neutral_artifact_kinds)}; "
                f"identity=axiom artifact IDs; compatibility_host_source_kind=excluded"
            ),
            [source(artifact_path, "artifact_envelope", "$id,$defs.artifact.properties.kind.enum")],
        )
    )

    surfaces.sort(key=lambda item: (KIND_RANK[item["kind"]], item["id"]))
    surface_migrations = inventory.get("surface_migrations", {})
    if not isinstance(surface_migrations, dict):
        raise ValueError("source inventory surface_migrations must be an object")
    surface_by_id = {item["id"]: item for item in surfaces}
    for identifier, migration in surface_migrations.items():
        if identifier not in surface_by_id:
            raise ValueError(f"surface migration references unknown surface {identifier}")
        if (
            not isinstance(migration, dict)
            or set(migration) != {"action"}
            or not isinstance(migration.get("action"), str)
            or not migration["action"].strip()
        ):
            raise ValueError(
                f"surface migration for {identifier} must contain one non-empty action"
            )
        surface_by_id[identifier]["migration"] = {
            "action": migration["action"].strip()
        }
    observed = {item["kind"] for item in surfaces}
    missing = set(KINDS) - observed
    if missing:
        raise ValueError("generated contract missing required kinds: " + ", ".join(sorted(missing)))
    ids = [item["id"] for item in surfaces]
    if len(ids) != len(set(ids)):
        raise ValueError("generated contract contains duplicate AxiOM surface identities")
    unused_overrides = sorted(set(surface_version_overrides) - set(ids))
    if unused_overrides:
        raise ValueError(
            "surface version overrides reference unknown surfaces: "
            + ", ".join(unused_overrides)
        )

    return {
        "compiler": {
            "current": compiler_version,
            "maximum": maximum,
            "minimum": minimum,
            "source": compiler_source,
        },
        "contract_version": contract_version,
        "edition": {
            "id": edition_id,
            "source": edition_source,
            "status": edition_row["status"],
        },
        "migrations": {},
        "policy_version": policy_version,
        "schema_version": "axiom.public_contract.v1",
        "snapshot_id": inventory.get("snapshot_id"),
        "surfaces": surfaces,
    }


def main() -> int:
    args = parse_args()
    try:
        payload = extract(args.inventory, args.policy)
        encoded = canonical(payload)
        if args.check:
            try:
                actual = args.output.read_bytes()
            except OSError as error:
                raise ValueError(f"cannot read generated contract {args.output}: {error}") from error
            if actual != encoded:
                raise ValueError(
                    f"{args.output} does not match source extraction; regenerate it with this script"
                )
        if args.json or not args.check:
            sys.stdout.buffer.write(encoded)
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"public contract extraction: fail\n- {error}", file=sys.stderr)
        return 1
    if args.check and not args.json:
        print("public contract extraction: pass")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
