#!/usr/bin/env python3
"""Validate the non-transport HTTP client v1 contract and negative fixtures."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.runtime_http_client.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/http-client-v1.json")
FIXTURES = Path("stage1/compiler-contracts/fixtures/http-client-v1")


class ContractError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def json_type(value: Any) -> str:
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


def validate_schema_node(value: Any, schema: dict[str, Any], path: str, defs: dict[str, Any]) -> None:
    reference = schema.get("$ref")
    if reference is not None:
        prefix = "#/$defs/"
        require(isinstance(reference, str) and reference.startswith(prefix), f"{path}: unsupported schema reference")
        name = reference.removeprefix(prefix)
        require(name in defs, f"{path}: missing schema definition {name}")
        validate_schema_node(value, defs[name], path, defs)
        return
    if "const" in schema:
        require(value == schema["const"], f"{path}: expected {schema['const']!r}")
    if "enum" in schema:
        require(value in schema["enum"], f"{path}: value is outside enum")
    expected = schema.get("type")
    if expected is not None:
        actual = json_type(value)
        require(actual == expected, f"{path}: expected {expected}, got {actual}")
    if isinstance(value, dict):
        required = schema.get("required", [])
        for key in required:
            require(key in value, f"{path}: missing required field {key}")
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            unknown = set(value) - set(properties)
            require(not unknown, f"{path}: unknown fields {sorted(unknown)}")
        for key, nested in value.items():
            if key in properties:
                validate_schema_node(nested, properties[key], f"{path}.{key}", defs)
    if isinstance(value, list):
        require(len(value) >= schema.get("minItems", 0), f"{path}: too few items")
        item_schema = schema.get("items")
        if item_schema:
            for index, item in enumerate(value):
                validate_schema_node(item, item_schema, f"{path}[{index}]", defs)
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: string is empty")
        pattern = schema.get("pattern")
        if pattern:
            require(re.search(pattern, value) is not None, f"{path}: string does not match pattern")
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if "minimum" in schema:
            require(value >= schema["minimum"], f"{path}: value is below minimum")
        if "maximum" in schema:
            require(value <= schema["maximum"], f"{path}: value is above maximum")


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"unable to load {path}: {error}") from error


def duplicate_content_lengths(headers: list[dict[str, Any]]) -> bool:
    values = [item["value"] for item in headers if item.get("name", "").casefold() == "content-length"]
    return len(set(values)) > 1


def validate_negative_fixture(kind: str, fixture: dict[str, Any], snapshot: dict[str, Any]) -> None:
    if kind == "malformed-status":
        status = fixture.get("status")
        require(not isinstance(status, int) or not 100 <= status <= 599, "malformed-status fixture is valid")
    elif kind == "conflicting-lengths":
        headers = fixture.get("headers")
        require(isinstance(headers, list) and duplicate_content_lengths(headers), "conflicting-lengths fixture is valid")
    elif kind == "oversize-body":
        body_bytes = fixture.get("body_bytes")
        maximum = snapshot["response"]["body"]["max_bytes"]
        require(isinstance(body_bytes, int) and body_bytes > maximum, "oversize-body fixture is valid")
    elif kind == "unsupported-policy":
        require(
            fixture.get("redirects") not in {"deny", "same_origin", "allowlist"}
            or fixture.get("tls") not in {"system_roots", "pinned"},
            "unsupported-policy fixture is valid",
        )
    elif kind == "cancellation-error":
        require(
            not (
                fixture.get("error") == "cancelled"
                and fixture.get("cancellation_outcome") in {"acknowledged", "too_late"}
                and fixture.get("body_delivered") is False
            ),
            "cancellation-error fixture is valid",
        )
    else:
        raise ContractError(f"unknown negative fixture kind {kind}")


def validate_contract(root: Path) -> dict[str, Any]:
    schema = load_json(root / SCHEMA)
    snapshot = load_json(root / SNAPSHOT)
    require(isinstance(schema, dict), "schema must be an object")
    require(isinstance(snapshot, dict), "snapshot must be an object")
    validate_schema_node(snapshot, schema, "$", schema.get("$defs", {}))
    require(snapshot["schema_version"] == "axiom.runtime_http_client.v1", "schema version drift")
    require(snapshot["contract"] == "runtime.http_client" and snapshot["issue"] == 1448, "contract identity drift")
    methods = snapshot["request"]["methods"]
    require(methods == sorted(set(methods)), "request methods must be sorted and unique")
    require({"GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"} <= set(methods), "request method surface is incomplete")
    status = snapshot["response"]["status"]
    require(status["minimum"] == 100 and status["maximum"] == 599, "response status range drift")
    require(snapshot["response"]["body"]["max_bytes"] <= 8 * 1024 * 1024, "response limit is unbounded")
    require(snapshot["request"]["limits"]["max_bytes"] <= 1 * 1024 * 1024, "request limit is unbounded")
    require(snapshot["policies"]["redirects"] == "deny", "redirects must default to deny")
    require(snapshot["policies"]["tls"] in {"system_roots", "pinned"}, "TLS policy is not verified")
    require("cancelled" in snapshot["errors"] and "response_too_large" in snapshot["errors"], "required errors missing")
    require(snapshot["cancellation"]["outcomes"] == sorted(snapshot["cancellation"]["outcomes"]), "cancellation outcomes must be deterministic")
    fixture_root = root / FIXTURES
    seen: set[str] = set()
    for fixture_spec in snapshot["fixtures"]:
        fixture_id = fixture_spec["id"]
        require(fixture_id not in seen, f"duplicate fixture {fixture_id}")
        seen.add(fixture_id)
        fixture_path = fixture_root / fixture_spec["path"]
        fixture = load_json(fixture_path)
        validate_negative_fixture(fixture_id.rsplit("/", 1)[-1], fixture, snapshot)
    return {"schema": snapshot["schema_version"], "ok": True, "fixtures": len(seen)}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
    except (ContractError, OSError, TypeError, KeyError) as error:
        if args.json:
            print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True))
        else:
            print(f"http-client-v1: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "http-client-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
