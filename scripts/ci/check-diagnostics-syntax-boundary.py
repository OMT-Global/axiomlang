#!/usr/bin/env python3
"""Validate compiler.diagnostics/compiler.syntax boundary fixtures."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "axiom.compiler.diagnostics_syntax.v1"
DEFAULT_SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.compiler.diagnostics_syntax.v1.schema.json")
DEFAULT_SNAPSHOT = Path("stage1/compiler-contracts/snapshots/diagnostics-syntax.json")
REQUIRED_CONTRACTS = {"compiler.diagnostics", "compiler.syntax"}
REQUIRED_PARSE_CODES = {
    "parse.unexpected_token",
    "parse.invalid_syntax",
    "parse.missing_token",
    "parse.unsupported_syntax",
}
REQUIRED_ENVELOPE_FIELDS = {"kind", "message", "path", "line", "column", "end_line", "end_column", "related"}
FORBIDDEN_RUST_TERMS = ("rust enum", "rust module", "serde layout", "syntax.rs")


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def validate_against_schema(value: Any, schema: dict[str, Any]) -> None:
    validate_schema_node(value, schema, "$", schema.get("$defs", {}))


def validate_schema_node(value: Any, schema: dict[str, Any], path: str, defs: dict[str, Any]) -> None:
    if "$ref" in schema:
        prefix = "#/$defs/"
        ref = schema["$ref"]
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
        properties = schema.get("properties", {})
        missing = sorted(set(schema.get("required", [])) - set(value))
        require(not missing, f"{path} is missing required fields: {', '.join(missing)}")
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
        if schema.get("uniqueItems") is True:
            seen = {json.dumps(item, sort_keys=True) for item in value}
            require(len(seen) == len(value), f"{path} items must be unique")
        if "items" in schema:
            for index, item in enumerate(value):
                validate_schema_node(item, schema["items"], f"{path}[{index}]", defs)
    elif expected_type == "string":
        require(isinstance(value, str), f"{path} must be a string")
        if "minLength" in schema:
            require(len(value) >= schema["minLength"], f"{path} must not be empty")
    elif expected_type == "integer":
        require(isinstance(value, int) and not isinstance(value, bool), f"{path} must be an integer")
        if "minimum" in schema:
            require(value >= schema["minimum"], f"{path} must be >= {schema['minimum']}")
    elif expected_type is not None:
        fail(f"{path} uses unsupported schema type {expected_type}")


def assert_check_fixture(name: str, fixture: dict[str, Any]) -> None:
    path = Path(fixture["path"])
    require(path.exists(), f"{name} fixture does not exist: {path}")
    payload = load_json(path)
    assertions = set(fixture["assertions"])

    if "ok_true" in assertions:
        require(payload.get("ok") is True, f"{name} must be ok=true")
    if "ok_false" in assertions:
        require(payload.get("ok") is False, f"{name} must be ok=false")
    if "command_check" in assertions:
        require(payload.get("command") == "check", f"{name} must be a check payload")
    if "statement_count_present" in assertions:
        require(isinstance(payload.get("statement_count"), int), f"{name} must include statement_count")

    error = payload.get("error")
    if any(assertion.startswith("kind_") or assertion.endswith("_span_present") or assertion == "stable_code_present" for assertion in assertions):
        require(isinstance(error, dict), f"{name} must include error object")

    if "kind_parse" in assertions:
        require(error.get("kind") == "parse", f"{name} must be a parse diagnostic")
    if "kind_type" in assertions:
        require(error.get("kind") == "type", f"{name} must be a type diagnostic")
    if "kind_ownership" in assertions:
        require(error.get("kind") == "ownership", f"{name} must be an ownership diagnostic")
    if "kind_capability" in assertions:
        require(error.get("kind") == "capability", f"{name} must be a capability diagnostic")
    if "start_span_present" in assertions:
        for key in ["path", "line", "column"]:
            require(key in error, f"{name} diagnostic must include {key}")
    if "end_span_present" in assertions:
        for key in ["end_line", "end_column"]:
            require(key in error, f"{name} diagnostic must include {key}")
    if "stable_code_present" in assertions:
        require(isinstance(error.get("code"), str) and error["code"], f"{name} diagnostic must include stable code")


def reject_rust_capture(snapshot: dict[str, Any]) -> None:
    public_values = []
    public_values.extend(snapshot["diagnostics"]["stable_parse_codes"])
    public_values.extend(snapshot["syntax"]["public_terms"])
    public_values.extend(snapshot["diagnostics"]["entrypoints"])
    public_values.extend(snapshot["syntax"]["entrypoints"])
    for value in public_values:
        lowered = value.lower()
        for term in FORBIDDEN_RUST_TERMS:
            require(term not in lowered, f"public contract value {value!r} captures Rust term {term!r}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA)
    parser.add_argument("--snapshot", type=Path, default=DEFAULT_SNAPSHOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    schema = load_json(args.schema)
    snapshot = load_json(args.snapshot)

    require(schema.get("$id", "").endswith("/axiom.compiler.diagnostics_syntax.v1.schema.json"), "schema $id must name diagnostics/syntax v1")
    require(schema.get("title") == "Axiom compiler diagnostics and syntax contract", "schema title changed unexpectedly")
    validate_against_schema(snapshot, schema)

    require(snapshot.get("schema_version") == SCHEMA_VERSION, "snapshot schema_version mismatch")
    require(set(snapshot.get("contracts", [])) == REQUIRED_CONTRACTS, "snapshot must include diagnostics and syntax contracts")
    stable_parse_codes = snapshot["diagnostics"]["stable_parse_codes"]
    require(REQUIRED_PARSE_CODES.issubset(set(stable_parse_codes)), "stable parse code list is incomplete")
    for code in stable_parse_codes:
        require(code.startswith("parse."), f"stable parse code {code!r} must use the parse.* namespace")
    require(REQUIRED_ENVELOPE_FIELDS.issubset(set(snapshot["diagnostics"]["envelope_fields"])), "diagnostic envelope field list is incomplete")
    reject_rust_capture(snapshot)

    macro = snapshot["syntax"]["macro_expansion_record"]["fixture"]
    require(macro["expanded_line_start"] <= macro["expanded_line_end"], "macro expansion line range is invalid")
    require(macro["call_site"]["line"] >= 1 and macro["call_site"]["column"] >= 1, "macro call_site must be one-based")

    for name, fixture in snapshot["fixtures"].items():
        assert_check_fixture(name, fixture)

    result = {
        "schema": SCHEMA_VERSION,
        "ok": True,
        "fixtures": len(snapshot["fixtures"]),
        "stable_parse_codes": len(snapshot["diagnostics"]["stable_parse_codes"]),
    }
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(f"diagnostics/syntax boundary fixture ok: {result['fixtures']} fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
