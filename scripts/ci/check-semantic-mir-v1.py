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


def resolve_schema(schema, defs):
    while "$ref" in schema:
        ref = schema["$ref"]
        prefix = "#/$defs/"
        require(ref.startswith(prefix), f"unsupported schema ref {ref}")
        name = ref[len(prefix):]
        require(name in defs, f"unknown schema def {name}")
        schema = defs[name]
    return schema


def enum_values(schema, *path):
    value = schema
    for key in path:
        value = value[key]
    value = resolve_schema(value, schema.get("$defs", {}))
    return set(value["enum"])

def require(condition, message):
    if not condition:
        fail(message)

def validate_against_schema(value, schema):
    validate_schema_node(value, schema, "$", schema.get("$defs", {}))

def validate_schema_node(value, schema, path, defs):
    if "$ref" in schema:
        validate_schema_node(value, resolve_schema(schema, defs), path, defs)
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


RESULT_OPERATIONS = {"const", "copy", "move", "borrow", "load", "aggregate", "binary", "cast", "await"}
PLACE_OPERATIONS = {"load", "store"}
TERMINATOR_MIN_SUCCESSORS = {
    "goto": 1,
    "branch": 2,
    "match": 2,
    "return": 0,
    "panic": 1,
    "unwind": 1,
    "unreachable": 0,
}


def validate_executable_function(function, features):
    blocks = function["blocks"]
    block_by_id = {block["id"]: block for block in blocks}
    require(len(block_by_id) == len(blocks), "Semantic MIR blocks must have unique ids")
    values = {}
    ids = [function["id"]]
    for block in blocks:
        ids.append(block["id"])
        for parameter in block["parameters"]:
            values[parameter["id"]] = parameter
            ids.append(parameter["id"])
        for instruction in block["instructions"]:
            ids.append(instruction["id"])
            if "result" in instruction:
                values[instruction["result"]["id"]] = instruction["result"]
                ids.append(instruction["result"]["id"])
    places = {place["id"]: place for place in function["places"]}
    cleanup_scopes = {scope["id"]: scope for scope in function["cleanup_scopes"]}
    ids.extend(places)
    ids.extend(cleanup_scopes)
    require(len(ids) == len(set(ids)), "Semantic MIR declaration ids must be unique")
    for place in places.values():
        require(place["base"] in values, "Semantic MIR place base must reference a declared value")
    instruction_ids = {
        instruction["id"]
        for block in blocks
        for instruction in block["instructions"]
    }
    for scope in cleanup_scopes.values():
        require(set(scope["actions"]).issubset(instruction_ids), "Semantic MIR cleanup action must reference an instruction")

    observed_features = set()
    has_back_edge = False
    block_order = {block["id"]: index for index, block in enumerate(blocks)}
    for block in blocks:
        available_values = {parameter["id"] for parameter in block["parameters"]}
        for instruction in block["instructions"]:
            operation = instruction["op"]
            require(set(instruction["operands"]).issubset(available_values), "Semantic MIR instruction operand must reference a value available in its block")
            if operation in RESULT_OPERATIONS:
                require("result" in instruction, f"Semantic MIR {operation} instruction must declare a result value")
            if operation in PLACE_OPERATIONS:
                require(instruction.get("place") in places, f"Semantic MIR {operation} instruction must reference a declared place")
            if operation == "store":
                require(bool(instruction["operands"]), "Semantic MIR store instruction must consume a value")
            if operation in {"call", "capability_call"}:
                require(bool(instruction.get("callee")), "Semantic MIR call instruction must identify its callee")
                require(bool(instruction.get("effects")), "Semantic MIR call instruction must declare effects")
            if operation == "capability_call":
                require(bool(instruction.get("capability")), "Semantic MIR capability call must declare its capability")
            if operation == "defer_scope":
                require(instruction.get("cleanup_scope") in cleanup_scopes, "Semantic MIR defer instruction must reference a cleanup scope")
            if "result" in instruction:
                available_values.add(instruction["result"]["id"])
            observed_features.update(instruction["features"])
        terminator = block["terminator"]
        successors = terminator["successors"]
        operation = terminator["op"]
        require(set(terminator["operands"]).issubset(available_values), "Semantic MIR terminator operand must reference a value available in its block")
        minimum = TERMINATOR_MIN_SUCCESSORS[operation]
        if minimum == 0:
            require(not successors, f"Semantic MIR {operation} terminator must not have successors")
        else:
            require(len(successors) >= minimum, f"Semantic MIR {operation} terminator lacks required successors")
        for successor in successors:
            target = successor["target"]
            require(target in block_by_id, "Semantic MIR successor must reference a declared block")
            require(
                len(successor["arguments"]) == len(block_by_id[target]["parameters"]),
                "Semantic MIR successor arguments must match target block parameters",
            )
            require(set(successor["arguments"]).issubset(available_values), "Semantic MIR successor argument must reference a value available in its block")
            if block_order[target] <= block_order[block["id"]]:
                has_back_edge = True
        observed_features.update(terminator["features"])
        for scope in terminator.get("cleanup_scopes", []):
            require(scope in cleanup_scopes, "Semantic MIR terminator cleanup scope must be declared")
    require(observed_features == features, "Semantic MIR fixture must provide executable evidence for every declared feature")
    if "loop" in features:
        require(has_back_edge, "Semantic MIR loop feature requires an explicit CFG back-edge")

def main():
    schema = json.loads(SCHEMA.read_text())
    snapshot = json.loads(SNAPSHOT.read_text())
    if schema.get("$id", "").endswith("axiom.semantic_mir.v1.schema.json") is False:
        fail("Semantic MIR schema id mismatch")
    validate_against_schema(snapshot, schema)
    if snapshot.get("schema_version") != "axiom.semantic_mir.v1" or snapshot.get("contract") != "compiler.semantic_mir" or snapshot.get("issue") != 1437:
        fail("Semantic MIR snapshot identity mismatch")
    feature_values = enum_values(schema, "$defs", "feature")
    if set(snapshot["features"]) != feature_values or len(snapshot["features"]) != len(feature_values) or snapshot["features"] != sorted(snapshot["features"]):
        fail("Semantic MIR features must be complete and deterministically ordered")
    terminator_values = enum_values(schema, "$defs", "terminator", "properties", "op")
    instruction_values = enum_values(schema, "$defs", "instruction", "properties", "op")
    ids = []
    terminators = set()
    instructions = set()
    for function in snapshot["functions"]:
        ids.append(function["id"])
        validate_executable_function(function, feature_values)
        for block in function["blocks"]:
            ids.append(block["id"])
            if block["terminator"]["op"] not in terminator_values:
                fail("Semantic MIR block has an unsupported terminator")
            if not block["semantic_nodes"]:
                fail("Semantic MIR block lacks semantic provenance")
            terminators.add(block["terminator"]["op"])
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
