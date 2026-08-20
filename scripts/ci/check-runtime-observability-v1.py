#!/usr/bin/env python3
"""Validate the offline runtime observability v1 contract and fixtures."""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import stat
import sys
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.runtime_observability.v1.schema.json")
EVIDENCE_SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.runtime_observability_evidence.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/runtime-observability-v1.json")
FIXTURES = Path("stage1/compiler-contracts/fixtures/observability-v1")
MAX_SOURCE_BYTES = 1024 * 1024
MAX_FIXTURE_FILES = 32
EVIDENCE_SCHEMA_VERSION = "axiom.runtime_observability.evidence.v1"
MAX_FINITE_JSON_FLOAT = Decimal("1.7976931348623157e308")


class ContractError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def relative_parts(path: Path) -> tuple[str, ...]:
    text = os.fspath(path)
    require("\x00" not in text, "source path contains NUL")
    require(not path.is_absolute(), f"source path must be relative: {text}")
    require(
        re.match(r"^(?:[A-Za-z]:[\\/]|[\\/]{2})", text) is None,
        f"source path must not be Windows-absolute: {text}",
    )
    parts = tuple(part for part in path.parts if part)
    require(bool(parts), "source path must not be empty")
    require(all(part not in {".", ".."} for part in parts), f"unsafe source path: {text}")
    return parts


def read_bytes(root: Path, path: Path) -> bytes:
    """Read one bounded regular file beneath root without following symlinks."""

    parts = relative_parts(path)
    nofollow = getattr(os, "O_NOFOLLOW", 0)
    nonblock = getattr(os, "O_NONBLOCK", 0)
    cloexec = getattr(os, "O_CLOEXEC", 0)
    directory = getattr(os, "O_DIRECTORY", 0)
    require(nofollow != 0 and directory != 0, "descriptor-safe source reads are unavailable")
    descriptors: list[int] = []
    try:
        current = os.open(os.fspath(root), os.O_RDONLY | directory | nofollow | cloexec)
        descriptors.append(current)
        for component in parts[:-1]:
            current = os.open(
                component,
                os.O_RDONLY | directory | nofollow | cloexec,
                dir_fd=current,
            )
            descriptors.append(current)
        file_descriptor = os.open(
            parts[-1],
            os.O_RDONLY | nofollow | nonblock | cloexec,
            dir_fd=current,
        )
        descriptors.append(file_descriptor)
        metadata = os.fstat(file_descriptor)
        require(stat.S_ISREG(metadata.st_mode), f"source is not a regular file: {path}")
        require(metadata.st_size <= MAX_SOURCE_BYTES, f"source exceeds 1 MiB: {path}")
        chunks: list[bytes] = []
        total = 0
        while True:
            chunk = os.read(file_descriptor, min(64 * 1024, MAX_SOURCE_BYTES + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            require(total <= MAX_SOURCE_BYTES, f"source exceeds 1 MiB: {path}")
        return b"".join(chunks)
    except ContractError:
        raise
    except OSError as error:
        raise ContractError(f"unable to read {path}: {error.strerror or error}") from error
    finally:
        for descriptor in reversed(descriptors):
            try:
                os.close(descriptor)
            except OSError:
                pass


def read_text(root: Path, path: Path) -> str:
    try:
        return read_bytes(root, path).decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise ContractError(f"source is not valid UTF-8: {path}") from error


def load(root: Path, path: Path) -> Any:
    try:
        return json.loads(
            read_text(root, path),
            parse_constant=reject_nonfinite_constant,
            parse_float=parse_exact_float,
            parse_int=parse_bounded_int,
        )
    except (json.JSONDecodeError, ValueError) as error:
        raise ContractError(f"unable to parse {path}: {error}") from error


def kind(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, (float, Decimal)):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    return "object"


def json_equal(left: Any, right: Any) -> bool:
    left_number = is_json_number(left)
    right_number = is_json_number(right)
    if left_number or right_number:
        return left_number and right_number and left == right
    if isinstance(left, list) or isinstance(right, list):
        return (
            isinstance(left, list)
            and isinstance(right, list)
            and len(left) == len(right)
            and all(json_equal(left_item, right_item) for left_item, right_item in zip(left, right))
        )
    if isinstance(left, dict) or isinstance(right, dict):
        return (
            isinstance(left, dict)
            and isinstance(right, dict)
            and set(left) == set(right)
            and all(json_equal(left[key], right[key]) for key in left)
        )
    return kind(left) == kind(right) and left == right


def reject_nonfinite_constant(value: str) -> None:
    raise ValueError(f"non-finite JSON constant {value}")


def parse_exact_float(value: str) -> Decimal:
    try:
        parsed = Decimal(value)
    except InvalidOperation as error:
        raise ValueError(f"invalid JSON number {value}") from error
    if not parsed.is_finite() or abs(parsed) > MAX_FINITE_JSON_FLOAT:
        raise ValueError(f"non-finite JSON number {value}")
    return parsed


def parse_bounded_int(value: str) -> int:
    parsed = int(value)
    if abs(Decimal(value)) > MAX_FINITE_JSON_FLOAT:
        raise ValueError(f"JSON integer exceeds the finite runtime range {value[:32]}")
    return parsed


def is_json_number(value: Any) -> bool:
    return (
        isinstance(value, (int, float, Decimal))
        and not isinstance(value, bool)
        and (not isinstance(value, float) or math.isfinite(value))
        and (not isinstance(value, Decimal) or value.is_finite())
    )


def encode_json(value: Any) -> str:
    """Serialize parsed JSON exactly, without ASCII-expanding runtime strings."""
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, Decimal):
        require(value.is_finite(), "cannot serialize a non-finite JSON number")
        return str(value)
    if isinstance(value, float):
        require(math.isfinite(value), "cannot serialize a non-finite JSON number")
        return json.dumps(value, allow_nan=False)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, list):
        return "[" + ",".join(encode_json(item) for item in value) + "]"
    if isinstance(value, dict):
        require(all(isinstance(key, str) for key in value), "JSON object keys must be strings")
        return "{" + ",".join(
            f"{json.dumps(key, ensure_ascii=False)}:{encode_json(value[key])}"
            for key in sorted(value)
        ) + "}"
    raise ContractError(f"unsupported JSON value {type(value).__name__}")


