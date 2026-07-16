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

def main():
    schema = json.loads(SCHEMA.read_text())
    snapshot = json.loads(SNAPSHOT.read_text())
    if schema.get("$id", "").endswith("axiom.semantic_mir.v1.schema.json") is False:
        fail("Semantic MIR schema id mismatch")
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
