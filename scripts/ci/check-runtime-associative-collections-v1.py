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
CAPTURE = {"rust", "cargo", "cranelift", "hashmap", "hashset", "host address", "instruction stream"}

COLLECTIONS = {
    "map": ["construct", "lookup", "insert_or_replace", "remove", "clear", "len", "capacity", "iterate"],
    "set": ["construct", "contains", "insert", "remove", "clear", "len", "capacity", "iterate"],
    "replacement": "one_value_per_equal_key",
}
KEY_EQUALITY = {
    "mode": "total_deterministic_axiom_semantics",
    "cross_type": "never_equal",
    "primitive": "same_type_and_value",
    "text": "same_unicode_scalar_sequence",
    "tuple": "same_arity_and_recursive_component_equality",
    "enum": "same_declared_type_variant_and_recursive_payload_equality",
    "user_defined": "same_declared_immutable_type_and_declaration_order_fields",
    "hash_compatibility": "equal_keys_same_hash",
}
KEYS = {
    "equality": KEY_EQUALITY,
    "accepted": ["primitive", "text", "tuple_recursive", "enum_recursive", "accepted_immutable_value_shape"],
    "rejected": ["float", "mutable", "resource", "function", "borrowed", "unaccepted_user_shape"],
}
HASHING = {
    "equal_keys": "same_stable_hash",
    "algorithm": "runtime_defined_versioned_stable",
    "host_seed": "forbidden_for_default_or_reproducible_output",
    "randomized_hardening": "explicit_opt_in_preserves_observable_order",
}
ITERATION = {
    "order": "insertion_order",
    "replace": "does_not_move_key",
    "remove_reinsert": "appends",
    "clear": "resets_order",
    "mutation": "generation_checked_fail_closed",
}
RESOURCES = {
    "collision": "separate_chaining_bounded_probes",
    "limits": ["entries", "bytes", "load_factor", "collision_chain"],
    "growth": "checked_before_mutation",
    "failure": "structured_error_no_partial_mutation",
}
OWNERSHIP = {
    "lookup": "borrowed_key_no_transfer",
    "insert": "owned_key_value_transfer",
    "iterator": "scoped_generation_checked",
    "nested": "clone_drop_exactly_once",
    "aliasing": "ownership_and_borrow_contract_enforced",
}
ERRORS = ["invalid_key_type", "allocation_failed", "collection_limit", "collision_limit", "concurrent_collection_mutation", "use_after_move", "borrow_violation"]
FIXTURES = [
    {"id": "map-runtime-origin-growth-replace-remove", "kind": "positive"},
    {"id": "set-runtime-origin-contains-remove-clear", "kind": "positive"},
    {"id": "deterministic-insertion-order", "kind": "positive"},
    {"id": "equal-keys-stable-hash", "kind": "positive"},
    {"id": "tuple-enum-user-value-key", "kind": "positive"},
    {"id": "invalid-float-resource-function-borrowed-key", "kind": "negative"},
    {"id": "mutation-during-iteration", "kind": "negative"},
    {"id": "use-after-move-and-alias-borrow", "kind": "negative"},
    {"id": "allocation-and-limit-failure-no-partial-mutation", "kind": "resource"},
    {"id": "adversarial-collision-bounded", "kind": "adversarial"},
    {"id": "nested-clone-drop-cleanup", "kind": "lifecycle"},
    {"id": "compiler-symbol-table-determinism", "kind": "compiler-proof"},
]
MIGRATION = {
    "semantic_input": "Axiom semantic collection operations and lifecycle provenance",
    "dependencies": [1425, 1437, 1438, 1440],
}


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def validate_against_schema(value: Any, schema: dict[str, Any]) -> None:
    """Validate the checked-in snapshot with the published schema vocabulary."""
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
        if schema.get("pattern"):
            require(re.search(schema["pattern"], value) is not None, f"{path} must match {schema['pattern']!r}")
    elif expected_type == "integer":
        require(isinstance(value, int) and not isinstance(value, bool), f"{path} must be an integer")
    elif expected_type == "boolean":
        require(isinstance(value, bool), f"{path} must be a boolean")
    elif expected_type is not None:
        fail(f"{path} uses unsupported schema type {expected_type}")


def main() -> None:
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    snapshot: dict[str, Any] = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
    validate_against_schema(snapshot, schema)
    required = {"schema_version", "contract", "issue", "status", "collections", "keys", "hashing", "iteration", "resources", "ownership", "errors", "fixtures", "migration"}
    require(schema["type"] == "object" and schema["additionalProperties"] is False, "schema envelope drift")
    require(set(schema["required"]) == required, "schema required surface drift")
    require((snapshot["schema_version"], snapshot["contract"], snapshot["issue"], snapshot["status"]) == ("axiom.runtime-associative-collections.v1", "runtime.associative_collections", 1476, "contract_only"), "contract identity drifted")
    require(snapshot["collections"] == COLLECTIONS, "collection operation contract drifted")
    require(snapshot["keys"] == KEYS, "key shape or equality contract drifted")
    require(snapshot["hashing"] == HASHING, "hashing contract drifted")
    require(snapshot["iteration"] == ITERATION, "iteration contract drifted")
    require(snapshot["resources"] == RESOURCES, "collision or resource-bound contract drifted")
    require(snapshot["ownership"] == OWNERSHIP, "ownership contract drifted")
    require(snapshot["errors"] == ERRORS, "error coverage drifted")
    require(snapshot["fixtures"] == FIXTURES, "fixture coverage or classification drifted")
    require(snapshot["migration"]["semantic_input"] == MIGRATION["semantic_input"], "migration input drifted")
    require(snapshot["migration"]["dependencies"] == MIGRATION["dependencies"], "dependency boundary drifted")
    surface_value = dict(snapshot)
    surface_value["migration"] = dict(snapshot["migration"])
    surface_value["migration"]["forbidden_terms"] = []
    surface = json.dumps(surface_value, sort_keys=True).lower()
    require(not any(term in surface for term in CAPTURE), "contract leaks a host implementation term")
    print(json.dumps({"schema": snapshot["schema_version"], "ok": True, "operations": 16, "fixtures": len(FIXTURES)}))


if __name__ == "__main__":
    main()
