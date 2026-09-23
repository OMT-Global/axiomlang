#!/usr/bin/env python3
"""Validate the offline, portable Runtime Process v1 contract and fixtures."""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any, Callable


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.runtime_process.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/runtime-process-v1.json")
FIXTURES = Path("stage1/compiler-contracts/fixtures/process-v1")

IMPLEMENTATION_EVIDENCE = (
    Path("stage1/crates/axiomc-backend-cranelift/src/lib.rs"),
    Path("stage1/crates/axiomc/src/codegen.rs"),
    Path("stage1/crates/axiomc/src/stdlib.rs"),
    Path("stage1/examples/stdlib_process/axiom.toml"),
    Path("stage1/runtime-abi/direct-native-v0.json"),
)
BLOCKERS = [1425, 1426, 1434, 1438, 1445, 1477]
LEGACY_SEMANTICS = (
    "POSIX direct-native and generated-native execution use one exact executable value with no arguments; "
    "Windows direct-native execution passes command text to system and is shell-parsing, so it is non-qualifying "
    "legacy evidence; all paths inherit cwd, environment, and stdio and report synchronous status only"
)

COMMAND = {
    "fields": ["argv", "cwd", "environment", "executable", "resource_limits", "stdio", "timeout_ms"],
    "implicit_shell": False,
    "executable": "one exact executable identity",
    "argv": "ordered UTF-8 arguments preserved as distinct values with argv[0] equal to executable identity",
    "cwd": "optional authority-checked directory identity",
    "environment": "inherit nothing by default; pass only policy-approved keys with explicit values",
}
STDIO = {
    "modes": ["capture", "inherit", "null", "stream"],
    "independent_stdout_stderr": True,
    "stdin_close": "close after input is written or immediately when no input is supplied",
    "capture_overflow": "terminate child and report process.output_limit_exceeded with bounded retained bytes",
}
LIFECYCLE_STATES = {"cancelled", "exited", "running", "signaled", "spawn_failed", "spawning", "timed_out"}
LIFECYCLE_TRANSITIONS = {
    "cancelled->exited",
    "cancelled->signaled",
    "running->cancelled",
    "running->exited",
    "running->signaled",
    "running->timed_out",
    "spawning->running",
    "spawning->spawn_failed",
    "timed_out->exited",
    "timed_out->signaled",
}
TERMINAL_STATES = {"exited", "signaled", "spawn_failed"}
PORTABLE_SIGNALS = {"interrupt", "kill", "terminate"}
TERMINAL_OUTCOMES = {
    "cancelled",
    "exit_code",
    "output_limit_exceeded",
    "resource_limit_exceeded",
    "signal",
    "spawn_error",
    "timeout",
}
LIFECYCLE_TEXT = {
    "initiating_outcome_preservation": {
        "cancelled": {"terminal_states": ["exited", "signaled"], "outcome": "cancelled"},
        "timed_out": {"terminal_states": ["exited", "signaled"], "outcome": "timeout"},
    },
    "timeout_clock": "monotonic",
    "cancellation": "request graceful termination, wait bounded grace, then force termination",
}
TERMINAL = {
    "detection": "query each inherited or streamed endpoint independently",
    "size": "optional positive columns and rows from the attached terminal endpoint",
    "color_policy": ["always", "auto", "never"],
    "non_terminal_size": "unavailable",
}

