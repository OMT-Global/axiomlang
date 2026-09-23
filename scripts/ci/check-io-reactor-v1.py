#!/usr/bin/env python3
"""Validate the I/O Reactor v1 evidence contract and current fail-closed boundary."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.io_reactor.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/io-reactor-v1.json")
FIXTURES = Path("stage1/compiler-contracts/fixtures/io-reactor-v1")
REACTOR_DOC = Path("docs/io-reactor-v1.md")
TCP_DOC = Path("docs/stage1-net-tcp.md")
READINESS = Path("docs/production-language-readiness.json")
CAPABILITY_LEDGER = Path("stage1/compiler-contracts/snapshots/capability-ledger.json")
CODEGEN_SOURCE = Path("stage1/crates/axiomc/src/codegen.rs")
STDLIB_SOURCE = Path("stage1/crates/axiomc/src/stdlib.rs")

BLOCKERS = {1425, 1426, 1436, 1438, 1445}
DEPENDENCIES = BLOCKERS
RESOURCES = {"cancellation_wakeup", "signal", "tcp_listener", "tcp_stream", "timer", "udp_socket"}
INTERESTS = {"error", "hangup", "readable", "writable"}
OUTCOMES = {"canceled", "closed", "deadline_expired", "failed", "partial", "ready"}
OPERATIONS = {"accept", "cancel", "close", "connect", "poll", "read", "recv_from", "register", "send_to", "signal_wait", "timer_wait", "write"}
FEATURES = {"backpressure", "bounded_buffers", "cancellation", "deadlines", "deterministic_close", "partial_io"}
PRODUCER_ACTIONS = {"await_capacity", "cancel", "deadline_expired"}
INSPECTION_FIELDS = {"adapter", "buffer_bound", "cancellation_owner", "deadline", "operation_generation", "readiness", "resource_id", "resource_kind", "target"}
TARGET_ADAPTERS = {
    "aarch64-apple-darwin": "kqueue",
    "x86_64-unknown-linux-gnu": "epoll",
}
FIXTURE_NAMES = {
    "adapter-leak-rejected",
    "cancellation-race",
    "current-blocking-runtime",
    "partial-io",
    "thread-per-connection-rejected",
    "unbounded-buffer-rejected",
}


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
    if value is None:
        return "null"
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
    if isinstance(left, list) or isinstance(right, list):
        return (
            isinstance(left, list)
            and isinstance(right, list)
            and len(left) == len(right)
            and all(json_equal(a, b) for a, b in zip(left, right))
        )
    if isinstance(left, dict) or isinstance(right, dict):
        return (
            isinstance(left, dict)
            and isinstance(right, dict)
            and set(left) == set(right)
            and all(json_equal(left[key], right[key]) for key in left)
        )
    return type(left) is type(right) and left == right


def validate_schema(value: Any, schema: dict[str, Any], path: str = "$") -> None:
    for index, nested in enumerate(schema.get("allOf", [])):
        validate_schema(value, nested, f"{path}.allOf[{index}]")
    if "if" in schema:
        try:
            validate_schema(value, schema["if"], path)
        except ContractError:
            branch = schema.get("else")
        else:
            branch = schema.get("then")
        if branch is not None:
            validate_schema(value, branch, path)
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
        if "maxItems" in schema:
            require(len(value) <= schema["maxItems"], f"{path}: too many items")
        if "items" in schema:
            for index, item in enumerate(value):
                validate_schema(item, schema["items"], f"{path}[{index}]")
    if isinstance(value, int) and not isinstance(value, bool) and "minimum" in schema:
        require(value >= schema["minimum"], f"{path}: below minimum")
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: empty string")
        if schema.get("pattern"):
            require(re.search(schema["pattern"], value) is not None, f"{path}: pattern mismatch")


def sorted_unique(values: list[str], expected: set[str], field: str) -> None:
    require(values == sorted(expected), f"{field} must be complete, sorted, and unique")


def is_positive_integer(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def validate_inspection(inspection: Any, operation_generation: int, buffer_bound: int, label: str) -> None:
    require(isinstance(inspection, dict), f"{label} inspection must be an object")
    require(set(inspection) == INSPECTION_FIELDS, f"{label} inspection fields are incomplete")
    require(is_positive_integer(buffer_bound), f"{label} buffer bound must be a positive integer")
    require(
        is_positive_integer(inspection["operation_generation"]),
        f"{label} inspection generation must be a positive integer",
    )
    require(
        is_positive_integer(inspection["buffer_bound"]),
        f"{label} inspection buffer bound must be a positive integer",
    )
    require(
        json_equal(inspection["operation_generation"], operation_generation),
        f"{label} inspection generation drifted",
    )
    require(
        json_equal(inspection["buffer_bound"], buffer_bound),
        f"{label} inspection buffer bound drifted",
    )
    require(inspection["resource_kind"] in RESOURCES, f"{label} inspection resource kind is invalid")
    require(inspection["readiness"] in INTERESTS, f"{label} inspection readiness is invalid")
    require(inspection["target"] in TARGET_ADAPTERS, f"{label} inspection target is invalid")
    require(inspection["adapter"] == TARGET_ADAPTERS[inspection["target"]], f"{label} inspection adapter drifted")
    require(
        all(isinstance(inspection[field], str) and bool(inspection[field].strip()) for field in ("cancellation_owner", "deadline", "resource_id")),
        f"{label} inspection identities must be non-empty",
    )


def validate_fixture(name: str, fixture: dict[str, Any]) -> None:
    if name == "current-blocking-runtime":
        require(
            set(fixture)
            == {
                "schema_version",
                "claim",
                "runtime_backed",
                "tcp_listener_blocking",
                "async_wrapper_transport",
                "adapter",
                "adapter_unavailable_reason",
                "decision",
                "blocker_issues",
            },
            "current runtime fixture fields drifted",
        )
        require(fixture.get("claim") == "runtime_io_reactor", "current runtime claim drifted")
        require(fixture.get("runtime_backed") is False, "blocking runtime was promoted to reactor-backed")
        require(fixture.get("tcp_listener_blocking") is True, "current TCP listener no longer reports blocking behavior")
        require(fixture.get("async_wrapper_transport") == "blocking_call_in_task", "async wrapper boundary drifted")
        require(fixture.get("adapter") is None, "current runtime fixture invents a reactor adapter")
        require(
            isinstance(fixture.get("adapter_unavailable_reason"), str)
            and bool(fixture["adapter_unavailable_reason"].strip()),
            "current runtime fixture omits the unavailable adapter reason",
        )
        require(fixture.get("decision") == "partial", "current blocking runtime was qualified")
        require(set(fixture.get("blocker_issues", [])) == BLOCKERS, "current runtime blockers drifted")
        return
    if name == "partial-io":
        requested = fixture.get("requested_bytes")
        completed = fixture.get("completed_bytes")
        remaining = fixture.get("remaining_bytes")
        require(all(isinstance(value, int) and not isinstance(value, bool) and value >= 0 for value in (requested, completed, remaining)), "partial I/O byte counts are invalid")
        require(completed < requested and completed + remaining == requested, "partial I/O accounting is inconsistent")
        buffer_bound = fixture.get("buffer_bound")
        require(is_positive_integer(buffer_bound), "partial I/O buffer bound must be a positive integer")
        require(json_equal(buffer_bound, requested), "partial I/O exceeds or omits its buffer bound")
        require(fixture.get("outcome") == "partial" and fixture.get("decision") == "accepted", "partial I/O outcome drifted")
        require(
            is_positive_integer(fixture.get("operation_generation")),
            "partial I/O lacks a positive operation generation",
        )
        validate_inspection(fixture.get("inspection"), fixture["operation_generation"], buffer_bound, "partial I/O")
        return
    if name == "cancellation-race":
        operation_generation = fixture.get("operation_generation")
        require(
            is_positive_integer(operation_generation),
            "cancellation race lacks a positive operation generation",
        )
        events = fixture.get("events", [])
        sequences = [event.get("sequence") for event in events]
        names = [event.get("event") for event in events]
        require(sequences == sorted(sequences) and len(sequences) == len(set(sequences)), "cancellation race is not monotonically ordered")
        require(names == ["registered", "cancellation_requested", "terminal_canceled", "late_readiness_ignored"], "cancellation race transcript drifted")
        require(
            all(json_equal(event.get("operation_generation"), operation_generation) for event in events),
            "cancellation race events are not bound to the operation generation",
        )
        require(fixture.get("terminal_outcome") == "canceled", "cancellation race has the wrong terminal outcome")
        require(fixture.get("late_readiness_delivered") is False, "late readiness revived a canceled operation")
        require(fixture.get("decision") == "accepted", "valid cancellation race was rejected")
        validate_inspection(fixture.get("inspection"), operation_generation, fixture.get("buffer_bound"), "cancellation race")
        return
    if name == "unbounded-buffer-rejected":
        require(fixture.get("buffer_bound") is None, "unbounded-buffer fixture has a bound")
        require(fixture.get("decision") == "rejected", "unbounded operation was accepted")
        require(fixture.get("diagnostic") == "io_reactor.buffer_bound_required", "unbounded-buffer diagnostic drifted")
        return
    if name == "thread-per-connection-rejected":
        require(fixture.get("execution_model") == "thread_per_connection", "thread execution fixture drifted")
        require(fixture.get("decision") == "rejected", "thread-per-connection execution was accepted")
        require(fixture.get("diagnostic") == "io_reactor.thread_per_connection", "thread execution diagnostic drifted")
        return
    if name == "adapter-leak-rejected":
        public_fields = set(fixture.get("public_fields", []))
        require("epoll_fd" in public_fields, "adapter leak fixture does not expose a target handle")
        require(fixture.get("decision") == "rejected", "adapter-specific public semantics were accepted")
        require(fixture.get("diagnostic") == "io_reactor.adapter_detail_exposed", "adapter leak diagnostic drifted")
        return
    raise ContractError(f"unknown I/O Reactor v1 fixture {name}")


def validate_current_boundary(root: Path) -> None:
    reactor_doc = (root / REACTOR_DOC).read_text(encoding="utf-8").lower()
    tcp_doc = (root / TCP_DOC).read_text(encoding="utf-8").lower()
    codegen = (root / CODEGEN_SOURCE).read_text(encoding="utf-8")
    stdlib = (root / STDLIB_SOURCE).read_text(encoding="utf-8")
    readiness = load(root / READINESS)
    capability_ledger = load(root / CAPABILITY_LEDGER)
    require("does not claim" in reactor_doc and "blocking" in reactor_doc, "reactor documentation no longer states the implementation boundary")
    require("blocking tcp listener" in tcp_doc and "blocking" in tcp_doc, "TCP documentation no longer identifies blocking transport")
    require("listener.set_nonblocking(false).ok()?;" in codegen, "current generated TCP listener boundary drifted")
    require("pub async fn accept(listener: TcpListener)" in stdlib, "async TCP wrapper is missing")
    require("return net_tcp_accept(listener)" in stdlib, "async TCP wrapper no longer delegates to the current transport")
    readiness_row = next(
        (row for row in readiness.get("rows", []) if row.get("id") == "io_reactor"),
        None,
    )
    require(readiness_row is not None, "production readiness is missing the I/O Reactor row")
    require(
        readiness_row.get("currentTier") == "syntax_only"
        and readiness_row.get("status") == "blocked",
        "production readiness promoted I/O Reactor without executable evidence",
    )
    reactor_schema_id = "https://axiom-lang.org/schemas/axiom.io_reactor.v1.schema.json"
    ledger_row = next(
        (row for row in capability_ledger.get("schemas", []) if row.get("name") == reactor_schema_id),
        None,
    )
    require(ledger_row is not None, "capability ledger is missing the I/O Reactor schema")
    require(
        ledger_row.get("evidenceTier") == "static_spike",
        "capability ledger did not catalog the checked I/O Reactor schema",
    )


def validate_contract(root: Path) -> dict[str, Any]:
    schema = load(root / SCHEMA)
    snapshot = load(root / SNAPSHOT)
    require(schema.get("$id", "").endswith("axiom.io_reactor.v1.schema.json"), "I/O Reactor v1 schema id drifted")
    validate_schema(snapshot, schema)
    require((snapshot["schema_version"], snapshot["contract"], snapshot["issue"]) == ("axiom.io_reactor.v1", "runtime.io_reactor", 1446), "I/O Reactor v1 identity drifted")

    implementation = snapshot["implementation"]
    require(set(implementation["blockers"]) == BLOCKERS, "I/O Reactor blocker boundary drifted")
    require(implementation["tier"] == "syntax_only", "current I/O Reactor snapshot was promoted without executable evidence")
    unavailable_claims = ("runtime_backed", "nonblocking_io", "portable_adapters", "thread_per_connection_free")
    require(not any(implementation[field] for field in unavailable_claims), "static spike claims unavailable runtime proof")

    readiness = snapshot["readiness_model"]
    sorted_unique(readiness["resources"], RESOURCES, "reactor resources")
    sorted_unique(readiness["interests"], INTERESTS, "readiness interests")
    sorted_unique(readiness["outcomes"], OUTCOMES, "operation outcomes")
    require(readiness["registration_generation_required"] is True, "readiness registrations are not generation-bound")

    operations = snapshot["operations"]
    sorted_unique(operations["required_operations"], OPERATIONS, "reactor operations")
    sorted_unique(operations["required_features"], FEATURES, "operation features")
    require(operations["partial_completion_reports_bytes"] is True, "partial I/O loses byte counts")
    require(operations["zero_progress_requires_readiness_change"] is True, "zero-progress operations may spin")
    require(operations["deadline_clock"] == "monotonic", "deadlines are not monotonic")
    require(operations["close_idempotent"] is True, "resource close is not idempotent")
    require(operations["readiness_after_close"] == "ignored", "late readiness may revive a closed resource")

    backpressure = snapshot["backpressure"]
    require(backpressure["queue_capacity_required"] is True and backpressure["buffer_bound_required"] is True, "reactor bounds are optional")
    require(backpressure["fairness_policy_required"] is True, "backpressure has no fairness contract")
    sorted_unique(backpressure["producer_actions"], PRODUCER_ACTIONS, "backpressure producer actions")

    cancellation = snapshot["cancellation"]
    require(cancellation == {
        "owner_required": True,
        "terminal_outcome": "canceled",
        "readiness_after_terminal": "ignored",
        "race_order": "monotonic_operation_generation",
    }, "cancellation race contract drifted")

    adapters = snapshot["adapters"]
    require(adapters["implementation_detail_only"] is True and adapters["adapter_names_not_language_semantics"] is True, "target adapters leaked into language semantics")
    require(adapters["no_thread_per_connection"] is True, "thread-per-connection execution was allowed")
    observed_adapters = {row["target"]: row["adapter"] for row in adapters["targets"]}
    require(observed_adapters == TARGET_ADAPTERS and len(adapters["targets"]) == len(TARGET_ADAPTERS), "supported target adapter matrix drifted")
    require(
        all(
            row["available"] is False
            and isinstance(row.get("reason"), str)
            and bool(row["reason"].strip())
            for row in adapters["targets"]
        ),
        "current target adapter evidence must remain unavailable with a reason",
    )

    sorted_unique(snapshot["inspection"]["fields"], INSPECTION_FIELDS, "inspection fields")
    require(snapshot["inspection"]["unavailable_value"] == "null_with_reason", "unavailable evidence is not explicit")
    require(set(snapshot["migration"]["dependencies"]) == DEPENDENCIES, "I/O Reactor dependencies drifted")

    seen: set[str] = set()
    for fixture_spec in snapshot["fixtures"]:
        name = fixture_spec["id"].rsplit("/", 1)[-1]
        require(name not in seen, f"duplicate I/O Reactor fixture {name}")
        require(fixture_spec["path"] == f"{name}.json", f"I/O Reactor fixture path drifted for {name}")
        seen.add(name)
        validate_fixture(name, load(root / FIXTURES / fixture_spec["path"]))
    require(seen == FIXTURE_NAMES, "I/O Reactor fixture coverage is incomplete")
    validate_current_boundary(root)
    return {
        "schema": snapshot["schema_version"],
        "ok": True,
        "fixtures": len(seen),
        "operations": len(operations["required_operations"]),
        "resources": len(readiness["resources"]),
        "targets": len(adapters["targets"]),
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
    except (ContractError, KeyError, OSError, TypeError) as error:
        if args.json:
            print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True))
        else:
            print(f"io-reactor-v1: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "io-reactor-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
