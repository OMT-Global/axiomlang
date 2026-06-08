#!/usr/bin/env python3
"""Validate the compiler MIR/backend package boundary fixture."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "axiom.compiler.mir_backend.v1"
CONTRACT = "compiler.mir_backend"
DEFAULT_SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.compiler.mir_backend.v1.schema.json")
DEFAULT_SNAPSHOT = Path("stage1/compiler-contracts/snapshots/mir-backend.json")
EXPECTED_PACKAGES = {
    "compiler.mir",
    "compiler.backend.contracts",
    "compiler.backend.generated_rust",
    "compiler.backend.native",
}
EXPECTED_TARGETS = {
    "axiom://target/stage1-generated-rust": "rust_source",
    "axiom://target/stage1-direct-native": "native_binary",
}
REQUIRED_BACKEND_INPUT_FIELDS = {
    "mir_version",
    "package_id",
    "entrypoints",
    "type_features",
    "effect_kinds",
    "artifact_kinds",
    "source_spans",
    "evidence_hooks",
}
FORBIDDEN_BACKEND_INPUT_REQUIRED_FIELDS = {
    "rust_source",
    "generated_rust",
    "cargo_metadata",
    "rustc_command",
    "rustc_output",
    "cranelift_module_path",
}
FORBIDDEN_DIRECT_NATIVE_REQUIREMENTS = {
    "rust_source",
    "generated_rust",
    "cargo_metadata",
    "rustc_output",
}
RUST_CAPTURE_TERMS = {
    "cargo",
    "rustc",
    "cranelift",
    "main.rs",
    "mir.rs",
    "codegen.rs",
    "cranelift_backend.rs",
    "serde",
    "rust module",
}


def load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def validate_against_schema(value: Any, schema: dict[str, Any]) -> None:
    validate_schema_node(value, schema, "$", schema.get("$defs", {}))


def validate_schema_node(value: Any, schema: dict[str, Any], path: str, defs: dict[str, Any]) -> None:
    if "$ref" in schema:
        ref = schema["$ref"]
        prefix = "#/$defs/"
        require(ref.startswith(prefix), f"{path} uses unsupported schema ref {ref}")
        name = ref[len(prefix):]
        require(name in defs, f"{path} references unknown schema def {name}")
        validate_schema_node(value, defs[name], path, defs)
        return

    if "const" in schema:
        require(value == schema["const"], f"{path} must equal {schema['const']!r}")
    if "enum" in schema:
        require(value in schema["enum"], f"{path} must be one of {schema['enum']!r}")

    expected_type = schema.get("type")
    if expected_type == "object":
        require(isinstance(value, dict), f"{path} must be an object")
        required = set(schema.get("required", []))
        missing = sorted(required - set(value))
        require(not missing, f"{path} is missing required fields: {', '.join(missing)}")
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            unexpected = sorted(set(value) - set(properties))
            require(not unexpected, f"{path} has unexpected fields: {', '.join(unexpected)}")
        for key, nested in value.items():
            if key in properties:
                validate_schema_node(nested, properties[key], f"{path}.{key}", defs)
    elif expected_type == "array":
        require(isinstance(value, list), f"{path} must be an array")
        if "minItems" in schema:
            require(len(value) >= schema["minItems"], f"{path} must have at least {schema['minItems']} items")
        item_schema = schema.get("items")
        if item_schema:
            for index, item in enumerate(value):
                validate_schema_node(item, item_schema, f"{path}[{index}]", defs)
    elif expected_type == "string":
        require(isinstance(value, str), f"{path} must be a string")
        if "minLength" in schema:
            require(len(value) >= schema["minLength"], f"{path} must not be empty")
    elif expected_type == "integer":
        require(isinstance(value, int) and not isinstance(value, bool), f"{path} must be an integer")
    elif expected_type is not None:
        fail(f"{path} uses unsupported schema type {expected_type}")


def reject_rust_capture_terms(values: list[str], path: str, allow_generated_rust_package: bool = False) -> None:
    for value in values:
        lowered = value.lower()
        for term in RUST_CAPTURE_TERMS:
            if allow_generated_rust_package and term in {"rustc"}:
                continue
            require(term not in lowered, f"{path} must not expose Rust capture term {term!r}: {value}")


def validate_packages(snapshot: dict[str, Any]) -> None:
    packages = {package["name"]: package for package in snapshot["packages"]}
    require(set(packages) == EXPECTED_PACKAGES, "package set mismatch")

    require("compiler.mir.lower_package" in packages["compiler.mir"]["apis"], "compiler.mir must expose lower_package")
    require("compiler.mir.export_backend_input" in packages["compiler.mir"]["apis"], "compiler.mir must expose export_backend_input")
    require("compiler.backend.contracts.select_target" in packages["compiler.backend.contracts"]["apis"], "backend contracts must expose select_target")
    require("compiler.backend.native.lower_to_object" in packages["compiler.backend.native"]["apis"], "native backend must expose lower_to_object")

    for name, package in packages.items():
        values = [*package["apis"], *package["owns"]]
        reject_rust_capture_terms(
            values,
            f"packages.{name}",
            allow_generated_rust_package=name == "compiler.backend.generated_rust",
        )


def validate_backend_input(snapshot: dict[str, Any]) -> None:
    backend_input = snapshot["backend_input"]
    fields = set(backend_input["required_fields"])
    require(REQUIRED_BACKEND_INPUT_FIELDS.issubset(fields), "backend input missing required MIR-to-target fields")
    forbidden = set(backend_input["forbidden_required_fields"])
    require(FORBIDDEN_BACKEND_INPUT_REQUIRED_FIELDS.issubset(forbidden), "backend input must forbid Rust-derived required fields")
    require(not fields.intersection(forbidden), "backend input required fields conflict with forbidden fields")


def validate_targets(snapshot: dict[str, Any]) -> None:
    targets = {target["id"]: target for target in snapshot["targets"]}
    require(set(targets) == set(EXPECTED_TARGETS), "target set mismatch")

    for target_id, expected_class in EXPECTED_TARGETS.items():
        target = targets[target_id]
        require(target["class"] == expected_class, f"{target_id} target class mismatch")
        require(expected_class in target["primary_artifacts"], f"{target_id} must emit its class as primary artifact")

    generated = targets["axiom://target/stage1-generated-rust"]
    native = targets["axiom://target/stage1-direct-native"]
    require(generated["package"] == "compiler.backend.generated_rust", "generated Rust target package mismatch")
    require(native["package"] == "compiler.backend.native", "direct native target package mismatch")
    require(set(native["must_not_require"]) == FORBIDDEN_DIRECT_NATIVE_REQUIREMENTS, "direct native must_not_require set mismatch")
    require("rust_source" not in native["primary_artifacts"], "direct native primary artifacts must not include rust_source")
    require("rust_source" not in native["required_evidence"], "direct native evidence must not require rust_source")
    require("runtime_abi" in native["required_evidence"], "direct native evidence must include runtime ABI")


def validate_evidence(snapshot: dict[str, Any]) -> None:
    rules = snapshot["evidence_rules"]
    forbidden = set(rules["direct_native_must_not_require"])
    require(forbidden == FORBIDDEN_DIRECT_NATIVE_REQUIREMENTS, "direct native evidence forbidden set mismatch")
    allowed_refs = set(rules["direct_native_may_reference"])
    require({"native_binary", "native_object", "runtime_abi_row", "unsupported_feature_diagnostic"}.issubset(allowed_refs), "direct native evidence references are incomplete")
    scope = rules["generated_rust_scope"].lower()
    require("optional parity evidence" in scope, "generated Rust scope must be optional parity evidence only for native backend")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA)
    parser.add_argument("--snapshot", type=Path, default=DEFAULT_SNAPSHOT)
    parser.add_argument("--json", action="store_true", help="emit a JSON validation result")
    args = parser.parse_args()

    schema = load_json(args.schema)
    snapshot = load_json(args.snapshot)

    require(schema.get("$id", "").endswith("/axiom.compiler.mir_backend.v1.schema.json"), "schema $id must name MIR/backend v1")
    require(schema.get("title") == "Axiom compiler MIR and backend package contract", "schema title changed unexpectedly")
    validate_against_schema(snapshot, schema)
    require(snapshot["schema_version"] == SCHEMA_VERSION, "snapshot schema_version mismatch")
    require(snapshot["contract"] == CONTRACT, "snapshot contract mismatch")
    require(snapshot["issue"] == 939, "snapshot issue mismatch")
    validate_packages(snapshot)
    validate_backend_input(snapshot)
    validate_targets(snapshot)
    validate_evidence(snapshot)

    result = {
        "schema": SCHEMA_VERSION,
        "ok": True,
        "packages": len(snapshot["packages"]),
        "targets": len(snapshot["targets"]),
        "fixture": str(args.snapshot),
    }
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(f"MIR/backend boundary fixture ok: {result['packages']} packages, {result['targets']} targets")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