def matches_json_type(value: Any, expected: str) -> bool:
    if expected == "number":
        return is_json_number(value)
    if expected == "integer":
        return (
            is_json_number(value)
            and (
                isinstance(value, int)
                or (isinstance(value, float) and value.is_integer())
                or (isinstance(value, Decimal) and value == value.to_integral_value())
            )
        )
    return kind(value) == expected


def resolve_ref(root_schema: dict[str, Any], reference: str) -> dict[str, Any]:
    require(reference.startswith("#/"), f"unsupported schema reference {reference}")
    value: Any = root_schema
    for component in reference[2:].split("/"):
        component = component.replace("~1", "/").replace("~0", "~")
        require(isinstance(value, dict) and component in value, f"unknown schema reference {reference}")
        value = value[component]
    require(isinstance(value, dict), f"schema reference is not an object: {reference}")
    return value


def schema_matches(value: Any, schema: dict[str, Any], path: str, root_schema: dict[str, Any]) -> bool:
    try:
        validate_schema(value, schema, path, root_schema)
    except ContractError:
        return False
    return True


def validate_schema(
    value: Any,
    schema: dict[str, Any],
    path: str,
    root_schema: dict[str, Any] | None = None,
) -> None:
    root_schema = schema if root_schema is None else root_schema
    if "$ref" in schema:
        validate_schema(value, resolve_ref(root_schema, schema["$ref"]), path, root_schema)
        return
    if "oneOf" in schema:
        matches = sum(
            schema_matches(value, candidate, path, root_schema)
            for candidate in schema["oneOf"]
        )
        require(matches == 1, f"{path}: expected exactly one schema match")
    for candidate in schema.get("allOf", []):
        validate_schema(value, candidate, path, root_schema)
    if "not" in schema:
        require(
            not schema_matches(value, schema["not"], path, root_schema),
            f"{path}: forbidden schema matched",
        )
    if "if" in schema and schema_matches(value, schema["if"], path, root_schema):
        if "then" in schema:
            validate_schema(value, schema["then"], path, root_schema)
    if "const" in schema:
        require(json_equal(value, schema["const"]), f"{path}: const mismatch")
    if "enum" in schema:
        require(any(json_equal(value, candidate) for candidate in schema["enum"]), f"{path}: enum mismatch")
    expected = schema.get("type")
    if expected:
        expected_types = [expected] if isinstance(expected, str) else expected
        require(any(matches_json_type(value, item) for item in expected_types), f"{path}: expected {expected}")
    if isinstance(value, dict):
        properties = schema.get("properties", {})
        for field in schema.get("required", []):
            require(field in value, f"{path}: missing {field}")
        if schema.get("additionalProperties") is False:
            require(not (set(value) - set(properties)), f"{path}: unknown fields")
        require(len(value) <= schema.get("maxProperties", len(value)), f"{path}: too many properties")
        if "propertyNames" in schema:
            for field in value:
                validate_schema(field, schema["propertyNames"], f"{path}.<propertyName>", root_schema)
        for field, nested in value.items():
            if field in properties:
                validate_schema(nested, properties[field], f"{path}.{field}", root_schema)
            elif isinstance(schema.get("additionalProperties"), dict):
                validate_schema(
                    nested,
                    schema["additionalProperties"],
                    f"{path}.{field}",
                    root_schema,
                )
    if isinstance(value, list):
        require(len(value) >= schema.get("minItems", 0), f"{path}: too few items")
        require(len(value) <= schema.get("maxItems", len(value)), f"{path}: too many items")
        if schema.get("uniqueItems"):
            require(
                all(
                    not any(json_equal(item, previous) for previous in value[:index])
                    for index, item in enumerate(value)
                ),
                f"{path}: duplicate items",
            )
        if "items" in schema:
            for index, item in enumerate(value):
                validate_schema(item, schema["items"], f"{path}[{index}]", root_schema)
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: empty string")
        require(len(value) <= schema.get("maxLength", len(value)), f"{path}: string too long")
        if schema.get("pattern"):
            require(re.search(schema["pattern"], value) is not None, f"{path}: pattern mismatch")
    if is_json_number(value):
        require(value >= schema.get("minimum", value), f"{path}: below minimum")
        require(value <= schema.get("maximum", value), f"{path}: above maximum")


