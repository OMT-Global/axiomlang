#!/usr/bin/env python3
"""Small unconditional Draft 2020-12 validator for repository policy schemas.

This intentionally implements only the keywords used by the checked
Compatibility v1 policy schema. Unsupported keywords fail closed.
"""

from __future__ import annotations

import json
import re
from typing import Any


SUPPORTED_ANNOTATIONS = {"$schema", "$id", "$defs", "title", "description", "default"}
SUPPORTED_ASSERTIONS = {
    "$ref",
    "type",
    "const",
    "enum",
    "required",
    "properties",
    "additionalProperties",
    "pattern",
    "minLength",
    "minItems",
    "maxItems",
    "uniqueItems",
    "prefixItems",
    "items",
    "oneOf",
    "allOf",
    "if",
    "then",
    "else",
    "not",
    "minimum",
    "maximum",
}


def _type_matches(value: Any, expected: str) -> bool:
    return {
        "object": isinstance(value, dict),
        "array": isinstance(value, list),
        "string": isinstance(value, str),
        "integer": isinstance(value, int) and not isinstance(value, bool),
        "number": isinstance(value, (int, float)) and not isinstance(value, bool),
        "boolean": isinstance(value, bool),
        "null": value is None,
    }.get(expected, False)


def _resolve_ref(root: dict[str, Any], ref: str) -> dict[str, Any]:
    if not ref.startswith("#/"):
        raise ValueError(f"unsupported external JSON Schema reference {ref!r}")
    value: Any = root
    for raw in ref[2:].split("/"):
        key = raw.replace("~1", "/").replace("~0", "~")
        if not isinstance(value, dict) or key not in value:
            raise ValueError(f"unresolvable JSON Schema reference {ref!r}")
        value = value[key]
    if not isinstance(value, dict):
        raise ValueError(f"JSON Schema reference {ref!r} does not resolve to an object")
    return value


def _validate(instance: Any, schema: Any, root: dict[str, Any], path: str) -> None:
    if schema is False:
        raise ValueError(f"{path} is forbidden by the policy schema")
    if schema is True:
        return
    if not isinstance(schema, dict):
        raise ValueError(f"policy schema node at {path} must be an object or boolean")
    unsupported = sorted(set(schema) - SUPPORTED_ANNOTATIONS - SUPPORTED_ASSERTIONS)
    if unsupported:
        raise ValueError(
            f"policy schema uses unsupported keywords at {path}: {', '.join(unsupported)}"
        )
    if "$ref" in schema:
        if len(set(schema) - SUPPORTED_ANNOTATIONS - {"$ref"}) > 0:
            raise ValueError(f"policy schema combines $ref with assertions at {path}")
        _validate(instance, _resolve_ref(root, schema["$ref"]), root, path)
        return
    if "oneOf" in schema:
        matches = 0
        failures = []
        for branch in schema["oneOf"]:
            try:
                _validate(instance, branch, root, path)
                matches += 1
            except ValueError as error:
                failures.append(str(error))
        if matches != 1:
            raise ValueError(
                f"{path} must match exactly one oneOf branch: {'; '.join(failures)}"
            )
    if "not" in schema:
        try:
            _validate(instance, schema["not"], root, path)
        except ValueError:
            pass
        else:
            raise ValueError(f"{path} must not match the forbidden schema")
    for branch in schema.get("allOf", []):
        _validate(instance, branch, root, path)
    if "if" in schema:
        try:
            _validate(instance, schema["if"], root, path)
            condition = True
        except ValueError:
            condition = False
        branch = schema.get("then") if condition else schema.get("else")
        if branch is not None:
            _validate(instance, branch, root, path)
    if "const" in schema and instance != schema["const"]:
        raise ValueError(f"{path} must equal {schema['const']!r}")
    if "enum" in schema and instance not in schema["enum"]:
        raise ValueError(f"{path} must be one of {schema['enum']!r}")
    expected_type = schema.get("type")
    if expected_type is not None:
        types = [expected_type] if isinstance(expected_type, str) else expected_type
        if not isinstance(types, list) or not all(isinstance(item, str) for item in types):
            raise ValueError(f"policy schema type at {path} is invalid")
        if not any(_type_matches(instance, item) for item in types):
            raise ValueError(f"{path} must have JSON type {' or '.join(types)}")
    if isinstance(instance, str):
        minimum = schema.get("minLength")
        if isinstance(minimum, int) and len(instance) < minimum:
            raise ValueError(f"{path} must have length at least {minimum}")
        pattern = schema.get("pattern")
        if isinstance(pattern, str) and re.search(pattern, instance) is None:
            raise ValueError(f"{path} does not match required pattern {pattern!r}")
    if isinstance(instance, (int, float)) and not isinstance(instance, bool):
        if "minimum" in schema and instance < schema["minimum"]:
            raise ValueError(f"{path} must be at least {schema['minimum']}")
        if "maximum" in schema and instance > schema["maximum"]:
            raise ValueError(f"{path} must be at most {schema['maximum']}")
    if isinstance(instance, dict):
        required = schema.get("required", [])
        if not isinstance(required, list) or not all(isinstance(key, str) for key in required):
            raise ValueError(f"policy schema required list at {path} is invalid")
        missing = [key for key in required if key not in instance]
        if missing:
            raise ValueError(f"{path} is missing required properties: {', '.join(missing)}")
        properties = schema.get("properties", {})
        if not isinstance(properties, dict):
            raise ValueError(f"policy schema properties at {path} must be an object")
        additional = schema.get("additionalProperties", True)
        unknown = sorted(set(instance) - set(properties))
        if additional is False and unknown:
            raise ValueError(f"{path} contains unknown properties: {', '.join(unknown)}")
        for key, value in instance.items():
            if key in properties:
                _validate(value, properties[key], root, f"{path}.{key}")
            elif isinstance(additional, dict):
                _validate(value, additional, root, f"{path}.{key}")
    if isinstance(instance, list):
        minimum = schema.get("minItems")
        if isinstance(minimum, int) and len(instance) < minimum:
            raise ValueError(f"{path} must contain at least {minimum} items")
        maximum = schema.get("maxItems")
        if isinstance(maximum, int) and len(instance) > maximum:
            raise ValueError(f"{path} must contain at most {maximum} items")
        if schema.get("uniqueItems") is True:
            encoded = [json.dumps(item, sort_keys=True, separators=(",", ":")) for item in instance]
            if len(encoded) != len(set(encoded)):
                raise ValueError(f"{path} must contain unique items")
        prefix = schema.get("prefixItems", [])
        if not isinstance(prefix, list):
            raise ValueError(f"policy schema prefixItems at {path} must be an array")
        for index, item_schema in enumerate(prefix):
            if index >= len(instance):
                break
            _validate(instance[index], item_schema, root, f"{path}[{index}]")
        items = schema.get("items", True)
        for index in range(len(prefix), len(instance)):
            _validate(instance[index], items, root, f"{path}[{index}]")


def validate_draft_2020_12(instance: Any, schema: dict[str, Any]) -> None:
    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
        raise ValueError("policy schema must declare JSON Schema Draft 2020-12")
    closed_object = (
        schema.get("type") == "object" and schema.get("additionalProperties") is False
    )
    if not closed_object and "oneOf" not in schema:
        raise ValueError("policy schema root must be a closed object")
    _validate(instance, schema, schema, "$")
