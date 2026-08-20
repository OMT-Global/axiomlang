#!/usr/bin/env python3
"""Validate the Native Debugging v1 evidence contract and fail-closed DAP status."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.native_debugging.v1.schema.json")
STATUS_SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.native_debug_status.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/native-debugging-v1.json")
FIXTURES = Path("stage1/compiler-contracts/fixtures/native-debugging-v1")
DAP_SOURCE = Path("stage1/crates/axiomc/src/dap.rs")
PROJECT_SOURCE = Path("stage1/crates/axiomc/src/project.rs")
DEBUG_DOC = Path("docs/stage1-debug-map.md")

BLOCKERS = {1436, 1455}
DEPENDENCIES = {1436, 1437, 1455, 1457}
OPERATIONS = {"attach", "breakpoints", "continue", "evaluate", "exit", "launch", "pause", "scopes", "signals", "stacks", "step", "threads", "variables"}
IDENTITY_FIELDS = {"binary_digest", "runtime_state", "source_generation", "target"}
RUNTIME_STATES = {"not_started", "source_simulation_stopped", "source_simulation_terminated"}
TARGETS = {"aarch64-apple-darwin", "x86_64-unknown-linux-gnu"}
DWARF_EVIDENCE = {"function_symbols", "line_tables", "representative_locals", "source_paths", "stack_frames"}
DEBUGGER_TOOLS = {"gdb", "lldb"}
DEBUGGER_ACTIONS = {"backtrace", "breakpoint", "locals", "source_fidelity", "step"}
PROFILE_IDENTITY = {"binary_digest", "source_generation", "target"}
PROFILE_SAMPLES = {"source_span", "stable_axiom_symbol", "weighted_sample_count"}
SIDECAR_LIMITS = {"native_breakpoint_installation", "native_dwarf", "process_runtime_state", "symbolized_profile"}
FIXTURE_NAMES = {"current-source-simulator", "missing-process-proof", "sidecar-only-rejected", "unverified-breakpoint"}
STATUS_SCHEMA_VERSION = "axiom.native_debug_status.v1"


class ContractError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def load(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"unable to load {path}: {error}") from error


def value_type(value: Any) -> str:
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        return "object"
    if value is None:
        return "null"
    return "number"


def json_equal(left: Any, right: Any) -> bool:
    if isinstance(left, bool) or isinstance(right, bool):
        return isinstance(left, bool) and isinstance(right, bool) and left == right
    if isinstance(left, (int, float)) or isinstance(right, (int, float)):
        return (
            isinstance(left, (int, float))
            and not isinstance(left, bool)
            and isinstance(right, (int, float))
            and not isinstance(right, bool)
            and left == right
        )
    return type(left) is type(right) and left == right


def validate_schema(value: Any, schema: dict[str, Any], path: str = "$") -> None:
    if "oneOf" in schema:
        matches = 0
        for candidate in schema["oneOf"]:
            try:
                validate_schema(value, candidate, path)
            except ContractError:
                continue
            matches += 1
        require(matches == 1, f"{path}: expected exactly one matching schema")
    if "const" in schema:
        require(json_equal(value, schema["const"]), f"{path}: const mismatch")
    if "enum" in schema:
        require(any(json_equal(value, candidate) for candidate in schema["enum"]), f"{path}: enum mismatch")
    expected = schema.get("type")
    if expected:
        require(value_type(value) == expected, f"{path}: expected {expected}")
    if isinstance(value, dict):
        properties = schema.get("properties", {})
        missing = sorted(set(schema.get("required", [])) - set(value))
        require(not missing, f"{path}: missing {', '.join(missing)}")
        if schema.get("additionalProperties") is False:
            unexpected = sorted(set(value) - set(properties))
            require(not unexpected, f"{path}: unknown fields {', '.join(unexpected)}")
        for field, nested in value.items():
            if field in properties:
                validate_schema(nested, properties[field], f"{path}.{field}")
    if isinstance(value, list):
        require(len(value) >= schema.get("minItems", 0), f"{path}: too few items")
        require(len(value) <= schema.get("maxItems", len(value)), f"{path}: too many items")
        if schema.get("uniqueItems"):
            encoded = [json.dumps(item, sort_keys=True, separators=(",", ":")) for item in value]
            require(len(encoded) == len(set(encoded)), f"{path}: duplicate items")
        if "items" in schema:
            for index, item in enumerate(value):
                validate_schema(item, schema["items"], f"{path}[{index}]")
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: empty string")
        if schema.get("pattern"):
            require(re.search(schema["pattern"], value) is not None, f"{path}: pattern mismatch")


def sorted_unique(values: list[str], expected: set[str], field: str) -> None:
    require(values == sorted(expected), f"{field} must be complete, sorted, and unique")


def validate_fixture(name: str, fixture: dict[str, Any]) -> None:
    if name == "current-source-simulator":
        require(fixture.get("schemaVersion") == STATUS_SCHEMA_VERSION, "source simulator status schema drifted")
        require(fixture.get("mode") == "source_simulator", "source simulator mode drifted")
        for field in ("processBacked", "nativeAxiomDwarf", "profileSymbolization"):
            require(fixture.get(field) is False, f"source simulator falsely claims {field}")
        require(fixture.get("runtimeState") == "not_started", "source simulator initial state drifted")
        require(fixture.get("identity") == {"binaryDigest": None, "sourceGeneration": None, "target": None}, "source simulator must expose unavailable native identity")
        require(fixture.get("unavailableReason") == "native_debugging.dependencies_unmet", "source simulator reason drifted")
        require(set(fixture.get("blockerIssues", [])) == BLOCKERS, "source simulator blockers drifted")
        return
    if name == "unverified-breakpoint":
        response = fixture.get("response", {})
        require(response.get("verified") is False, "source-line match was reported as a native breakpoint")
        require(response.get("axiomSourceResolved") is True, "source-line resolution evidence is missing")
        require("no process-backed native breakpoint" in response.get("message", ""), "breakpoint limitation is not explicit")
        return
    if name == "sidecar-only-rejected":
        require(fixture.get("claim") == "native_debugging_qualified", "sidecar claim drifted")
        require(fixture.get("decision") == "rejected", "sidecar-only evidence was accepted")
        require(fixture.get("native_dwarf") is False and fixture.get("process_backed_dap") is False and fixture.get("profile_symbolization") is False, "sidecar fixture contains native proof")
        require(fixture.get("diagnostic") == "native_debugging.sidecar_only", "sidecar diagnostic drifted")
        return
    if name == "missing-process-proof":
        evidence = fixture.get("evidence", {})
        require(fixture.get("claim") == "process_backed_dap", "process claim drifted")
        require(all(evidence.get(field) is False for field in ("process_launched", "native_breakpoint_confirmed", "runtime_state_observed")), "missing-process fixture contains runtime proof")
        require(fixture.get("decision") == "rejected", "missing process proof was accepted")
        require(fixture.get("diagnostic") == "native_debugging.process_proof_missing", "process diagnostic drifted")
        return
    raise ContractError(f"unknown Native Debugging v1 fixture {name}")


def validate_implementation(root: Path) -> None:
    dap = (root / DAP_SOURCE).read_text(encoding="utf-8")
    project = (root / PROJECT_SOURCE).read_text(encoding="utf-8")
    doc = (root / DEBUG_DOC).read_text(encoding="utf-8").lower()
    required_dap_snippets = (
        'const NATIVE_DEBUG_STATUS_SCHEMA_VERSION: &str = "axiom.native_debug_status.v1"',
        'const SOURCE_SIMULATOR_MODE: &str = "source-simulator"',
        '"axiom/debugStatus"',
        '"processBacked": false',
        '"nativeAxiomDwarf": false',
        '"profileSymbolization": false',
        '"verified": false',
        '"axiomSourceResolved": breakpoint.source_resolved',
        "process-backed launch is not implemented",
        "active stopped source-simulator session",
    )
    for snippet in required_dap_snippets:
        require(snippet in dap, f"DAP fail-closed evidence is missing {snippet!r}")
    for forbidden in ("std::process::Command", "Command::new(", ".spawn("):
        require(forbidden not in dap, f"DAP source contains unqualified process control {forbidden!r}")
    require("axiom_dwarf: true" not in project, "build manifests claim native AxiOM DWARF without proof")
    require(project.count("axiom_dwarf: false") >= 2, "build manifests no longer expose false native-DWARF status")
    require("sidecar" in doc and "not present yet" in doc, "debug-map documentation no longer states the native-DWARF limitation")


def validate_contract(root: Path) -> dict[str, Any]:
    schema = load(root / SCHEMA)
    status_schema = load(root / STATUS_SCHEMA)
    snapshot = load(root / SNAPSHOT)
    require(schema.get("$id", "").endswith("axiom.native_debugging.v1.schema.json"), "Native Debugging v1 schema id drifted")
    require(status_schema.get("$id", "").endswith("axiom.native_debug_status.v1.schema.json"), "Native Debug Status v1 schema id drifted")
    validate_schema(snapshot, schema)
    require((snapshot["schema_version"], snapshot["contract"], snapshot["issue"]) == ("axiom.native_debugging.v1", "native.debugging", 1466), "Native Debugging v1 identity drifted")

    implementation = snapshot["implementation"]
    require(set(implementation["blockers"]) == BLOCKERS, "native debugging blocker boundary drifted")
    require(not any(implementation[field] for field in ("process_backed", "native_axiom_dwarf", "profile_symbolization")), "static spike claims unavailable native proof")

    dap = snapshot["dap"]
    sorted_unique(dap["required_operations"], OPERATIONS, "DAP operations")
    sorted_unique(dap["identity_fields"], IDENTITY_FIELDS, "DAP identity fields")
    sorted_unique(dap["runtime_states"], RUNTIME_STATES, "DAP runtime states")
    require(dap["status_schema"] == STATUS_SCHEMA_VERSION, "DAP status schema drifted")
    require(dap["source_simulator_opt_in"] is True, "source simulator must require explicit opt-in")

    native_dwarf = snapshot["native_dwarf"]
    sorted_unique(native_dwarf["targets"], TARGETS, "native DWARF targets")
    sorted_unique(native_dwarf["required_evidence"], DWARF_EVIDENCE, "native DWARF evidence")
    debugger = snapshot["debugger_matrix"]
    sorted_unique(debugger["tools"], DEBUGGER_TOOLS, "debugger tools")
    sorted_unique(debugger["actions"], DEBUGGER_ACTIONS, "debugger actions")
    require(debugger["target_bound"] is True and debugger["real_binary_required"] is True, "debugger proof must bind a real target binary")

    profiling = snapshot["profiling"]
    sorted_unique(profiling["identity_fields"], PROFILE_IDENTITY, "profile identity fields")
    sorted_unique(profiling["sample_requirements"], PROFILE_SAMPLES, "profile sample requirements")
    sidecars = snapshot["sidecars"]
    require(sidecars["supplemental_only"] is True, "sidecars were promoted to authoritative proof")
    sorted_unique(sidecars["cannot_prove"], SIDECAR_LIMITS, "sidecar proof limits")
    require(set(snapshot["migration"]["dependencies"]) == DEPENDENCIES, "native debugging dependencies drifted")

    seen: set[str] = set()
    for fixture_spec in snapshot["fixtures"]:
        name = fixture_spec["id"].rsplit("/", 1)[-1]
        require(name not in seen, f"duplicate Native Debugging v1 fixture {name}")
        seen.add(name)
        fixture = load(root / FIXTURES / fixture_spec["path"])
        validate_fixture(name, fixture)
        if name == "current-source-simulator":
            validate_schema(fixture, status_schema)
    require(seen == FIXTURE_NAMES, "Native Debugging v1 fixture coverage is incomplete")
    validate_implementation(root)
    return {"schema": snapshot["schema_version"], "status_schema": dap["status_schema"], "ok": True, "fixtures": len(seen), "targets": len(native_dwarf["targets"]), "operations": len(dap["required_operations"])}


def parse_dap_response(output: bytes) -> dict[str, Any]:
    header, separator, body = output.partition(b"\r\n\r\n")
    require(bool(separator), "adapter output has no DAP header terminator")
    content_length: int | None = None
    for line in header.split(b"\r\n"):
        name, delimiter, value = line.partition(b":")
        if delimiter and name.strip().lower() == b"content-length":
            try:
                content_length = int(value.strip())
            except ValueError as error:
                raise ContractError("adapter output has an invalid Content-Length") from error
    require(content_length is not None, "adapter output has no Content-Length")
    require(len(body) == content_length, "adapter output contains a truncated or extra DAP payload")
    try:
        payload = json.loads(body)
    except json.JSONDecodeError as error:
        raise ContractError(f"adapter output is not JSON: {error}") from error
    require(isinstance(payload, dict), "adapter output must be a JSON object")
    return payload


def adapter_target_dir(root: Path) -> Path:
    root_key = hashlib.sha256(str(root.resolve()).encode("utf-8")).hexdigest()[:16]
    return Path(tempfile.gettempdir()) / f"axiom-native-debugging-v1-{root_key}"


def read_adapter_status(root: Path) -> dict[str, Any]:
    request = json.dumps(
        {
            "seq": 1,
            "type": "request",
            "command": "axiom/debugStatus",
            "arguments": {},
        },
        separators=(",", ":"),
    ).encode("utf-8")
    framed = f"Content-Length: {len(request)}\r\n\r\n".encode("ascii") + request
    command = [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str((root / "stage1/Cargo.toml").resolve()),
        "-p",
        "axiomc",
        "--",
        "dap",
    ]
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(adapter_target_dir(root))
    try:
        completed = subprocess.run(
            command,
            cwd=root,
            env=environment,
            input=framed,
            capture_output=True,
            check=False,
            timeout=180,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ContractError(f"unable to execute the DAP adapter: {error}") from error
    require(
        completed.returncode == 0,
        "DAP adapter failed: " + completed.stderr.decode("utf-8", errors="replace").strip(),
    )
    response = parse_dap_response(completed.stdout)
    require(response.get("type") == "response", "adapter status output is not a DAP response")
    require(response.get("command") == "axiom/debugStatus", "adapter status command drifted")
    require(response.get("success") is True, "adapter rejected axiom/debugStatus")
    status = response.get("body")
    require(isinstance(status, dict), "adapter status response has no object body")
    return status


def validate_adapter_status(root: Path) -> dict[str, Any]:
    status = read_adapter_status(root)
    validate_schema(status, load(root / STATUS_SCHEMA))
    fixture = load(root / FIXTURES / "current-source-simulator.json")
    require(status == fixture, "actual adapter status drifted from the checked initial-state fixture")
    return status


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
        status = validate_adapter_status(args.root)
        result["adapter_status_schema"] = status["schemaVersion"]
    except (ContractError, KeyError, OSError, TypeError) as error:
        if args.json:
            print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True))
        else:
            print(f"native-debugging-v1: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "native-debugging-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