def validate_runtime_proof(
    fixture: dict[str, Any],
    snapshot: dict[str, Any],
    evidence_schema: dict[str, Any],
) -> None:
    require(
        set(fixture) == {"schema_version", "delivery", "event", "shutdown_report", "assertions"},
        "runtime proof fields drifted",
    )
    require(
        fixture["schema_version"] == "axiom.runtime_observability.runtime_proof.v1"
        and fixture["delivery"] == "executed_rust_runtime",
        "runtime proof is not executable evidence",
    )
    event = fixture["event"]
    validate_schema(event, evidence_schema, "$.event", evidence_schema)
    require(
        event.get("schema_version") == EVIDENCE_SCHEMA_VERSION and event.get("kind") == "event",
        "runtime event envelope drifted",
    )
    require(event.get("level") in snapshot["event"]["levels"], "runtime event level invalid")
    require(
        isinstance(event.get("correlation"), dict)
        and set(event["correlation"]) == {"request_id", "runtime_origin", "span_id", "trace_id"},
        "runtime event correlation proof is incomplete",
    )
    require(len(event.get("fields", {})) <= snapshot["event"]["max_fields"], "runtime event fields unbounded")
    require(
        len(encode_json(event).encode("utf-8"))
        <= snapshot["event"]["max_event_bytes"],
        "runtime event bytes unbounded",
    )
    password = event.get("fields", {}).get("password", {})
    error_message = event.get("error", {}).get("message", {})
    for label, field, reason in (
        ("secret key", password, "sensitive_key"),
        ("error message", error_message, "error_message"),
    ):
        require(
            field.get("type") == "redacted"
            and field.get("value") == snapshot["redaction"]["replacement"]
            and field.get("redaction", {}).get("reason") == reason,
            f"runtime {label} was not redacted",
        )
    shutdown = fixture["shutdown_report"]
    validate_schema(shutdown, evidence_schema, "$.shutdown_report", evidence_schema)
    require(
        shutdown.get("schema_version") == EVIDENCE_SCHEMA_VERSION
        and shutdown.get("kind") == "shutdown_report",
        "shutdown report envelope drifted",
    )
    require(
        shutdown.get("state") == "flushed"
        and shutdown.get("queue_remaining") == 0
        and shutdown.get("queued_bytes") == 0
        and shutdown.get("flush_attempted") is True
        and shutdown.get("deadline_exceeded") is False
        and shutdown.get("sink_failure") is None,
        "runtime proof does not establish successful drain-before-flush",
    )
    counters = shutdown.get("counters", {})
    require(
        counters.get("attempted") == counters.get("accepted") == counters.get("written") == 1
        and counters.get("dropped") == counters.get("filtered") == counters.get("rejected") == 0
        and counters.get("sink_failures") == 0,
        "runtime proof counters drifted",
    )
    serialized = encode_json(fixture)
    require("golden-must-not-contain-this" not in serialized, "runtime proof leaked marker secret")


def validate_fixture(
    name: str,
    fixture: dict[str, Any],
    snapshot: dict[str, Any],
    evidence_schema: dict[str, Any],
) -> None:
    if name == "golden-event":
        correlation = fixture.get("correlation", {})
        payload = fixture.get("sink_payload", {})
        require(fixture.get("level") in snapshot["event"]["levels"], "golden event level invalid")
        require(correlation.get("runtime_origin") == "native", "golden event origin is not runtime evidence")
        require("password" not in payload and "token" not in payload, "golden event leaked a secret")
        return
    if name == "redaction-negative":
        secret_keys = set(snapshot["redaction"]["secret_keys"])
        payload = fixture.get("sink_payload", {})
        redacted = secret_keys & set(payload)
        require(bool(redacted), "redaction-negative fixture is valid")
        require(
            all(payload[key] == snapshot["redaction"]["replacement"] for key in redacted),
            "redaction-negative fixture contains an unredacted secret value",
        )
        return
    if name == "unbounded-cardinality":
        require(fixture.get("label_values", 0) > snapshot["event"]["max_label_values"], "unbounded-cardinality fixture is valid")
        return
    if name == "sink-shutdown":
        require(fixture.get("sink_state") == "failed" and fixture.get("shutdown_state") == "flushed", "sink-shutdown fixture is valid")
        return
    if name == "correlation":
        correlation = fixture.get("correlation", {})
        require(
            fixture.get("source") != snapshot["correlation"]["source"]
            or not correlation.get("runtime_origin")
            or not correlation.get("trace_id"),
            "correlation fixture is valid",
        )
        return
    if name == "runtime-core-golden":
        validate_runtime_proof(fixture, snapshot, evidence_schema)
        return
    raise ContractError(f"unknown fixture {name}")


