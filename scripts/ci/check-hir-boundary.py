#!/usr/bin/env python3
"""Validate the compiler.hir ownership and capability boundary fixture."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "axiom.compiler.hir_ownership_capability.v1"
CONTRACT = "compiler.hir"
DEFAULT_SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.compiler.hir_ownership_capability.v1.schema.json")
DEFAULT_SNAPSHOT = Path("stage1/compiler-contracts/snapshots/hir-ownership-capability.json")
EXPECTED_APIS = {
    "compiler.hir.build_package_hir",
    "compiler.hir.resolve_names",
    "compiler.hir.check_types",
    "compiler.hir.evaluate_capability_policy",
    "compiler.hir.evaluate_ownership",
    "compiler.hir.evaluate_borrow_state",
    "compiler.hir.evaluate_property_clauses",
    "compiler.hir.export_public_api",
    "compiler.hir.infer_capability_use",
}
REQUIRED_ANALYSIS_INPUT_FIELDS = {
    "package_graph",
    "syntax_units",
    "diagnostics_context",
    "manifest_capability_policy",
    "source_span_index",
}
FORBIDDEN_ANALYSIS_INPUT_FIELDS = {
    "host_implementation_file",
    "host_package_metadata",
    "generated_source",
    "backend_artifact",
    "backend_runtime_diagnostic",
}
EXPECTED_CONTRACTS = {
    "typed_declaration_contract",
    "capability_policy_contract",
    "ownership_state_contract",
    "borrow_state_contract",
    "property_clause_contract",
    "agent_inspection_contract",
}
REQUIRED_DIAGNOSTIC_KINDS = {"type", "import", "ownership", "capability", "property"}
REQUIRED_DIAGNOSTIC_CODES = {
    "use_after_move",
    "move_while_borrowed",
    "borrow_return_requires_param_origin",
    "mutable_borrow_while_shared_live",
    "shared_borrow_while_mutable_live",
    "mutable_borrow_while_mutable_live",
    "property_failed",
}
SOURCE_FIELDS = {"path", "line", "column"}
RUST_CAPTURE_TERMS = {
    "rust",
    "cargo",
    "serde",
    "trait",
    "lifetime",
    "option",
    "result",
    "borrow checker",
    "borrowck",
    "hir.rs",
}
PROPERTY_PATTERN = re.compile(r"^\s*property\s+fn\s+[A-Za-z_][A-Za-z0-9_]*\s*\(", re.MULTILINE)


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
    elif expected_type == "boolean":
        require(isinstance(value, bool), f"{path} must be a boolean")
    elif expected_type is not None:
        fail(f"{path} uses unsupported schema type {expected_type}")


def reject_rust_capture_terms(values: list[str], path: str) -> None:
    for value in values:
        lowered = value.lower()
        for term in RUST_CAPTURE_TERMS:
            if " " in term or "." in term:
                found = term in lowered
            else:
                found = re.search(rf"(?<![a-z]){re.escape(term)}(?![a-z])", lowered) is not None
            require(not found, f"{path} must not expose Rust capture term {term!r}: {value}")


def reject_rust_capture_payload(value: Any, path: str = "$") -> None:
    if isinstance(value, dict):
        for key, nested in value.items():
            reject_rust_capture_payload(nested, f"{path}.{key}")
    elif isinstance(value, list):
        for index, nested in enumerate(value):
            reject_rust_capture_payload(nested, f"{path}[{index}]")
    elif isinstance(value, str):
        reject_rust_capture_terms([value], path)


def validate_package(snapshot: dict[str, Any]) -> None:
    package = snapshot["package"]
    require(package["name"] == CONTRACT, "package name mismatch")
    require(package["owner_lane"] == "Daedalus", "compiler.hir owner lane mismatch")
    require("typed declarations" in package["owns"], "compiler.hir must own typed declarations")
    require("capability policy verdicts" in package["owns"], "compiler.hir must own capability verdicts")
    require("ownership state" in package["owns"], "compiler.hir must own ownership state")
    require("borrow state" in package["owns"], "compiler.hir must own borrow state")
    require("property clause verdicts" in package["owns"], "compiler.hir must own property verdicts")


def validate_apis(snapshot: dict[str, Any]) -> None:
    apis = {api["name"]: api for api in snapshot["apis"]}
    require(set(apis) == EXPECTED_APIS, "compiler.hir API set mismatch")
    for name, api in apis.items():
        require(name.startswith("compiler.hir."), f"{name} must be package-qualified")
    require(
        "inferred_capability_use_records" in apis["compiler.hir.infer_capability_use"]["outputs"],
        "infer_capability_use must expose inferred_capability_use_records",
    )


def validate_analysis_input(snapshot: dict[str, Any]) -> None:
    analysis_input = snapshot["analysis_input"]
    required = set(analysis_input["required_fields"])
    forbidden = set(analysis_input["forbidden_required_fields"])
    require(REQUIRED_ANALYSIS_INPUT_FIELDS.issubset(required), "HIR analysis input missing required fields")
    require(FORBIDDEN_ANALYSIS_INPUT_FIELDS.issubset(forbidden), "HIR analysis input must forbid host/backend required fields")
    require(not required.intersection(forbidden), "HIR required fields conflict with forbidden fields")


def validate_contracts(snapshot: dict[str, Any]) -> None:
    contracts = {contract["name"]: contract for contract in snapshot["contracts"]}
    require(set(contracts) == EXPECTED_CONTRACTS, "HIR contract set mismatch")
    for name, contract in contracts.items():
        require(contract["source_correlated"] is True, f"{name} must be source-correlated")
    require("capability" in contracts["capability_policy_contract"]["diagnostic_kinds"], "capability contract must emit capability diagnostics")
    require("ownership" in contracts["ownership_state_contract"]["diagnostic_kinds"], "ownership contract must emit ownership diagnostics")
    require("ownership" in contracts["borrow_state_contract"]["diagnostic_kinds"], "borrow contract must emit ownership diagnostics")
    require("property" in contracts["property_clause_contract"]["diagnostic_kinds"], "property contract must emit property diagnostics")


def validate_diagnostics(snapshot: dict[str, Any]) -> None:
    diagnostics = snapshot["diagnostics"]
    require(REQUIRED_DIAGNOSTIC_KINDS.issubset(set(diagnostics["required_kinds"])), "HIR diagnostic kinds are incomplete")
    require(REQUIRED_DIAGNOSTIC_CODES.issubset(set(diagnostics["required_codes"])), "HIR diagnostic codes are incomplete")
    require(SOURCE_FIELDS.issubset(set(diagnostics["source_fields"])), "HIR diagnostics must include source fields")


def count_property_clauses(path: Path) -> int:
    count = 0
    for source in sorted(path.glob("*.ax")):
        count += len(PROPERTY_PATTERN.findall(source.read_text(encoding="utf-8")))
    return count


def validate_expected_error(name: str, fixture: dict[str, Any]) -> None:
    expected_error = fixture.get("expected_error")
    if not expected_error:
        return

    error_path = Path(expected_error)
    require(error_path.exists(), f"{name} expected-error fixture is missing: {error_path}")
    payload = load_json(error_path)
    assertions = set(fixture["assertions"])

    if "kind_ownership" in assertions:
        require(payload.get("kind") == "ownership", f"{name} must be an ownership diagnostic")
    if "kind_capability" in assertions:
        require(payload.get("kind") == "capability", f"{name} must be a capability diagnostic")
    if "kind_property" in assertions:
        require(payload.get("kind") == "property", f"{name} must be a property diagnostic")

    for assertion in assertions:
        prefix = "code_"
        if assertion.startswith(prefix):
            expected = assertion[len(prefix):]
            require(payload.get("code") == expected, f"{name} code mismatch: expected {expected}")

    if "source_correlated" in assertions:
        for field in SOURCE_FIELDS:
            require(field in payload, f"{name} diagnostic must include {field}")
        require(isinstance(payload["path"], str) and payload["path"], f"{name} diagnostic path must be non-empty")
        require(isinstance(payload["line"], int) and payload["line"] >= 1, f"{name} diagnostic line must be one-based")
        require(isinstance(payload["column"], int) and payload["column"] >= 1, f"{name} diagnostic column must be one-based")


def validate_fixtures(snapshot: dict[str, Any]) -> None:
    fixtures = {fixture["name"]: fixture for fixture in snapshot["fixtures"]}
    require("compiler_property_corpus" in fixtures, "compiler property corpus fixture missing")
    corpus = fixtures["compiler_property_corpus"]
    corpus_path = Path(corpus["path"])
    require(corpus_path.exists(), f"compiler property corpus path missing: {corpus_path}")
    if "property_count_at_least_100" in corpus["assertions"]:
        count = count_property_clauses(corpus_path)
        require(count >= 100, f"compiler property corpus has {count} property clauses; expected at least 100")

    for name, fixture in fixtures.items():
        path = Path(fixture["path"])
        require(path.exists(), f"{name} fixture path missing: {path}")
        validate_expected_error(name, fixture)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA)
    parser.add_argument("--snapshot", type=Path, default=DEFAULT_SNAPSHOT)
    parser.add_argument("--json", action="store_true", help="emit a JSON validation result")
    args = parser.parse_args()

    schema = load_json(args.schema)
    snapshot = load_json(args.snapshot)

    require(schema.get("$id", "").endswith("/axiom.compiler.hir_ownership_capability.v1.schema.json"), "schema $id must name HIR ownership/capability v1")
    require(schema.get("title") == "Axiom compiler HIR ownership and capability contract", "schema title changed unexpectedly")
    validate_against_schema(snapshot, schema)
    require(snapshot["schema_version"] == SCHEMA_VERSION, "snapshot schema_version mismatch")
    require(snapshot["contract"] == CONTRACT, "snapshot contract mismatch")
    require(snapshot["issue"] == 940, "snapshot issue mismatch")
    reject_rust_capture_payload(snapshot)
    validate_package(snapshot)
    validate_apis(snapshot)
    validate_analysis_input(snapshot)
    validate_contracts(snapshot)
    validate_diagnostics(snapshot)
    validate_fixtures(snapshot)

    result = {
        "schema": SCHEMA_VERSION,
        "ok": True,
        "apis": len(snapshot["apis"]),
        "contracts": len(snapshot["contracts"]),
        "fixtures": len(snapshot["fixtures"]),
        "fixture": str(args.snapshot),
    }
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(f"HIR boundary fixture ok: {result['apis']} APIs, {result['contracts']} contracts")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