AUTHORITY_DIMENSIONS = {"argv", "cwd", "environment", "executable", "process_control", "signals", "stdio", "terminal"}
AUTHORITY_RULES = {
    "deny_by_default": True,
    "dimensions": sorted(AUTHORITY_DIMENSIONS),
    "executable_rule": "exact executable identity must be allowed before spawn",
    "argv_rule": "argument count, origins, and structural policy are checked separately from executable identity",
    "cwd_rule": "resolved cwd must remain inside an allowed root",
    "environment_rule": "inheritance and each key are independently authorized; values never grant authority",
    "process_control_rule": "caller wait, graceful termination, force termination, timeout handling, cancellation, and resource-limit ceilings are independently authorized",
    "runtime_cleanup_rule": "process-control denial blocks only the caller-requested operation; runtime supervision remains mandatory, closes pipes and reaps every child, and handle abandonment triggers bounded graceful-then-forced termination",
    "runtime_cleanup": {
        "ownership": "runtime_owned",
        "denial_scope": "caller_requested_operation_only",
        "supervision_after_denial": True,
        "abandonment": {
            "grace_ms": 5000,
            "graceful_termination_requested": True,
            "force_termination_if_running": True,
            "pipes_closed": True,
            "child_reaped": True,
        },
    },
    "signals_rule": "signal subscription and each portable signal send are independently authorized",
    "stdio_rule": "stdin, stdout, and stderr modes are authorized independently before spawn",
    "terminal_rule": "terminal detection, size queries, and terminal inheritance or streaming are independently authorized",
}
AUTHORITY_OPERATIONS = {
    "argv": ("spawn_preflight", "set_arguments"),
    "cwd": ("spawn_preflight", "set_working_directory"),
    "environment": ("spawn_preflight", "set_environment"),
    "executable": ("spawn_preflight", "select_executable"),
    "process_control": ("runtime_control", "wait_or_terminate"),
    "signals": ("runtime_control", "subscribe_or_send_signal"),
    "stdio": ("spawn_preflight", "configure_stdio"),
    "terminal": ("spawn_preflight", "request_terminal_access"),
}

AUDIT_FIELDS = {
    "argv_count",
    "argv_origins",
    "authority_dimension",
    "cwd_identity",
    "decision",
    "denied_dimensions",
    "environment_keys",
    "executable_identity",
    "operation",
    "stdio_modes",
    "terminal_requested",
}
REDACTED_FIELDS = {"argv_values", "environment_values", "stdin_bytes"}
AUDIT = {
    "decision_before_spawn": True,
    "decision_before_operation": True,
    "fields": sorted(AUDIT_FIELDS),
    "redacted_fields": sorted(REDACTED_FIELDS),
    "denied_rule": "record denied dimensions and stable identities without spawning or recording argument, environment, or stdin values",
}

RESOURCE_LIMITS = [
    {"name": "cpu_time_ms", "unit": "milliseconds", "minimum": 1, "default": 30000, "maximum": 300000},
    {"name": "memory_bytes", "unit": "bytes", "minimum": 1048576, "default": 268435456, "maximum": 1073741824},
    {"name": "open_files", "unit": "count", "minimum": 3, "default": 64, "maximum": 1024},
    {"name": "subprocesses", "unit": "count", "minimum": 0, "default": 1, "maximum": 64},
]
RESOURCE_FAILURES = {
    "invalid_request": "reject before spawn with process.resource_limit_invalid",
    "unauthorized": "reject before spawn with process.capability_denied",
    "unsupported_host": "reject before spawn with process.resource_limit_unsupported",
    "exceeded": "terminate and clean up with process.resource_limit_exceeded and the exceeded limit name",
}
RESOURCE_LIMIT_CONTRACT = {
    "supported": RESOURCE_LIMITS,
    "default_rule": "omitted values use finite published defaults; process_control authority may lower defaults but never remove a bound",
    "authority_rule": "process_control authority sets a finite ceiling per supported limit; a request above its ceiling is denied before spawn",
    "failures": RESOURCE_FAILURES,
}

