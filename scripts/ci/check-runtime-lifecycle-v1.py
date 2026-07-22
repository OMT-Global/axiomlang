#!/usr/bin/env python3
"""Validate the deterministic, target-neutral Runtime Lifecycle ABI v1 fixture."""
import json
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "stage1/compiler-contracts/schemas/axiom.runtime_lifecycle.v1.schema.json"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/runtime-lifecycle-v1.json"
FEATURES = {"aggregate_cleanup", "allocation", "allocation_failure", "backend_declaration", "borrow_extent", "capability_resource", "clone", "copy", "defer", "deterministic_cleanup", "diagnostic", "drop", "inspection", "move", "panic_unwind", "recursive_destroy", "resize"}
OPERATIONS = {"allocate", "resize", "allocation_failure", "move", "copy", "clone", "borrow", "borrow_end", "drop", "recursive_destroy", "defer", "scope_exit", "resource_close", "resource_use"}
EXIT_REASONS = {"normal_return", "early_return", "error_return", "panic_unwind", "cancellation"}
FIXTURES = {"normal-return", "early-return", "error-return", "panic-defer", "nested-aggregate", "allocation-failure", "move-clone-copy", "borrow-extent", "resource-close", "leak", "double-free", "use-after-free", "resource-escape", "double-close"}
CAPTURE = {"rust", "cargo", "cranelift", "serde", "box", "vec", "drop trait"}


def fail(message):
    print(message, file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def validate_against_schema(value: Any, schema: dict[str, Any]) -> None:
    """Validate the checked-in fixture with the JSON Schema vocabulary it uses."""
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
        if "pattern" in schema:
            import re

            require(re.search(schema["pattern"], value) is not None, f"{path} must match {schema['pattern']!r}")
    elif expected_type == "integer":
        require(isinstance(value, int) and not isinstance(value, bool), f"{path} must be an integer")
    elif expected_type is not None:
        fail(f"{path} uses unsupported schema type {expected_type}")


def main():
    schema = json.loads(SCHEMA.read_text())
    snapshot = json.loads(SNAPSHOT.read_text())
    if not schema.get("$id", "").endswith("axiom.runtime_lifecycle.v1.schema.json"):
        fail("Runtime Lifecycle ABI schema id mismatch")
    validate_against_schema(snapshot, schema)
    if (snapshot.get("schema_version"), snapshot.get("contract"), snapshot.get("issue"), snapshot.get("semantic_mir_version")) != ("axiom.runtime_lifecycle.v1", "runtime.lifecycle", 1438, "axiom.semantic_mir.v1"):
        fail("Runtime Lifecycle ABI snapshot identity mismatch")
    if set(snapshot["features"]) != FEATURES or snapshot["features"] != sorted(snapshot["features"]):
        fail("Runtime Lifecycle ABI features must be complete and deterministically ordered")
    operation_ids = [operation["id"] for operation in snapshot["operations"]]
    operation_kinds = {operation["kind"] for operation in snapshot["operations"]}
    if operation_kinds != OPERATIONS or len(operation_ids) != len(set(operation_ids)):
        fail("Runtime Lifecycle ABI operations must be complete and have unique ids")
    if any(not value.startswith("axiom://") for value in operation_ids):
        fail("Runtime Lifecycle ABI operation ids must be Axiom ids")
    cleanup = snapshot["cleanup"]
    if set(cleanup["exit_reasons"]) != EXIT_REASONS or cleanup["exit_reasons"] != sorted(cleanup["exit_reasons"]):
        fail("Runtime Lifecycle ABI cleanup exits must be complete and deterministically ordered")
    if (cleanup["defer_order"], cleanup["drop_order"], cleanup["aggregate_order"], cleanup["exactly_once"]) != ("last_in_first_out_before_drop", "reverse_introduction", "reverse_declaration_or_insertion_then_release", True):
        fail("Runtime Lifecycle ABI cleanup order or exactly-once rule drifted")
    backend = snapshot["backend"]
    if backend.get("unsupported_diagnostic") != "backend.unsupported_lifecycle_feature":
        fail("Runtime Lifecycle ABI backend diagnostic drifted")
    if backend.get("diagnostic_fields") != ["backend_id", "feature_id", "operation_id", "source_span"]:
        fail("Runtime Lifecycle ABI backend diagnostic fields are incomplete")
    if set(backend["required_declarations"]) != {"supported_lifecycle_features", "allocation_failure_model", "lifecycle_diagnostics"}:
        fail("Runtime Lifecycle ABI backend declarations are incomplete")
    if set(snapshot["inspection_fields"]) != {"allocation_effect", "borrow_extent", "cleanup_obligations", "ownership_transfer", "resource_authority", "source_provenance"}:
        fail("Runtime Lifecycle ABI inspection fields are incomplete")
    fixture_ids = {fixture["id"].rsplit("/", 1)[-1] for fixture in snapshot["fixtures"]}
    if fixture_ids != FIXTURES:
        fail("Runtime Lifecycle ABI fixture coverage is incomplete")
    negative = {fixture["id"].rsplit("/", 1)[-1] for fixture in snapshot["fixtures"] if fixture["kind"] == "negative"}
    if negative != {"leak", "double-free", "use-after-free", "resource-escape", "double-close"}:
        fail("Runtime Lifecycle ABI negative fixture coverage drifted")
    capture_surface = {
        "features": snapshot["features"],
        "operations": snapshot["operations"],
        "cleanup": snapshot["cleanup"],
        "backend": snapshot["backend"],
        "inspection_fields": snapshot["inspection_fields"],
        "fixtures": snapshot["fixtures"],
        "semantic_input": snapshot["migration"]["semantic_input"],
        "out_of_scope": snapshot["migration"]["out_of_scope"],
    }
    text = json.dumps(capture_surface).lower()
    if any(term in text for term in CAPTURE):
        fail("Runtime Lifecycle ABI fixture leaks a host capture term")
    print(json.dumps({"schema": snapshot["schema_version"], "ok": True, "operations": len(operation_ids), "fixtures": len(fixture_ids)}))


if __name__ == "__main__":
    main()
