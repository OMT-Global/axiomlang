#!/usr/bin/env python3
"""Validate the deterministic, Axiom-neutral Semantic MIR v1 fixture."""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "stage1/compiler-contracts/schemas/axiom.semantic_mir.v1.schema.json"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/semantic-mir-v1.json"
CAPTURE = {"rust", "cargo", "cranelift", "serde", "main.rs", "mir.rs"}

def fail(message):
    print(message, file=sys.stderr)
    raise SystemExit(1)


def enum_values(schema, *path):
    value = schema
    for key in path:
        value = value[key]
    return set(value["enum"])

def require(condition, message):
    if not condition:
        fail(message)

def validate_against_schema(value, schema):
    validate_schema_node(value, schema, "$", schema.get("$defs", {}))

def validate_schema_node(value, schema, path, defs):
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
        if "items" in schema:
            for index, item in enumerate(value):
                validate_schema_node(item, schema["items"], f"{path}[{index}]", defs)
    elif expected_type == "string":
        require(isinstance(value, str), f"{path} must be a string")
        if "minLength" in schema:
            require(len(value) >= schema["minLength"], f"{path} must not be empty")
        if "pattern" in schema:
            import re
            require(re.search(schema["pattern"], value) is not None, f"{path} must match {schema['pattern']!r}")
    elif expected_type == "integer":
        require(isinstance(value, int) and not isinstance(value, bool), f"{path} must be an integer")
        if "minimum" in schema:
            require(value >= schema["minimum"], f"{path} must be at least {schema['minimum']}")
    elif expected_type is not None:
        fail(f"{path} uses unsupported schema type {expected_type}")

def main():
    schema = json.loads(SCHEMA.read_text())
    snapshot = json.loads(SNAPSHOT.read_text())
    if schema.get("$id", "").endswith("axiom.semantic_mir.v1.schema.json") is False:
        fail("Semantic MIR schema id mismatch")
    validate_against_schema(snapshot, schema)
    if snapshot.get("schema_version") != "axiom.semantic_mir.v1" or snapshot.get("contract") != "compiler.semantic_mir" or snapshot.get("issue") != 1437:
        fail("Semantic MIR snapshot identity mismatch")
    feature_values = enum_values(schema, "properties", "features", "items")
    if set(snapshot["features"]) != feature_values or snapshot["features"] != sorted(snapshot["features"]):
        fail("Semantic MIR features must be complete and deterministically ordered")
    terminator_values = enum_values(schema, "$defs", "block", "properties", "terminator")
    instruction_values = enum_values(schema, "$defs", "instruction", "properties", "op")
    ids = []
    terminators = set()
    instructions = set()
    for function in snapshot["functions"]:
        ids.append(function["id"])
        for block in function["blocks"]:
            ids.append(block["id"])
            if block["terminator"] not in terminator_values:
                fail("Semantic MIR block has an unsupported terminator")
            if not block["semantic_nodes"]:
                fail("Semantic MIR block lacks semantic provenance")
            terminators.add(block["terminator"])
            for instruction in block["instructions"]:
                ids.append(instruction["id"])
                if instruction["op"] not in instruction_values:
                    fail("Semantic MIR instruction has an unsupported operation")
                if not instruction["semantic_nodes"]:
                    fail("Semantic MIR instruction lacks semantic provenance")
                instructions.add(instruction["op"])
    if terminators != terminator_values:
        fail("Semantic MIR fixture must cover every v1 terminator")
    if instructions != instruction_values:
        fail("Semantic MIR fixture must cover every v1 instruction operation")
    if len(ids) != len(set(ids)) or any(not value.startswith("axiom://") for value in ids):
        fail("Semantic MIR ids must be unique Axiom ids")
    text = json.dumps({"functions": snapshot["functions"], "features": snapshot["features"]}).lower()
    if any(term in text for term in CAPTURE):
        fail("Semantic MIR fixture leaks a Rust capture term")
    print(json.dumps({"schema": snapshot["schema_version"], "ok": True, "functions": len(snapshot["functions"])}))

if __name__ == "__main__":
    main()