BOUNDS = {
    "max_argv_entries": 256,
    "max_argument_bytes": 65536,
    "max_environment_entries": 128,
    "max_capture_bytes_per_stream": 1048576,
    "max_stdin_bytes": 1048576,
    "max_grace_ms": 5000,
}
INSPECTION_FIELDS = {
    "argv_count",
    "argv_origins",
    "authority_decision",
    "cancellation_state",
    "cwd_identity",
    "environment_keys",
    "executable_identity",
    "exit_reason",
    "resource_limits",
    "stdio_modes",
    "terminal_dependencies",
    "timeout_ms",
}
MIGRATION = {
    "compatibility": "run_status(command: string) remains a legacy synchronous status helper; it does not satisfy or implicitly opt into the structured Process v1 contract",
    "dependencies": BLOCKERS,
    "out_of_scope": [
        "host-specific process object layout",
        "platform-specific signal numbers",
        "shell command parsing",
        "terminal escape sequence implementation",
    ],
    "forbidden_terms": ["Cargo process wrapper", "POSIX wait status", "Rust std::process", "shell command string"],
}
FIXTURE_SPECS = {
    "argv-unicode": (
        "positive",
        ["argument boundaries, ordering, spaces, and Unicode survive without shell parsing"],
    ),
    "capability-denied": (
        "negative",
        ["every authority dimension has an allowed path and a denied path that cannot perform its protected action"],
    ),
    "environment-redaction": (
        "positive",
        ["audit reports approved keys and redacts environment, argument, and stdin values"],
    ),
    "large-output": (
        "negative",
        ["capture is bounded and overflow terminates the child with a stable output-limit outcome and diagnostic"],
    ),
    "resource-limits": (
        "negative",
        ["supported limits use finite defaults and authority ceilings while invalid, unauthorized, unsupported, and exceeded limits fail closed"],
    ),
    "shell-literal": (
        "positive",
        ["shell metacharacters remain one literal argument"],
    ),
    "signaled": (
        "positive",
        ["portable signal outcome is distinct from exit code"],
    ),
    "terminal": (
        "positive",
        ["terminal detection, size, and color policy are explicit"],
    ),
    "timeout-cancellation": (
        "negative",
        ["timeout and explicit cancellation have distinct bounded graceful-then-forced paths and outcomes"],
    ),
}
CAPTURE_TERMS = {"cargo", "cranelift", "execv", "fork", "posix", "rust", "sigint", "sigterm", "std::process", "waitpid"}


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
        return
    if "const" in schema:
        require(value == schema["const"], f"{path}: const mismatch")
    if "enum" in schema:
        require(value in schema["enum"], f"{path}: enum mismatch")
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
            encoded = [json.dumps(item, sort_keys=True) for item in value]
            require(len(encoded) == len(set(encoded)), f"{path}: duplicate items")
        if "items" in schema:
            for index, item in enumerate(value):
                validate_schema(item, schema["items"], f"{path}[{index}]")
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: empty string")
        if schema.get("pattern"):
            require(re.search(schema["pattern"], value) is not None, f"{path}: pattern mismatch")
    if isinstance(value, int) and not isinstance(value, bool):
        require(value >= schema.get("minimum", value), f"{path}: below minimum")
        require(value <= schema.get("maximum", value), f"{path}: above maximum")


def require_exact(actual: Any, expected: Any, path: str) -> None:
    if isinstance(expected, dict):
        require(isinstance(actual, dict), f"{path}: expected object")
        missing = sorted(set(expected) - set(actual))
        unexpected = sorted(set(actual) - set(expected))
        require(not missing, f"{path}: missing fields {', '.join(missing)}")
        require(not unexpected, f"{path}: unknown fields {', '.join(unexpected)}")
        for key in sorted(expected):
            require_exact(actual[key], expected[key], f"{path}.{key}")
        return
    if isinstance(expected, list):
        require(isinstance(actual, list), f"{path}: expected array")
        require(len(actual) == len(expected), f"{path}: expected {len(expected)} items")
        for index, item in enumerate(expected):
            require_exact(actual[index], item, f"{path}[{index}]")
        return
    require(type(actual) is type(expected), f"{path}: expected {value_type(expected)}")
    require(actual == expected, f"{path}: expected {expected!r}")


def expected_authority_fixture() -> dict[str, Any]:
    checks = []
    for dimension in sorted(AUTHORITY_DIMENSIONS):
        phase, operation = AUTHORITY_OPERATIONS[dimension]
        checks.append(
            {
                "dimension": dimension,
                "phase": phase,
                "operation": operation,
                "allow": {"decision": "allowed", "protected_action_performed": True},
                "deny": {
                    "decision": "denied",
                    "diagnostic": "process.capability_denied",
                    "protected_action_performed": False,
                },
            }
        )
    return {
        "checks": checks,
        "pre_spawn_denial": {"child_started": False},
        "runtime_denial": {
            "control_effect_applied": False,
            "runtime_supervision": {
                "authority": "runtime_owned",
                "child_observed_until_terminal": True,
                "pipes_closed": True,
                "child_reaped": True,
            },
        },
        "handle_abandonment": {
            "grace_ms": 10,
            "graceful_termination_requested": True,
            "force_termination_if_running": True,
            "pipes_closed": True,
            "child_reaped": True,
        },
    }


