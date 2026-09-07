#!/usr/bin/env python3
"""Validate the offline runtime observability v1 contract and fixtures."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.runtime_observability.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/runtime-observability-v1.json")
FIXTURES = Path("stage1/compiler-contracts/fixtures/observability-v1")


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


def kind(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    return "object"


def validate_schema(value: Any, schema: dict[str, Any], path: str) -> None:
    if "const" in schema:
        require(value == schema["const"], f"{path}: const mismatch")
    if "enum" in schema:
        require(value in schema["enum"], f"{path}: enum mismatch")
    expected = schema.get("type")
    if expected:
        require(kind(value) == expected, f"{path}: expected {expected}")
    if isinstance(value, dict):
        properties = schema.get("properties", {})
        for field in schema.get("required", []):
            require(field in value, f"{path}: missing {field}")
        if schema.get("additionalProperties") is False:
            require(not (set(value) - set(properties)), f"{path}: unknown fields")
        for field, nested in value.items():
            if field in properties:
                validate_schema(nested, properties[field], f"{path}.{field}")
    if isinstance(value, list):
        require(len(value) >= schema.get("minItems", 0), f"{path}: too few items")
        if "items" in schema:
            for index, item in enumerate(value):
                validate_schema(item, schema["items"], f"{path}[{index}]")
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: empty string")
        if schema.get("pattern"):
            require(re.search(schema["pattern"], value) is not None, f"{path}: pattern mismatch")
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        require(value >= schema.get("minimum", value), f"{path}: below minimum")


def validate_fixture(name: str, fixture: dict[str, Any], snapshot: dict[str, Any]) -> None:
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
    raise ContractError(f"unknown fixture {name}")


def validate_contract(root: Path) -> dict[str, Any]:
    schema = load(root / SCHEMA)
    snapshot = load(root / SNAPSHOT)
    require(schema.get("$id", "").endswith("axiom.runtime_observability.v1.schema.json"), "schema id drift")
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
    fixture_root = root / FIXTURES
    seen: set[str] = set()
    for spec in snapshot["fixtures"]:
        fixture_id = spec["id"]
        name = fixture_id.rsplit("/", 1)[-1]
        require(name not in seen, f"duplicate fixture {name}")
        seen.add(name)
        validate_fixture(name, load(fixture_root / spec["path"]), snapshot)
    return {"schema": snapshot["schema_version"], "ok": True, "fixtures": len(seen)}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
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
