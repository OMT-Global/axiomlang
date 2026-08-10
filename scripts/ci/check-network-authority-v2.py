#!/usr/bin/env python3
"""Validate the target-neutral Network Authority v2 contract and fixtures."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.runtime_network_authority.v2.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/network-authority-v2.json")
FIXTURES = Path("stage1/compiler-contracts/fixtures/network-authority-v2")


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


def validate(value: Any, schema: dict[str, Any], path: str, defs: dict[str, Any]) -> None:
    reference = schema.get("$ref")
    if reference:
        prefix = "#/$defs/"
        require(isinstance(reference, str) and reference.startswith(prefix), f"{path}: unsupported reference")
        name = reference.removeprefix(prefix)
        require(name in defs, f"{path}: missing definition {name}")
        validate(value, defs[name], path, defs)
        return
    if "const" in schema:
        require(value == schema["const"], f"{path}: const mismatch")
    if "enum" in schema:
        require(value in schema["enum"], f"{path}: enum mismatch")
    if "type" in schema:
        require(kind(value) == schema["type"], f"{path}: expected {schema['type']}")
    if isinstance(value, dict):
        properties = schema.get("properties", {})
        for field in schema.get("required", []):
            require(field in value, f"{path}: missing {field}")
        if schema.get("additionalProperties") is False:
            require(not (set(value) - set(properties)), f"{path}: unknown fields")
        for field, nested in value.items():
            if field in properties:
                validate(nested, properties[field], f"{path}.{field}", defs)
    if isinstance(value, list):
        require(len(value) >= schema.get("minItems", 0), f"{path}: too few items")
        if "items" in schema:
            for index, item in enumerate(value):
                validate(item, schema["items"], f"{path}[{index}]", defs)
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: empty string")
        if "pattern" in schema:
            require(re.search(schema["pattern"], value) is not None, f"{path}: pattern mismatch")


def validate_fixture(name: str, fixture: dict[str, Any], snapshot: dict[str, Any]) -> None:
    if name == "allow-rules":
        require(fixture.get("direction") == "dns" and fixture.get("decision") == "allow", "allow fixture is valid")
    elif name == "deny-direction":
        require(fixture.get("direction") == "inbound_listen" and fixture.get("expected") == "deny", "listen denial fixture is valid")
        require(fixture.get("interface") != "loopback", "listen denial must be external")
    elif name == "dns-rebinding":
        require(fixture.get("rebinding") != "revalidate_before_use" and fixture.get("expected") == "deny", "rebinding fixture is valid")
    elif name == "runtime-endpoint":
        require(fixture.get("dynamic_endpoint") is True and fixture.get("expected") == "deny", "dynamic endpoint fixture is valid")
    else:
        raise ContractError(f"unknown fixture {name}")


def validate_contract(root: Path) -> dict[str, Any]:
    schema = load(root / SCHEMA)
    snapshot = load(root / SNAPSHOT)
    validate(snapshot, schema, "$", schema.get("$defs", {}))
    require(snapshot["directions"] == sorted(set(snapshot["directions"])), "directions must be sorted and unique")
    require(snapshot["directions"] == ["accepted_peer", "dns", "inbound_listen", "outbound_connect"], "direction surface drift")
    rules = snapshot["rules"]
    require(rules["inbound_listen"]["decision"] == "deny", "external listen must default to deny")
    require(rules["accepted_peer"]["decision"] == "deny", "accepted peers must default to deny")
    require(rules["dns"]["dynamic_endpoints"] == "validate_each_resolution", "DNS must validate dynamic endpoints")
    require(rules["dns"]["rebinding"] == "revalidate_before_use", "DNS rebinding must be revalidated")
    require({"requested_endpoint", "resolved_endpoint", "direction", "rule", "decision"} <= set(snapshot["audit"]["fields"]), "audit fields incomplete")
    require({"credentials", "query_values", "authorization_headers"} <= set(snapshot["audit"]["redaction"]), "audit redaction incomplete")
    seen: set[str] = set()
    fixture_root = root / FIXTURES
    for spec in snapshot["fixtures"]:
        name = spec["id"].rsplit("/", 1)[-1]
        require(name not in seen, f"duplicate fixture {name}")
        seen.add(name)
        validate_fixture(name, load(fixture_root / spec["path"]), snapshot)
    return {"schema": snapshot["schema_version"], "ok": True, "fixtures": len(seen), "directions": len(snapshot["directions"])}


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
            print(f"network-authority-v2: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "network-authority-v2: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