def expected_resource_limit_fixture(snapshot: dict[str, Any]) -> dict[str, Any]:
    limits = {item["name"]: item for item in snapshot["resource_limits"]["supported"]}
    ceilings = {
        "cpu_time_ms": 1000,
        "memory_bytes": 134217728,
        "open_files": 32,
        "subprocesses": 1,
    }
    return {
        "request": {
            "limits": {"cpu_time_ms": 250, "memory_bytes": 67108864},
            "omitted_limits": ["open_files", "subprocesses"],
        },
        "authority": {
            "dimension": "process_control",
            "ceilings": ceilings,
            "decision": "allowed",
        },
        "effective": {
            "cpu_time_ms": 250,
            "memory_bytes": 67108864,
            "open_files": min(limits["open_files"]["default"], ceilings["open_files"]),
            "subprocesses": min(limits["subprocesses"]["default"], ceilings["subprocesses"]),
        },
        "invalid_request": {
            "limit": "open_files",
            "requested": 0,
            "diagnostic": "process.resource_limit_invalid",
            "spawned": False,
        },
        "denied_request": {
            "limit": "memory_bytes",
            "requested": 268435456,
            "authority_ceiling": ceilings["memory_bytes"],
            "diagnostic": "process.capability_denied",
            "spawned": False,
        },
        "unsupported_host": {
            "limit": "subprocesses",
            "diagnostic": "process.resource_limit_unsupported",
            "spawned": False,
        },
        "exceeded": {
            "limit": "cpu_time_ms",
            "effective_value": 250,
            "observed_value": 251,
            "outcome": "resource_limit_exceeded",
            "diagnostic": "process.resource_limit_exceeded",
            "child_terminated": True,
            "cleanup_observed": True,
        },
    }


def expected_fixture(name: str, snapshot: dict[str, Any]) -> dict[str, Any]:
    if name == "argv-unicode":
        argv = ["axiom://fixture/process/argv-echo", "two words", "café", "🧪"]
        return {
            "command": {"executable": argv[0], "argv": argv},
            "observed": {"argv": argv, "implicit_shell": False, "ordering": "preserved", "encoding": "utf-8"},
        }
    if name == "capability-denied":
        return expected_authority_fixture()
    if name == "environment-redaction":
        return {
            "request": {
                "argv": ["axiom://fixture/process/env", "argument-value"],
                "environment": {
                    "SAFE_FLAG": "enabled",
                    "TOKEN": "example",
                },
                "stdin": "input-value",
            },
            "audit": {
                "argv_count": 2,
                "argv_origins": ["executable", "literal"],
                "environment_keys": ["SAFE_FLAG", "TOKEN"],
                "redacted_fields": sorted(REDACTED_FIELDS),
                "values_present": False,
            },
        }
    if name == "large-output":
        limit = snapshot["bounds"]["max_capture_bytes_per_stream"]
        return {
            "limit_bytes": limit,
            "produced_bytes": limit + 1,
            "captured_bytes": limit,
            "truncated": True,
            "child_terminated": True,
            "outcome": "output_limit_exceeded",
            "diagnostic": "process.output_limit_exceeded",
        }
    if name == "resource-limits":
        return expected_resource_limit_fixture(snapshot)
    if name == "shell-literal":
        argv = ["axiom://fixture/process/argv-echo", "$(fixture-side-effect)", "a; b", "*.ax"]
        return {
            "command": {"executable": argv[0], "argv": argv},
            "observed": {"argv": argv, "shell_evaluated": False, "side_effects": []},
        }
    if name == "signaled":
        return {
            "request": {"signal": "terminate", "authority": "allowed"},
            "outcome": {
                "exit_reason": "signal",
                "portable_signal": "terminate",
                "exit_code_available": False,
                "cleanup_observed": True,
            },
        }
    if name == "terminal":
        return {
            "stdout": {"is_terminal": True, "columns": 120, "rows": 40},
            "stderr": {"is_terminal": False, "size": "unavailable"},
            "color": {"policy": "auto", "enabled": True, "source": "stdout terminal detection"},
        }
    if name == "timeout-cancellation":
        return {
            "timeout": {
                "request": {"timeout_ms": 25, "grace_ms": 10},
                "transitions": ["spawning", "running", "timed_out", "exited"],
                "termination": {"graceful_signal": "terminate", "forced_signal": "kill", "forced_after_grace": False},
                "outcome": {
                    "exit_reason": "timeout",
                    "diagnostic": "process.timed_out",
                    "cancellation_state": "not_requested",
                    "cleanup_observed": True,
                },
            },
            "cancellation": {
                "request": {"cancel_requested": True, "grace_ms": 10},
                "transitions": ["spawning", "running", "cancelled", "signaled"],
                "termination": {"graceful_signal": "terminate", "forced_signal": "kill", "forced_after_grace": True},
                "outcome": {
                    "exit_reason": "cancelled",
                    "diagnostic": "process.cancelled",
                    "cancellation_state": "completed",
                    "cleanup_observed": True,
                },
            },
        }
    raise ContractError(f"unknown fixture {name}")


