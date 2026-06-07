#!/usr/bin/env python3
"""Validate the compiler.commands and compiler.services.lsp boundary fixture."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "axiom.compiler.command_lsp.v1"
CONTRACT = "compiler.commands_lsp"
DEFAULT_SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.compiler.command_lsp.v1.schema.json")
DEFAULT_SNAPSHOT = Path("stage1/compiler-contracts/snapshots/command-lsp.json")
EXPECTED_COMMAND_APIS = {
    "check": "compiler.commands.check_package",
    "build": "compiler.commands.build_package",
    "run": "compiler.commands.run_artifact",
    "test": "compiler.commands.test_package",
    "doc": "compiler.commands.render_docs",
    "caps": "compiler.commands.describe_capabilities",
    "trace": "compiler.evidence.trace",
}
EXPECTED_LSP_APIS = {
    "serve_stdio": "compiler.services.lsp.serve_stdio",
    "initialize": "compiler.services.lsp.initialize",
    "open_document": "compiler.services.lsp.open_document",
    "change_document": "compiler.services.lsp.change_document",
    "publish_diagnostics": "compiler.services.lsp.publish_diagnostics",
    "shutdown": "compiler.services.lsp.shutdown",
    "exit": "compiler.services.lsp.exit",
}
REQUIRED_LSP_PACKAGES = {
    "compiler.package_graph",
    "compiler.syntax",
    "compiler.hir",
    "compiler.diagnostics",
    "compiler.evidence",
}
RUST_CAPTURE_TERMS = {
    "cargo",
    "main.rs",
    "lsp.rs",
    "project.rs",
    "clap",
    "serde",
    "rustc",
    "rust module",
    "rust-only",
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
    elif expected_type == "boolean":
        require(isinstance(value, bool), f"{path} must be a boolean")
    elif expected_type is not None:
        fail(f"{path} uses unsupported schema type {expected_type}")


def reject_rust_capture_terms(values: list[str], path: str) -> None:
    for value in values:
        lowered = value.lower()
        for term in RUST_CAPTURE_TERMS:
            require(term not in lowered, f"{path} must not expose Rust capture term {term!r}: {value}")


def validate_command_contract(snapshot: dict[str, Any]) -> None:
    commands = {command["name"]: command for command in snapshot["commands"]}
    require(set(commands) == set(EXPECTED_COMMAND_APIS), "command API set mismatch")

    for name, expected_api in EXPECTED_COMMAND_APIS.items():
        command = commands[name]
        require(command["api"] == expected_api, f"{name} API mismatch")
        expected_package = "compiler.evidence" if name == "trace" else "compiler.commands"
        require(command["package"] == expected_package, f"{name} package owner mismatch")
        require(command["api"].startswith("compiler."), f"{name} API must be package-qualified")
        reject_rust_capture_terms(
            [command["api"], command["stable_output"], *command["inputs"], *command["delegates_to"]],
            f"commands.{name}",
        )
        for fixture in command.get("fixtures", []):
            path = Path(fixture)
            require(path.exists(), f"{name} fixture path does not exist: {fixture}")

    require("compiler.evidence.trace_graph" in commands["trace"]["delegates_to"], "trace must delegate to evidence trace graph")
    require(commands["trace"]["stable_output"] == "axiom.trace.v0", "trace must preserve axiom.trace.v0")


def validate_lsp_contract(snapshot: dict[str, Any]) -> None:
    services = {service["flow"]: service for service in snapshot["lsp_services"]}
    require(set(services) == set(EXPECTED_LSP_APIS), "LSP service API set mismatch")

    delegated_packages: set[str] = set()
    for flow, expected_api in EXPECTED_LSP_APIS.items():
        service = services[flow]
        require(service["api"] == expected_api, f"{flow} LSP API mismatch")
        reject_rust_capture_terms(
            [service["api"], service["protocol"], *service["delegates_to"]],
            f"lsp_services.{flow}",
        )
        for delegated in service["delegates_to"]:
            parts = delegated.split(".")
            if len(parts) >= 2:
                delegated_packages.add(".".join(parts[:2]))

    require(REQUIRED_LSP_PACKAGES.issubset(delegated_packages), "LSP services must call package graph, syntax, HIR, diagnostics, and evidence packages")
    framing = snapshot["protocols"]["lsp_framing"]
    require(framing["transport"] == "stdio", "LSP transport must remain stdio")
    require(framing["header"] == "Content-Length", "LSP framing must preserve Content-Length")
    require(framing["body"] == "JSON-RPC 2.0", "LSP body must remain JSON-RPC 2.0")


def validate_protocols(snapshot: dict[str, Any]) -> None:
    envelope_map = {
        envelope["schema"]: set(envelope["commands"])
        for envelope in snapshot["protocols"]["json_envelopes"]
    }
    require(envelope_map.get("axiom.stage1.v1") == {"check", "build", "run", "test", "doc", "caps"}, "stage1 JSON envelope command set mismatch")
    require(envelope_map.get("axiom.trace.v0") == {"trace"}, "trace JSON envelope command set mismatch")

    release = snapshot["official_release"]
    require(release["requires_cargo"] is False, "official release command behavior must not require Cargo")
    require(release["requires_rustc"] is False, "official release command behavior must not require rustc")
    require(release["temporary_developer_path"] is True, "temporary developer path must remain explicit while Rust host exists")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA)
    parser.add_argument("--snapshot", type=Path, default=DEFAULT_SNAPSHOT)
    parser.add_argument("--json", action="store_true", help="emit a JSON validation result")
    args = parser.parse_args()

    schema = load_json(args.schema)
    snapshot = load_json(args.snapshot)

    require(schema.get("$id", "").endswith("/axiom.compiler.command_lsp.v1.schema.json"), "schema $id must name command/LSP v1")
    require(schema.get("title") == "Axiom compiler command and LSP package contract", "schema title changed unexpectedly")
    validate_against_schema(snapshot, schema)
    require(snapshot["schema_version"] == SCHEMA_VERSION, "snapshot schema_version mismatch")
    require(snapshot["contract"] == CONTRACT, "snapshot contract mismatch")
    require(snapshot["issue"] == 938, "snapshot issue mismatch")
    validate_command_contract(snapshot)
    validate_lsp_contract(snapshot)
    validate_protocols(snapshot)

    result = {
        "schema": SCHEMA_VERSION,
        "ok": True,
        "commands": len(snapshot["commands"]),
        "lsp_services": len(snapshot["lsp_services"]),
        "fixture": str(args.snapshot),
    }
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(f"command/LSP boundary fixture ok: {result['commands']} commands, {result['lsp_services']} services")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
