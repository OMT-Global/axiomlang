#!/usr/bin/env python3
"""Validate the compiler.package_graph boundary fixture without Cargo."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "axiom.compiler.package_graph.v1"
CONTRACT = "compiler.package_graph"
DEFAULT_SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.compiler.package_graph.v1.schema.json")
DEFAULT_SNAPSHOT = Path("stage1/compiler-contracts/snapshots/package-graph.json")
DEFAULT_RUNTIME_SCHEMA = Path(
    "stage1/compiler-contracts/schemas/axiom.compiler.package_graph.runtime.v1.schema.json"
)
DEFAULT_RUNTIME_SNAPSHOT = Path(
    "stage1/compiler-contracts/snapshots/package-graph-runtime.json"
)
DEFAULT_MANIFEST_SCHEMA = Path("stage1/schemas/axiom.toml.schema.json")
DEFAULT_MANIFEST_FIXTURE = Path("stage1/package-resolver/fixtures/manifest-registry.json")
DEFAULT_LOCKFILE_V2_SCHEMA = Path("stage1/schemas/axiom-lockfile-v2.schema.json")
DEFAULT_LOCKFILE_V2_FIXTURE = Path("stage1/package-resolver/fixtures/lockfile-v2.json")
DEFAULT_RESOLUTION_SCHEMA = Path("stage1/schemas/axiom-package-resolution-v1.schema.json")
DEFAULT_RESOLUTION_FIXTURE = Path("stage1/package-resolver/fixtures/resolution-v1.json")


def load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def load_toml(path: Path) -> dict[str, Any]:
    data: dict[str, Any] = {}
    current: dict[str, Any] = data
    current_array: list[dict[str, Any]] | None = None

    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("[[") and line.endswith("]]"):
            section = line[2:-2].strip()
            current_array = data.setdefault(section, [])
            if not isinstance(current_array, list):
                fail(f"mixed TOML table types in {path}: {section}")
            current = {}
            current_array.append(current)
            continue
        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1].strip()
            current_array = None
            current = data.setdefault(section, {})
            if not isinstance(current, dict):
                fail(f"mixed TOML table types in {path}: {section}")
            continue
        if "=" not in line:
            fail(f"unsupported TOML line in {path}: {raw_line}")
        key, value = [part.strip() for part in line.split("=", 1)]
        current[key] = parse_toml_value(value)
    return data


def parse_toml_value(value: str) -> Any:
    value = value.strip()
    if value.startswith('"') and value.endswith('"'):
        return value[1:-1]
    if value in {"true", "false"}:
        return value == "true"
    if value.startswith("[") and value.endswith("]"):
        inner = value[1:-1].strip()
        if not inner:
            return []
        return [parse_toml_value(part.strip()) for part in inner.split(",")]
    if value.startswith("{") and value.endswith("}"):
        inner = value[1:-1].strip()
        result: dict[str, Any] = {}
        if not inner:
            return result
        for part in inner.split(","):
            key, nested = [item.strip() for item in part.split("=", 1)]
            result[key] = parse_toml_value(nested)
        return result
    if value.isdigit():
        return int(value)
    return value


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def validate_against_schema(value: Any, schema: dict[str, Any]) -> None:
    errors = schema_errors(value, schema, schema)
    require(not errors, errors[0] if errors else "schema validation failed")


def schema_type_matches(value: Any, expected: str) -> bool:
    return {
        "object": isinstance(value, dict),
        "array": isinstance(value, list),
        "string": isinstance(value, str),
        "integer": isinstance(value, int) and not isinstance(value, bool),
        "number": isinstance(value, (int, float)) and not isinstance(value, bool),
        "boolean": isinstance(value, bool),
        "null": value is None,
    }.get(expected, False)


def json_equal(left: Any, right: Any) -> bool:
    if isinstance(left, bool) or isinstance(right, bool):
        return isinstance(left, bool) and isinstance(right, bool) and left == right
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        return left == right
    return type(left) is type(right) and left == right


def schema_errors(
    value: Any,
    schema: Any,
    root: dict[str, Any],
    path: str = "$",
) -> list[str]:
    if not isinstance(schema, dict):
        return [f"{path} schema node must be an object"]
    if "$ref" in schema:
        ref = schema["$ref"]
        prefix = "#/$defs/"
        if not isinstance(ref, str) or not ref.startswith(prefix):
            return [f"{path} uses unsupported schema ref {ref!r}"]
        name = ref[len(prefix):]
        target = root.get("$defs", {}).get(name)
        if not isinstance(target, dict):
            return [f"{path} references unknown schema def {name}"]
        return schema_errors(value, target, root, path)

    errors: list[str] = []
    expected = schema.get("type")
    expected_types = expected if isinstance(expected, list) else [expected] if expected else []
    if expected_types and not any(
        isinstance(item, str) and schema_type_matches(value, item)
        for item in expected_types
    ):
        return [f"{path} must have type {' or '.join(map(str, expected_types))}"]

    if "const" in schema and not json_equal(value, schema["const"]):
        errors.append(f"{path} must equal {schema['const']!r}")
    if "enum" in schema and not any(json_equal(value, item) for item in schema["enum"]):
        errors.append(f"{path} must be one of {schema['enum']!r}")

    for keyword in ("allOf",):
        branches = schema.get(keyword, [])
        if isinstance(branches, list):
            for branch in branches:
                errors.extend(schema_errors(value, branch, root, path))
    for keyword, expected_matches in (("oneOf", 1), ("anyOf", None)):
        branches = schema.get(keyword)
        if isinstance(branches, list):
            matches = sum(not schema_errors(value, branch, root, path) for branch in branches)
            if (expected_matches == 1 and matches != 1) or (
                expected_matches is None and matches == 0
            ):
                errors.append(
                    f"{path} must match "
                    + ("exactly one" if expected_matches == 1 else "at least one")
                    + f" {keyword} branch (matched {matches})"
                )
    if "not" in schema and not schema_errors(value, schema["not"], root, path):
        errors.append(f"{path} must not match the forbidden schema")
    if "if" in schema:
        condition_matches = not schema_errors(value, schema["if"], root, path)
        selected = schema.get("then") if condition_matches else schema.get("else")
        if selected is not None:
            errors.extend(schema_errors(value, selected, root, path))

    if isinstance(value, dict):
        required = schema.get("required", [])
        if isinstance(required, list):
            missing = sorted(set(required) - set(value))
            if missing:
                errors.append(f"{path} is missing required fields: {', '.join(missing)}")
        properties = schema.get("properties", {})
        if not isinstance(properties, dict):
            properties = {}
        additional = schema.get("additionalProperties")
        if additional is False:
            unexpected = sorted(set(value) - set(properties))
            if unexpected:
                errors.append(f"{path} has unexpected fields: {', '.join(unexpected)}")
        for key, nested in value.items():
            if key in properties:
                errors.extend(schema_errors(nested, properties[key], root, f"{path}.{key}"))
            elif isinstance(additional, dict):
                errors.extend(schema_errors(nested, additional, root, f"{path}.{key}"))

    if isinstance(value, list):
        minimum = schema.get("minItems")
        maximum = schema.get("maxItems")
        if isinstance(minimum, int) and len(value) < minimum:
            errors.append(f"{path} must have at least {minimum} items")
        if isinstance(maximum, int) and len(value) > maximum:
            errors.append(f"{path} must have at most {maximum} items")
        if schema.get("uniqueItems") is True:
            encoded = [json.dumps(item, sort_keys=True) for item in value]
            if len(encoded) != len(set(encoded)):
                errors.append(f"{path} items must be unique")
        items = schema.get("items")
        if isinstance(items, dict):
            for index, item in enumerate(value):
                errors.extend(schema_errors(item, items, root, f"{path}[{index}]"))

    if isinstance(value, str):
        minimum = schema.get("minLength")
        maximum = schema.get("maxLength")
        pattern = schema.get("pattern")
        if isinstance(minimum, int) and len(value) < minimum:
            errors.append(f"{path} must have at least {minimum} characters")
        if isinstance(maximum, int) and len(value) > maximum:
            errors.append(f"{path} must have at most {maximum} characters")
        if isinstance(pattern, str) and re.search(pattern, value) is None:
            errors.append(f"{path} must match {pattern!r}")

    if isinstance(value, (int, float)) and not isinstance(value, bool):
        minimum = schema.get("minimum")
        maximum = schema.get("maximum")
        if isinstance(minimum, (int, float)) and value < minimum:
            errors.append(f"{path} must be >= {minimum}")
        if isinstance(maximum, (int, float)) and value > maximum:
            errors.append(f"{path} must be <= {maximum}")
    return errors


def reject_cargo_fields(value: Any, path: str = "outputs") -> None:
    if isinstance(value, dict):
        for key, nested in value.items():
            key_lower = key.lower()
            require("cargo" not in key_lower, f"{path}.{key} must not be Cargo-derived")
            require(key not in {"Cargo.toml", "Cargo.lock"}, f"{path}.{key} must not name Cargo files")
            reject_cargo_fields(nested, f"{path}.{key}")
    elif isinstance(value, list):
        for index, nested in enumerate(value):
            reject_cargo_fields(nested, f"{path}[{index}]")
    elif isinstance(value, str):
        text = value.lower()
        require("cargo" not in text, f"{path} must not contain Cargo-derived values")
        require(value not in {"Cargo.toml", "Cargo.lock"}, f"{path} must not name Cargo files")


def package_identity(package: dict[str, Any]) -> dict[str, str]:
    return {
        "name": str(package.get("name", "")),
        "version": str(package.get("version", "")),
        "source": str(package.get("source", "")),
    }


def normalize_dependencies(raw_dependencies: dict[str, Any] | None) -> list[dict[str, str]]:
    normalized = []
    for name, spec in sorted((raw_dependencies or {}).items()):
        if isinstance(spec, str):
            normalized.append({"name": name, "path": spec})
        else:
            entry = {"name": name, "path": str(spec["path"])}
            if "version" in spec:
                entry["version"] = str(spec["version"])
            normalized.append(entry)
    return normalized


def package_key(value: dict[str, Any]) -> tuple[str, str, str, str]:
    return (
        str(value.get("registry", "")),
        str(value.get("source", "")),
        str(value.get("namespace", "")),
        str(value.get("name", "")),
    )


def require_strict_order(values: list[Any], label: str) -> None:
    require(
        all(left < right for left, right in zip(values, values[1:])),
        f"{label} must be strictly sorted and duplicate-free",
    )


def validate_lockfile_v2_fixture(lockfile: dict[str, Any]) -> None:
    registries = lockfile["registry"]
    require_strict_order([row["name"] for row in registries], "lockfile registries")
    registry_names = {row["name"] for row in registries}
    for registry in registries:
        require(
            re.fullmatch(r"[0-9a-f]{64}", registry["expectation_sha256"]) is not None,
            f"registry {registry['name']} expectation digest must be lowercase SHA-256",
        )
        require_strict_order(
            registry["index_signer_key_ids"],
            f"registry {registry['name']} index signers",
        )

    packages = lockfile["package"]
    require_strict_order([row["id"] for row in packages], "lockfile packages")
    package_by_id = {row["id"]: row for row in packages}
    for package in packages:
        if package["source"].startswith("registry:"):
            require(package["registry"] in registry_names, "registry package must name a lock registry")
            require_strict_order(
                package["signer_key_ids"],
                f"package {package['id']} signers",
            )
            require(
                package["cache_key"] == f"sha256:{package['archive_sha256']}",
                f"package {package['id']} cache key must bind its archive digest",
            )

    source_rank = {"path": 0, "registry": 1}
    reason_rank = {
        "root_path_constraint": 0,
        "transitive_path_constraint": 1,
        "highest_compatible": 2,
        "exact_locked_replay": 3,
        "trusted_yanked_locked_replay": 4,
    }
    edge_keys = [
        (
            row["from"],
            row["alias"],
            row["to"],
            row["requested"],
            source_rank[row["source_kind"]],
            reason_rank[row["reason"]],
        )
        for row in lockfile["edge"]
    ]
    require_strict_order(edge_keys, "lockfile dependency edges")
    for edge in lockfile["edge"]:
        require(edge["from"] in package_by_id, "lockfile edge source must exist")
        require(edge["to"] in package_by_id, "lockfile edge target must exist")
        expected_registry = edge["source_kind"] == "registry"
        actual_registry = package_by_id[edge["to"]]["source"].startswith("registry:")
        require(expected_registry == actual_registry, "lockfile edge source kind must match target")


def validate_resolution_fixture(resolution: dict[str, Any]) -> None:
    packages = resolution["packages"]
    require_strict_order(
        [package_key(row["package"]) for row in packages],
        "resolved packages",
    )
    for package in packages:
        require_strict_order(
            package["signer_key_ids"],
            f"resolved package {package_key(package['package'])} signers",
        )
        require(not package["yanked"], "fresh resolution fixture must not select a yank")
    require(
        any(event["event"] == "catalog_authenticated" for event in resolution["trace"]),
        "resolver trace must show authenticated catalog input",
    )
    require(
        any(
            event["event"] == "candidate_rejected"
            and event["reason"]["reason"] == "yanked"
            for event in resolution["trace"]
        ),
        "resolver trace must explain a deterministic yank rejection",
    )
    require(
        any(event["event"] == "candidate_verified" for event in resolution["trace"]),
        "resolver trace must show Package Trust verification",
    )
    require(
        any(event["event"] == "selected" for event in resolution["trace"]),
        "resolver trace must show the selected release",
    )


def validate_runtime_package_graph_fixture(graph: dict[str, Any]) -> None:
    packages = graph["packages"]
    package_by_id = {
        package["id"]: package for package in packages if isinstance(package.get("id"), str)
    }
    require(
        len(package_by_id) == sum(isinstance(package.get("id"), str) for package in packages),
        "runtime package graph package ids must be unique",
    )
    for package in packages:
        identifier = package.get("id")
        source = package.get("source")
        is_registry = (
            isinstance(identifier, str) and identifier.startswith("registry:")
        ) or (isinstance(source, str) and source.startswith("registry:"))
        if is_registry:
            require(
                isinstance(source, str) and source.startswith("registry:"),
                "runtime registry packages must expose their canonical source",
            )
            require(
                isinstance(package.get("trust"), dict),
                "runtime registry packages must expose Package Trust evidence",
            )
            trust = package["trust"]
            for field in ("index_sha256", "verification_sha256"):
                require(
                    isinstance(trust.get(field), str)
                    and re.fullmatch(r"[0-9a-f]{64}", trust[field]) is not None,
                    f"runtime registry package trust {field} must be exact lowercase SHA-256",
                )
            materialization = package.get("materialization")
            require(
                isinstance(materialization, dict)
                and materialization.get("package_trust_verified") is True,
                "runtime registry packages must expose verified materialization evidence",
            )
            lockfile = package.get("lockfile")
            require(
                isinstance(lockfile, dict)
                and lockfile.get("version") == 2
                and isinstance(lockfile.get("hash"), str)
                and re.fullmatch(r"[0-9a-f]{64}", lockfile["hash"]) is not None,
                "runtime registry packages must expose an exact SHA-256 lockfile v2 identity",
            )

        for dependency in package["dependencies"]:
            package_id = dependency.get("package_id")
            source_kind = dependency.get("source_kind")
            is_registry_edge = source_kind == "registry" or (
                isinstance(package_id, str) and package_id.startswith("registry:")
            )
            if not is_registry_edge:
                continue
            require(
                all(
                    isinstance(dependency.get(field), str) and dependency[field]
                    for field in ("package_id", "source_kind", "requested", "reason")
                ),
                "runtime registry dependency edges must expose package_id, source_kind, requested, and reason",
            )
            require(
                dependency["source_kind"] == "registry"
                and dependency["package_id"].startswith("registry:"),
                "runtime registry dependency edge identity must agree with source_kind",
            )
            require(
                dependency["package_id"] in package_by_id,
                "runtime registry dependency edge must target a package in the graph",
            )


def render_version(value: dict[str, Any]) -> str:
    return f"{value['major']}.{value['minor']}.{value['patch']}"


def render_requirement(value: dict[str, Any]) -> str:
    prefix = {"exact": "", "caret": "^"}[value["kind"]]
    return f"{prefix}{render_version(value['version'])}"


def canonical_path_package_id(package: dict[str, Any]) -> str:
    source = package["source"]
    relative = "." if source == "path" else source.removeprefix("path:")
    return f"path:{relative}#{package['name']}@{package['version']}"


def canonical_registry_package_id(
    registry: str,
    namespace: str,
    package: str,
    version: str,
) -> str:
    return f"registry:{registry}/{namespace}/{package}@{version}"


def validate_resolver_fixture_consistency(
    manifest: dict[str, Any],
    lockfile: dict[str, Any],
    resolution: dict[str, Any],
) -> None:
    manifest_package = manifest["package"]
    root_id = f"path:.#{manifest_package['name']}@{manifest_package['version']}"
    require(
        lockfile["roots"] == [root_id],
        "lockfile roots must contain the canonical manifest root package identity",
    )

    lock_packages = {package["id"]: package for package in lockfile["package"]}
    require(root_id in lock_packages, "manifest root package must exist in lockfile packages")
    root_package = lock_packages[root_id]
    require(
        root_package["name"] == manifest_package["name"]
        and root_package["version"] == manifest_package["version"]
        and root_package["source"] == "path",
        "lockfile root package identity must match the manifest package",
    )
    for package in lockfile["package"]:
        if package["source"].startswith("path"):
            require(
                package["id"] == canonical_path_package_id(package),
                f"lockfile path package {package['id']} must use its canonical package id",
            )

    manifest_registry = manifest["registry"]
    lock_registries = {registry["name"]: registry for registry in lockfile["registry"]}
    require(
        set(lock_registries) == {manifest_registry["name"]},
        "lockfile registry names must exactly match the manifest registry name",
    )
    lock_registry = lock_registries[manifest_registry["name"]]
    require(
        lock_registry["source"] == manifest_registry["index"],
        "lockfile registry source must exactly match the manifest registry index",
    )
    require(
        re.fullmatch(r"[0-9a-f]{64}", lock_registry["expectation_sha256"]) is not None,
        "lockfile registry expectation_sha256 must be a lowercase SHA-256 digest",
    )

    resolved_by_key = {
        package_key(package["package"]): package for package in resolution["packages"]
    }
    require(
        len(resolved_by_key) == len(resolution["packages"]),
        "resolution packages must have unique package coordinates",
    )
    resolved_ids: dict[tuple[str, str, str, str], str] = {}
    for key, resolved in resolved_by_key.items():
        registry, source, namespace, name = key
        require(
            registry == manifest_registry["name"],
            "resolved package registry must match the manifest and lock registry name",
        )
        require(
            source == lock_registry["source_identity"],
            "resolved package source must match the authenticated lock registry source identity",
        )
        version = render_version(resolved["version"])
        package_id = canonical_registry_package_id(registry, namespace, name, version)
        require(
            package_id in lock_packages,
            f"resolved package {package_id} must exist in lockfile packages",
        )
        locked = lock_packages[package_id]
        require(
            locked["source"] == f"registry:{registry}/{namespace}/{name}"
            and locked["registry"] == registry
            and locked["namespace"] == namespace
            and locked["name"] == name
            and locked["version"] == version,
            f"resolved package {package_id} must match its lockfile coordinates",
        )
        require(
            locked["manifest_sha256"] == resolved["manifest_digest"],
            f"resolved package {package_id} manifest digest must match the lockfile",
        )
        require(
            locked["signer_key_ids"] == resolved["signer_key_ids"],
            f"resolved package {package_id} signer identities must match the lockfile",
        )
        require(
            locked["compatibility"]["contract"] == resolved["compatibility"]
            and locked["compatibility"]["edition_policy"] == resolved["edition"],
            f"resolved package {package_id} compatibility must match the lockfile",
        )
        resolved_ids[key] = package_id

    locked_registry_ids = {
        package["id"]
        for package in lockfile["package"]
        if package["source"].startswith("registry:")
    }
    require(
        set(resolved_ids.values()) == locked_registry_ids,
        "resolved package identities must exactly match registry lockfile packages",
    )

    def source_id(value: dict[str, Any] | None) -> str:
        if value is None:
            return root_id
        key = package_key(value)
        require(key in resolved_ids, "resolution edge source must be a selected package")
        return resolved_ids[key]

    expected_edges: set[tuple[str, str, str, str, str]] = set()
    for edge in resolution["edges"]:
        target_key = package_key(edge["to"])
        require(target_key in resolved_ids, "resolution edge target must be a selected package")
        selected = render_version(edge["selected"])
        target = resolved_by_key[target_key]
        require(
            selected == render_version(target["version"]),
            "resolution edge selected version must match its selected package",
        )
        expected_edges.add(
            (
                source_id(edge["from"]),
                edge["alias"],
                resolved_ids[target_key],
                render_requirement(edge["requirement"]),
                "registry",
            )
        )

    for dependency in resolution["path_dependencies"]:
        source = source_id(dependency["from"])
        candidates = [
            package
            for package in lockfile["package"]
            if package["source"] == f"path:{dependency['path']}"
        ]
        require(
            len(candidates) == 1,
            f"resolution path {dependency['path']} must identify one lockfile package",
        )
        lock_edge = next(
            (
                edge
                for edge in lockfile["edge"]
                if edge["from"] == source
                and edge["alias"] == dependency["alias"]
                and edge["to"] == candidates[0]["id"]
            ),
            None,
        )
        require(lock_edge is not None, "resolution path dependency must exist in lockfile edges")
        expected_edges.add(
            (
                source,
                dependency["alias"],
                candidates[0]["id"],
                lock_edge["requested"],
                "path",
            )
        )

    observed_edges = {
        (
            edge["from"],
            edge["alias"],
            edge["to"],
            edge["requested"],
            edge["source_kind"],
        )
        for edge in lockfile["edge"]
    }
    require(
        observed_edges == expected_edges,
        "resolution and lockfile dependency edges must match exactly",
    )

    manifest_dependencies = manifest.get("dependencies", {})
    root_resolution_aliases = {
        edge["alias"] for edge in resolution["edges"] if edge["from"] is None
    } | {
        dependency["alias"]
        for dependency in resolution["path_dependencies"]
        if dependency["from"] is None
    }
    root_lock_aliases = {
        edge["alias"] for edge in lockfile["edge"] if edge["from"] == root_id
    }
    require(
        set(manifest_dependencies) == root_resolution_aliases == root_lock_aliases,
        "manifest, resolution, and lockfile root dependency aliases must match exactly",
    )
    for alias, dependency in manifest_dependencies.items():
        if "registry" in dependency:
            require(
                dependency["registry"] == manifest_registry["name"],
                f"manifest dependency {alias} must use the configured registry name",
            )
            matching = [
                edge
                for edge in resolution["edges"]
                if edge["from"] is None and edge["alias"] == alias
            ]
            require(len(matching) == 1, f"manifest registry dependency {alias} must resolve once")
            edge = matching[0]
            require(
                edge["to"]["registry"] == dependency["registry"]
                and edge["to"]["namespace"] == dependency["namespace"]
                and edge["to"]["name"] == dependency.get("package", alias)
                and render_requirement(edge["requirement"]) == dependency["version"],
                f"manifest registry dependency {alias} must match its resolution edge",
            )
        else:
            matching = [
                path
                for path in resolution["path_dependencies"]
                if path["from"] is None and path["alias"] == alias
            ]
            require(len(matching) == 1, f"manifest path dependency {alias} must resolve once")
            require(
                matching[0]["path"] == dependency["path"],
                f"manifest path dependency {alias} must match its resolution path",
            )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA)
    parser.add_argument("--snapshot", type=Path, default=DEFAULT_SNAPSHOT)
    parser.add_argument("--runtime-schema", type=Path, default=DEFAULT_RUNTIME_SCHEMA)
    parser.add_argument("--runtime-snapshot", type=Path, default=DEFAULT_RUNTIME_SNAPSHOT)
    parser.add_argument("--manifest-schema", type=Path, default=DEFAULT_MANIFEST_SCHEMA)
    parser.add_argument("--manifest-fixture", type=Path, default=DEFAULT_MANIFEST_FIXTURE)
    parser.add_argument("--lockfile-v2-schema", type=Path, default=DEFAULT_LOCKFILE_V2_SCHEMA)
    parser.add_argument("--lockfile-v2-fixture", type=Path, default=DEFAULT_LOCKFILE_V2_FIXTURE)
    parser.add_argument("--resolution-schema", type=Path, default=DEFAULT_RESOLUTION_SCHEMA)
    parser.add_argument("--resolution-fixture", type=Path, default=DEFAULT_RESOLUTION_FIXTURE)
    parser.add_argument("--json", action="store_true", help="emit a JSON validation result")
    args = parser.parse_args()

    schema = load_json(args.schema)
    snapshot = load_json(args.snapshot)
    runtime_schema = load_json(args.runtime_schema)
    runtime_snapshot = load_json(args.runtime_snapshot)
    manifest_schema = load_json(args.manifest_schema)
    manifest_fixture = load_json(args.manifest_fixture)
    lockfile_v2_schema = load_json(args.lockfile_v2_schema)
    lockfile_v2_fixture = load_json(args.lockfile_v2_fixture)
    resolution_schema = load_json(args.resolution_schema)
    resolution_fixture = load_json(args.resolution_fixture)

    require(schema.get("$id", "").endswith("/axiom.compiler.package_graph.v1.schema.json"), "schema $id must name package graph v1")
    require(schema.get("title") == "Axiom compiler package graph contract", "schema title changed unexpectedly")
    require(
        runtime_schema.get("$id", "").endswith(
            "/axiom.compiler.package_graph.runtime.v1.schema.json"
        ),
        "runtime schema $id must name runtime package graph v1",
    )
    require(
        runtime_schema.get("title") == "Axiom compiler runtime package graph",
        "runtime schema title changed unexpectedly",
    )
    require(
        manifest_schema.get("$id") == "https://axiom.omt.global/schemas/axiom.toml.schema.json",
        "manifest schema $id mismatch",
    )
    require(
        lockfile_v2_schema.get("$id")
        == "https://axiom.omt.global/schemas/axiom-lockfile-v2.schema.json",
        "lockfile v2 schema $id mismatch",
    )
    require(
        resolution_schema.get("$id")
        == "https://axiom.omt.global/schemas/axiom-package-resolution-v1.schema.json",
        "package resolution schema $id mismatch",
    )
    validate_against_schema(snapshot, schema)
    validate_against_schema(runtime_snapshot, runtime_schema)
    validate_against_schema(manifest_fixture, manifest_schema)
    validate_against_schema(lockfile_v2_fixture, lockfile_v2_schema)
    validate_against_schema(resolution_fixture, resolution_schema)
    validate_lockfile_v2_fixture(lockfile_v2_fixture)
    validate_resolution_fixture(resolution_fixture)
    validate_runtime_package_graph_fixture(runtime_snapshot)
    validate_resolver_fixture_consistency(
        manifest_fixture,
        lockfile_v2_fixture,
        resolution_fixture,
    )
    require(snapshot.get("schema_version") == SCHEMA_VERSION, "snapshot schema_version mismatch")
    require(snapshot.get("contract") == CONTRACT, "snapshot contract mismatch")
    reject_cargo_fields(snapshot.get("outputs", {}))

    inputs = snapshot.get("inputs", {})
    outputs = snapshot.get("outputs", {})
    root = Path(inputs.get("root", ""))
    manifest_path = Path(inputs.get("manifest", ""))
    lockfile_path = Path(inputs.get("lockfile", ""))

    for path in [root, manifest_path, lockfile_path]:
        require(path.exists(), f"fixture path does not exist: {path}")

    manifest = load_toml(manifest_path)
    lockfile = load_toml(lockfile_path)
    packages = outputs.get("packages", [])
    lock_packages = lockfile.get("package", [])
    lockfile_integrity = outputs.get("lockfile_integrity", {})

    require(outputs.get("root") == str(root), "output root must match input root")
    require(lockfile.get("version") == 1, "fixture lockfile version must be 1")
    require(lockfile_integrity.get("version") == lockfile.get("version"), "lockfile integrity version mismatch")
    require([package_identity(p) for p in packages] == [package_identity(p) for p in lock_packages], "package graph packages must match axiom.lock identity")
    require(lockfile_integrity.get("packages") == [package_identity(p) for p in lock_packages], "lockfile_integrity packages must match axiom.lock")

    root_package = packages[0]
    manifest_package = manifest.get("package", {})
    require(root_package["name"] == manifest_package.get("name"), "root package name must come from axiom.toml")
    require(root_package["version"] == manifest_package.get("version"), "root package version must come from axiom.toml")
    require(root_package["manifest"] == str(manifest_path), "root package manifest path mismatch")
    require(root_package["lockfile"] == str(lockfile_path), "root package lockfile path mismatch")

    build = manifest.get("build", {})
    require(root_package["entry"] == build.get("entry", "src/main.ax"), "root package entry must come from axiom.toml")
    require(root_package["out_dir"] == build.get("out_dir", "dist"), "root package out_dir must come from axiom.toml")
    require(root_package["workspace_members"] == manifest.get("workspace", {}).get("members", []), "workspace members must come from axiom.toml")
    require(root_package["local_dependencies"] == normalize_dependencies(manifest.get("dependencies")), "local dependencies must come from axiom.toml")

    for package in packages:
        package_manifest = Path(package["manifest"])
        require(package_manifest.exists(), f"package manifest does not exist: {package_manifest}")
        decoded = load_toml(package_manifest)
        declared = decoded.get("package", {})
        require(package["name"] == declared.get("name"), f"{package['manifest']} package name mismatch")
        require(package["version"] == declared.get("version"), f"{package['manifest']} package version mismatch")
        require(Path(package["root"]).exists(), f"package root does not exist: {package['root']}")

    require({"manifest_hash", "lockfile_hash", "source_hashes"}.issubset(set(outputs.get("hash_inputs", []))), "hash inputs must include manifest, lockfile, and sources")
    result = {
        "schema": SCHEMA_VERSION,
        "ok": True,
        "packages": len(packages),
        "resolver_fixtures": 3,
        "fixture": str(args.snapshot),
    }
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(f"package graph boundary fixture ok: {len(packages)} packages")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