def load_external(path: Path) -> Any:
    try:
        metadata = path.lstat()
        require(stat.S_ISREG(metadata.st_mode), "runtime evidence must be a regular file")
        require(metadata.st_size <= MAX_SOURCE_BYTES, "runtime evidence exceeds 1 MiB")
        return json.loads(
            path.read_text(encoding="utf-8"),
            parse_constant=reject_nonfinite_constant,
            parse_float=parse_exact_float,
            parse_int=parse_bounded_int,
        )
    except (OSError, UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
        raise ContractError(f"unable to load runtime evidence: {error}") from error


def validate_contract(root: Path, runtime_evidence: Path | None = None) -> dict[str, Any]:
    schema = load(root, SCHEMA)
    evidence_schema = load(root, EVIDENCE_SCHEMA)
    snapshot = load(root, SNAPSHOT)
    require(schema.get("$id", "").endswith("axiom.runtime_observability.v1.schema.json"), "schema id drift")
    require(
        evidence_schema.get("$id", "").endswith("axiom.runtime_observability_evidence.v1.schema.json")
        and evidence_schema.get("properties", {}).get("schema_version", {}).get("const")
        == EVIDENCE_SCHEMA_VERSION,
        "runtime evidence schema identity drift",
    )
    require(
        evidence_schema.get("properties", {}).get("kind", {}).get("enum")
        == ["drain_report", "emit_receipt", "event", "inspection", "shutdown_report"],
        "runtime evidence kinds drifted",
    )
    validate_schema(snapshot, schema, "$")
    require(snapshot["schema_version"] == "axiom.runtime_observability.v1", "schema version drift")
    require(snapshot["contract"] == "runtime.observability" and snapshot["issue"] == 1451, "contract identity drift")
    event = snapshot["event"]
    require(event["levels"] == sorted(set(event["levels"])), "levels must be sorted and unique")
    require(event["labels"] == sorted(set(event["labels"])), "labels must be sorted and unique")
    require(event["max_event_bytes"] <= 65536 and event["max_fields"] <= 32, "event bounds are too large")
    require(event["max_label_values"] <= 1000, "label cardinality is unbounded")
    redaction = snapshot["redaction"]
    require(redaction["before_sink"] is True and redaction["replacement"] == "[REDACTED]", "redaction is not sink-safe")
    require(redaction["secret_keys"] == sorted(set(redaction["secret_keys"])), "secret keys must be sorted and unique")
    require(set(snapshot["correlation"]["fields"]) >= {"request_id", "trace_id", "span_id", "runtime_origin"}, "correlation fields incomplete")
    require(snapshot["sinks"]["queue_capacity"] <= 4096, "sink queue is unbounded")
    require(snapshot["shutdown"]["states"] == ["running", "draining", "flushed", "failed"], "shutdown states drift")
    require(len(snapshot["fixtures"]) <= MAX_FIXTURE_FILES, "fixture list exceeds safe copy bound")
    seen: set[str] = set()
    for spec in snapshot["fixtures"]:
        fixture_id = spec["id"]
        name = fixture_id.rsplit("/", 1)[-1]
        require(name not in seen, f"duplicate fixture {name}")
        seen.add(name)
        validate_fixture(
            name,
            load(root, FIXTURES / spec["path"]),
            snapshot,
            evidence_schema,
        )
    require("runtime-core-golden" in seen, "executable runtime proof is missing")
    if runtime_evidence is not None:
        validate_runtime_proof(load_external(runtime_evidence), snapshot, evidence_schema)
    return {
        "schema": snapshot["schema_version"],
        "ok": True,
        "fixtures": len(seen),
        "runtime_evidence": (
            "executed_rust_runtime"
            if runtime_evidence is not None
            else "checked_in_runtime_fixture"
        ),
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--runtime-evidence", type=Path)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root, args.runtime_evidence)
    except (ContractError, KeyError, TypeError) as error:
        if args.json:
            print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True))
        else:
            print(f"runtime-observability-v1: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "runtime-observability-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