def validate_fixture(name: str, fixture: dict[str, Any], snapshot: dict[str, Any]) -> None:
    require_exact(fixture, expected_fixture(name, snapshot), f"fixture.{name}")
    if name == "environment-redaction":
        audit_text = json.dumps(fixture["audit"], sort_keys=True)
        request = fixture["request"]
        secrets = [*request["argv"][1:], *request["environment"].values(), request["stdin"]]
        require(all(secret not in audit_text for secret in secrets), "fixture.environment-redaction.audit leaked a process value")
    elif name == "large-output":
        require(fixture["produced_bytes"] > fixture["limit_bytes"], "fixture.large-output must exceed its capture limit")
        require(fixture["captured_bytes"] == fixture["limit_bytes"], "fixture.large-output capture is not bounded")
        require(fixture["outcome"] == "output_limit_exceeded", "fixture.large-output.outcome drifted")
    elif name == "capability-denied":
        dimensions = [row["dimension"] for row in fixture["checks"]]
        require(dimensions == sorted(AUTHORITY_DIMENSIONS), "fixture.capability-denied authority dimensions are incomplete")
        require(all(row["allow"]["decision"] == "allowed" for row in fixture["checks"]), "fixture.capability-denied allow coverage is incomplete")
        require(all(row["deny"]["decision"] == "denied" for row in fixture["checks"]), "fixture.capability-denied deny coverage is incomplete")
        require(fixture["runtime_denial"]["runtime_supervision"]["child_reaped"], "fixture.capability-denied runtime supervision must reap the child")
        require(0 < fixture["handle_abandonment"]["grace_ms"] <= snapshot["bounds"]["max_grace_ms"], "fixture.capability-denied abandonment grace is outside the contract bound")
    elif name == "resource-limits":
        require(fixture["exceeded"]["observed_value"] > fixture["exceeded"]["effective_value"], "fixture.resource-limits exceeded evidence is not above the effective bound")
        require(fixture["exceeded"]["outcome"] in TERMINAL_OUTCOMES, "fixture.resource-limits outcome is not terminal")
    elif name == "timeout-cancellation":
        timeout = fixture["timeout"]
        cancellation = fixture["cancellation"]
        require(0 < timeout["request"]["grace_ms"] <= snapshot["bounds"]["max_grace_ms"], "fixture.timeout-cancellation timeout grace is outside the contract bound")
        require(0 < cancellation["request"]["grace_ms"] <= snapshot["bounds"]["max_grace_ms"], "fixture.timeout-cancellation cancellation grace is outside the contract bound")
        require(timeout["outcome"]["exit_reason"] != cancellation["outcome"]["exit_reason"], "fixture.timeout-cancellation must preserve distinct outcomes")


