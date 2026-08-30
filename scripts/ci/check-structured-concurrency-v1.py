#!/usr/bin/env python3
"""Validate the target-neutral Structured Concurrency v1 contract."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "stage1/compiler-contracts/schemas/axiom.runtime_concurrency.v1.schema.json"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/runtime-concurrency-v1.json"

FEATURES = {
    "bounded_channels", "cancellation", "fair_select", "join", "resource_budgets",
    "structured_children", "task_scopes", "timeouts", "waiter_fairness", "wake_on_close",
}
OPERATIONS = {
    "spawn", "join", "cancel", "timeout", "channel_create", "send", "receive", "close", "select", "task_scope",
}
FIXTURES = {
    "spawn-join", "nested-scope", "cancel-tree", "timeout", "channel-order", "channel-drain",
    "select-fairness", "bounded-send", "detached-child", "join-timeout", "channel-closed",
    "task-budget", "use-after-close",
}
NEGATIVE_FIXTURES = {"detached-child", "join-timeout", "channel-closed", "task-budget", "use-after-close"}
CAPTURE_TERMS = {"cargo", "cranelift", "rust", "tokio", "mpsc", "thread-local scheduler"}
AXIOM_ID = re.compile(r"^axiom://[A-Za-z0-9._~:/#@!$&'()*+,;=%-]+$")


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot load {path}: {error}")


def validate_schema_node(value: Any, schema: dict[str, Any], path: str, defs: dict[str, Any]) -> None:
    if "$ref" in schema:
        prefix = "#/$defs/"
        ref = schema["$ref"]
        require(ref.startswith(prefix) and ref[len(prefix):] in defs, f"{path} has unknown schema ref {ref}")
        validate_schema_node(value, defs[ref[len(prefix):]], path, defs)
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
        require(not missing, f"{path} is missing {', '.join(missing)}")
        if schema.get("additionalProperties") is False:
            require(not (set(value) - set(properties)), f"{path} has unexpected fields")
        for key, nested in value.items():
            if key in properties:
                validate_schema_node(nested, properties[key], f"{path}.{key}", defs)
    elif expected == "array":
        require(isinstance(value, list), f"{path} must be an array")
        require(len(value) >= schema.get("minItems", 0), f"{path} has too few items")
        if "items" in schema:
            for index, item in enumerate(value):
                validate_schema_node(item, schema["items"], f"{path}[{index}]", defs)
    elif expected == "string":
        require(isinstance(value, str), f"{path} must be a string")
        require(len(value) >= schema.get("minLength", 0), f"{path} must not be empty")
        if "pattern" in schema:
            require(re.search(schema["pattern"], value) is not None, f"{path} has an invalid format")
    elif expected == "integer":
        require(isinstance(value, int) and not isinstance(value, bool), f"{path} must be an integer")
        require(value >= schema.get("minimum", value), f"{path} is below its minimum")
    elif expected == "boolean":
        require(isinstance(value, bool), f"{path} must be a boolean")
    elif expected is not None:
        fail(f"{path} uses unsupported schema type {expected}")


def reject_capture(value: Any, path: str = "$") -> None:
    if isinstance(value, dict):
        for key, nested in value.items():
            reject_capture(key, f"{path}.{key}#key")
            reject_capture(nested, f"{path}.{key}")
    elif isinstance(value, list):
        for index, nested in enumerate(value):
            reject_capture(nested, f"{path}[{index}]")
    elif isinstance(value, str):
        lowered = value.lower()
        for term in CAPTURE_TERMS:
            if term in lowered:
                fail(f"{path} exposes host implementation term {term!r}")


def main() -> None:
    schema = load_json(SCHEMA)
    snapshot = load_json(SNAPSHOT)
    validate_schema_node(snapshot, schema, "$", schema.get("$defs", {}))
    require(schema["$id"].endswith("axiom.runtime_concurrency.v1.schema.json"), "schema id mismatch")
    require(snapshot["features"] == sorted(snapshot["features"]), "features must be deterministically ordered")
    require(set(snapshot["features"]) == FEATURES, "feature set is incomplete")
    operations = snapshot["operations"]
    require({operation["kind"] for operation in operations} == OPERATIONS, "operation set is incomplete")
    require(len({operation["id"] for operation in operations}) == len(operations), "operation ids must be unique")
    require(all(AXIOM_ID.fullmatch(operation["id"]) for operation in operations), "operation ids must be Axiom ids")
    require(snapshot["task_model"]["budgets"] == sorted(snapshot["task_model"]["budgets"]), "task budgets must be ordered")
    require(snapshot["cleanup"]["exit_reasons"] == sorted(snapshot["cleanup"]["exit_reasons"]), "cleanup exits must be ordered")
    require(snapshot["cleanup"]["exactly_once"] is True, "cleanup must be exactly once")
    require(snapshot["select"]["max_arms"] >= 2, "select must have at least two arms")
    diagnostics = snapshot["diagnostics"]
    require(diagnostics["required_codes"] == sorted(diagnostics["required_codes"]), "diagnostic codes must be ordered")
    require(set(diagnostics["source_fields"]) == {"column", "line", "path"}, "source fields are incomplete")
    fixture_ids = {fixture["id"].rsplit("/", 1)[-1] for fixture in snapshot["fixtures"]}
    require(fixture_ids == FIXTURES, "fixture set is incomplete")
    negative = {fixture["id"].rsplit("/", 1)[-1] for fixture in snapshot["fixtures"] if fixture["kind"] == "negative"}
    require(negative == NEGATIVE_FIXTURES, "negative fixture set is incomplete")
    require(all(AXIOM_ID.fullmatch(fixture["id"]) for fixture in snapshot["fixtures"]), "fixture ids must be Axiom ids")
    migration = snapshot["migration"]
    require(len(migration["out_of_scope"]) == len(set(migration["out_of_scope"])), "out-of-scope items must be unique")
    reject_capture({
        "features": snapshot["features"],
        "operations": snapshot["operations"],
        "task_model": snapshot["task_model"],
        "channel_model": snapshot["channel_model"],
        "select": snapshot["select"],
        "cleanup": snapshot["cleanup"],
        "diagnostics": snapshot["diagnostics"],
        "inspection_fields": snapshot["inspection_fields"],
        "fixtures": snapshot["fixtures"],
        "semantic_input": migration["semantic_input"],
        "out_of_scope": migration["out_of_scope"],
    })
    print(json.dumps({"schema": snapshot["schema_version"], "ok": True, "operations": len(operations), "fixtures": len(fixture_ids)}))


if __name__ == "__main__":
    main()
