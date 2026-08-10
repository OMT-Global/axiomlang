#!/usr/bin/env python3
"""Validate the target-neutral Runtime Associative Collections v1 contract."""
import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "stage1/compiler-contracts/schemas/axiom.runtime_associative_collections.v1.schema.json"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json"
OPERATIONS = {"lookup", "insert", "replace", "remove", "contains", "length", "iterate", "clear", "reserve", "hash"}
FIXTURES = {"map-lookup", "map-replace", "set-idempotent", "type-aware-equality", "deterministic-iteration", "remove-reinsert", "runtime-origin", "allocation-failure", "unsupported-key", "limit-exceeded", "mutation-during-iteration", "borrow-escape"}
CAPTURE = {"rust", "cargo", "cranelift", "hashmap", "hashset", "host address", "instruction stream"}


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def validate(value: Any, schema: dict[str, Any], path: str = "$", defs: dict[str, Any] | None = None) -> None:
    defs = schema.get("$defs", {}) if defs is None else defs
    if "$ref" in schema:
        name = schema["$ref"].removeprefix("#/$defs/")
        require(name in defs, f"{path} references unknown schema definition")
        validate(value, defs[name], path, defs)
        return
    if "const" in schema:
        require(value == schema["const"], f"{path} must equal {schema['const']!r}")
    if "enum" in schema:
        require(value in schema["enum"], f"{path} must be one of {schema['enum']!r}")
    expected = schema.get("type")
    if expected == "object":
        require(isinstance(value, dict), f"{path} must be an object")
        properties = schema.get("properties", {})
        missing = sorted(set(schema.get("required", [])) - set(value))
        require(not missing, f"{path} is missing required fields: {', '.join(missing)}")
        if schema.get("additionalProperties") is False:
            require(not (set(value) - set(properties)), f"{path} has unexpected fields")
        for key, nested in properties.items():
            if key in value:
                validate(value[key], nested, f"{path}.{key}", defs)
    elif expected == "array":
        require(isinstance(value, list), f"{path} must be an array")
        require(len(value) >= schema.get("minItems", 0), f"{path} has too few items")
        for index, item in enumerate(value):
            validate(item, schema.get("items", {}), f"{path}[{index}]", defs)
    elif expected == "string":
        require(isinstance(value, str), f"{path} must be a string")
        require(len(value) >= schema.get("minLength", 0), f"{path} must not be empty")
        if "pattern" in schema:
            require(re.search(schema["pattern"], value) is not None, f"{path} has an invalid format")
    elif expected == "integer":
        require(isinstance(value, int) and not isinstance(value, bool), f"{path} must be an integer")
    elif expected is not None:
        fail(f"{path} uses unsupported schema type {expected}")


def main() -> None:
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    snapshot = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
    validate(snapshot, schema)
    require(schema["$id"].endswith("axiom.runtime_associative_collections.v1.schema.json"), "schema id mismatch")
    require((snapshot["schema_version"], snapshot["contract"], snapshot["issue"], snapshot["status"]) == ("axiom.runtime_associative_collections.v1", "runtime.associative_collections", 1476, "contract_only"), "contract identity drifted")
    require(snapshot["key_types"] == sorted({"bool", "int", "text"}), "key types must be complete and ordered")
    operation_ids = [row["id"] for row in snapshot["operations"]]
    require(len(operation_ids) == len(set(operation_ids)), "operations must be unique")
    require({row["kind"] for row in snapshot["operations"]} == OPERATIONS, "operation coverage is incomplete")
    require(snapshot["equality"] == {"mode": "semantic_key_equality", "type_aware": True, "hashing": "deterministic_per_contract_version", "hash_visibility": "implementation_detail"}, "equality contract drifted")
    require(snapshot["ordering"] == {"iteration": "insertion_order", "replace_existing_position": "preserve", "remove_then_insert": "append"}, "ordering contract drifted")
    require(snapshot["limits"]["failure_behavior"] == "preserve_existing_entries", "limit failures must preserve existing entries")
    require(snapshot["lifecycle"] == {"ownership": "collection_owns_entries", "cleanup": "exactly_once", "borrow": "declared_extent", "authority": "cannot_escape_creator_authority"}, "lifecycle contract drifted")
    fixture_ids = {row["id"].rsplit("/", 1)[-1] for row in snapshot["fixtures"]}
    require(fixture_ids == FIXTURES, "fixture coverage is incomplete")
    negative = {row["id"].rsplit("/", 1)[-1] for row in snapshot["fixtures"] if row["kind"] == "negative"}
    require(negative == {"allocation-failure", "unsupported-key", "limit-exceeded", "mutation-during-iteration", "borrow-escape"}, "negative fixture coverage is incomplete")
    require(snapshot["migration"]["dependencies"] == [1425, 1438, 1440], "dependency boundary drifted")
    surface_value = dict(snapshot)
    surface_value["migration"] = dict(snapshot["migration"])
    surface_value["migration"]["forbidden_terms"] = []
    surface = json.dumps(surface_value, sort_keys=True).lower()
    require(not any(term in surface for term in CAPTURE), "contract leaks a host implementation term")
    print(json.dumps({"schema": snapshot["schema_version"], "ok": True, "operations": len(operation_ids), "fixtures": len(fixture_ids)}))


if __name__ == "__main__":
    main()