def extract_rust_function(text: str, name: str) -> str:
    start = text.find(f"fn {name}(")
    require(start >= 0, f"missing direct-native legacy helper {name}")
    opening = text.find("{", start)
    require(opening >= 0, f"direct-native legacy helper {name} has no body")
    depth = 0
    for index in range(opening, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return text[start : index + 1]
    raise ContractError(f"direct-native legacy helper {name} has an unterminated body")


def extract_generated_process_helper(text: str) -> str:
    start = text.find('out.push_str("fn axiom_process_status(program: String) -> i64 {\\n");')
    require(start >= 0, "missing generated-native process status helper")
    end = text.find('out.push_str("}\\n\\n");', start)
    require(end >= 0, "generated-native process status helper has no closing emission")
    return text[start:end]


def extract_stdlib_process_helper(text: str) -> str:
    start = text.find('        "process.ax",')
    require(start >= 0, "missing legacy process stdlib module")
    end = text.find("    ),", start)
    require(end >= 0, "legacy process stdlib module has no closing entry")
    return text[start:end]


def validate_codegen_evidence(path: Path) -> None:
    helper = extract_generated_process_helper(path.read_text(encoding="utf-8"))
    require("std::process::Command::new(program)" in helper, "generated-native legacy evidence no longer uses one executable value")
    require(".status()" in helper and ".and_then(|status| status.code())" in helper, "generated-native legacy process status evidence drifted")


def validate_direct_native_evidence(path: Path) -> None:
    helper = extract_rust_function(path.read_text(encoding="utf-8"), "emit_i64_process_status_expr")
    require(".call(runtime_refs.execv" in helper, "direct-native legacy evidence no longer calls execv")
    require(".call(runtime_refs.system" in helper, "direct-native Windows legacy evidence no longer invokes system")


def validate_stdlib_evidence(path: Path) -> None:
    helper = extract_stdlib_process_helper(path.read_text(encoding="utf-8"))
    require("pub fn run_status(command: string): int" in helper, "legacy stdlib process signature drifted")
    require("return process_status(command)" in helper, "legacy stdlib process binding drifted")


def validate_example_evidence(path: Path) -> None:
    manifest = tomllib.loads(path.read_text(encoding="utf-8"))
    capabilities = manifest.get("capabilities", {})
    require(capabilities.get("process") is True, "legacy process example no longer declares process authority")
    require(capabilities.get("env") is False, "legacy process example unexpectedly declares environment authority")
    require(capabilities.get("unsafe_rationale") == "stdlib_process example exercises unrestricted process execution intentionally", "legacy process example rationale drifted")


def validate_runtime_abi_evidence(path: Path) -> None:
    payload = load(path)
    rows = [row for row in payload.get("capability_shims", []) if row.get("id") == "process.status"]
    require(len(rows) == 1, "direct-native readiness must contain one process.status row")
    row = rows[0]
    require(row.get("status") == "implemented" and row.get("capability") == "process", "direct-native process.status readiness drifted")
    require("Arguments, broader command policy, environment control, and host-process policy coverage remain open." in row.get("notes", ""), "direct-native process.status blockers drifted")
    require(str(IMPLEMENTATION_EVIDENCE[0]) in row.get("runtime_evidence", []), "direct-native process.status row no longer cites the checked backend evidence")


def validate_implementation_evidence(root: Path, snapshot: dict[str, Any]) -> None:
    expected_implementation = {
        "tier": "static_spike",
        "structured_api": "contract_only",
        "legacy_entrypoint": "run_status(command: string): int",
        "legacy_semantics": LEGACY_SEMANTICS,
        "evidence": [path.as_posix() for path in IMPLEMENTATION_EVIDENCE],
        "blockers": BLOCKERS,
    }
    require_exact(snapshot["implementation"], expected_implementation, "implementation")
    validators: dict[Path, Callable[[Path], None]] = {
        IMPLEMENTATION_EVIDENCE[0]: validate_direct_native_evidence,
        IMPLEMENTATION_EVIDENCE[1]: validate_codegen_evidence,
        IMPLEMENTATION_EVIDENCE[2]: validate_stdlib_evidence,
        IMPLEMENTATION_EVIDENCE[3]: validate_example_evidence,
        IMPLEMENTATION_EVIDENCE[4]: validate_runtime_abi_evidence,
    }
    require(set(validators) == set(IMPLEMENTATION_EVIDENCE), "Process v1 evidence validators are not exact")
    for evidence in IMPLEMENTATION_EVIDENCE:
        path = root / evidence
        require(path.is_file(), f"missing Process v1 evidence {evidence}")
        validators[evidence](path)


def validate_lifecycle(lifecycle: dict[str, Any]) -> None:
    require_exact(lifecycle["states"], sorted(LIFECYCLE_STATES), "lifecycle.states")
    require_exact(lifecycle["signals"], sorted(PORTABLE_SIGNALS), "lifecycle.signals")
    require_exact(lifecycle["terminal_outcomes"], sorted(TERMINAL_OUTCOMES), "lifecycle.terminal_outcomes")
    transitions = lifecycle["transitions"]
    require(len(transitions) == len(set(transitions)), "lifecycle.transitions must be unique")
    for index, transition in enumerate(transitions):
        endpoints = transition.split("->")
        require(len(endpoints) == 2 and all(endpoints), f"lifecycle.transitions[{index}] must have one source and target")
        source, target = endpoints
        require(source in LIFECYCLE_STATES, f"lifecycle.transitions[{index}] has unknown source state {source}")
        require(target in LIFECYCLE_STATES, f"lifecycle.transitions[{index}] has unknown target state {target}")
        require(source not in TERMINAL_STATES, f"lifecycle.transitions[{index}] leaves terminal state {source}")
        require(target != "spawning", f"lifecycle.transitions[{index}] returns to spawning")
    require_exact(transitions, sorted(LIFECYCLE_TRANSITIONS), "lifecycle.transitions")
    for field, expected in LIFECYCLE_TEXT.items():
        require_exact(lifecycle[field], expected, f"lifecycle.{field}")


def reject_semantic_capture(snapshot: dict[str, Any]) -> None:
    semantic_surface = {
        "command": snapshot["command"],
        "stdio": snapshot["stdio"],
        "lifecycle": snapshot["lifecycle"],
        "terminal": snapshot["terminal"],
        "authority": snapshot["authority"],
        "audit": snapshot["audit"],
        "resource_limits": snapshot["resource_limits"],
        "bounds": snapshot["bounds"],
        "inspection_fields": snapshot["inspection_fields"],
        "fixtures": snapshot["fixtures"],
    }
    semantic_text = json.dumps(semantic_surface, sort_keys=True).lower()
    leaked = sorted(term for term in CAPTURE_TERMS if term in semantic_text)
    require(not leaked, f"Process v1 semantic contract leaks host capture terms: {', '.join(leaked)}")


def validate_contract(root: Path) -> dict[str, Any]:
    schema = load(root / SCHEMA)
    snapshot = load(root / SNAPSHOT)
    require(schema.get("$id", "").endswith("axiom.runtime_process.v1.schema.json"), "Process v1 schema id drift")
    validate_schema(snapshot, schema)
    require((snapshot["schema_version"], snapshot["contract"], snapshot["issue"]) == ("axiom.runtime_process.v1", "runtime.process", 1444), "Process v1 identity drift")
    reject_semantic_capture(snapshot)
    validate_implementation_evidence(root, snapshot)

    require_exact(snapshot["command"], COMMAND, "command")
    require_exact(snapshot["stdio"], STDIO, "stdio")
    validate_lifecycle(snapshot["lifecycle"])
    require_exact(snapshot["terminal"], TERMINAL, "terminal")
    require_exact(snapshot["authority"], AUTHORITY_RULES, "authority")
    require_exact(snapshot["audit"], AUDIT, "audit")
    require_exact(snapshot["resource_limits"], RESOURCE_LIMIT_CONTRACT, "resource_limits")
    for item in snapshot["resource_limits"]["supported"]:
        require(item["minimum"] <= item["default"] <= item["maximum"], f"resource_limits.supported.{item['name']} default is outside its finite bounds")
    require_exact(snapshot["bounds"], BOUNDS, "bounds")
    require_exact(snapshot["inspection_fields"], sorted(INSPECTION_FIELDS), "inspection_fields")
    require_exact(snapshot["migration"], MIGRATION, "migration")
    require(snapshot["migration"]["dependencies"] == snapshot["implementation"]["blockers"], "migration.dependencies must exactly match implementation.blockers")

    fixture_root = root / FIXTURES
    expected_files = {f"{name}.json" for name in FIXTURE_SPECS}
    actual_files = {path.name for path in fixture_root.glob("*.json")}
    require(actual_files == expected_files, "Process v1 fixture files must exactly match the pinned fixture set")
    seen: set[str] = set()
    for index, fixture_spec in enumerate(snapshot["fixtures"]):
        name = fixture_spec["id"].rsplit("/", 1)[-1]
        require(name not in seen, f"duplicate Process v1 fixture {name}")
        require(name in FIXTURE_SPECS, f"unknown Process v1 fixture metadata {name}")
        kind, asserts = FIXTURE_SPECS[name]
        expected_spec = {
            "id": f"axiom://process/fixture/{name}",
            "kind": kind,
            "path": f"{name}.json",
            "asserts": asserts,
        }
        require_exact(fixture_spec, expected_spec, f"fixtures[{index}]")
        seen.add(name)
        validate_fixture(name, load(fixture_root / fixture_spec["path"]), snapshot)
    require(seen == set(FIXTURE_SPECS), "Process v1 fixture coverage is incomplete")

    return {
        "schema": snapshot["schema_version"],
        "ok": True,
        "fixtures": len(seen),
        "authority_dimensions": len(snapshot["authority"]["dimensions"]),
        "resource_limits": len(snapshot["resource_limits"]["supported"]),
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
    except (ContractError, KeyError, OSError, TypeError, tomllib.TOMLDecodeError) as error:
        if args.json:
            print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True))
        else:
            print(f"runtime-process-v1: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "runtime-process-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
