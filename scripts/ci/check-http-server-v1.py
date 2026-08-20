#!/usr/bin/env python3
"""Validate the fail-closed HTTP server v1 evidence contract for issue #1449."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.runtime_http_server.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/http-server-v1.json")
FIXTURE_DIR = Path("stage1/compiler-contracts/fixtures/http-server-v1")

REQUEST_FIELDS = {"body_bytes", "body_text", "headers", "method", "path", "peer", "query"}
RESPONSE_FIELDS = {"body", "headers", "status"}
ENDPOINT_FIELDS = {"host", "ip", "port", "transport"}
INSPECTION_FIELDS = {
    "accept_queue_depth",
    "bind_authority",
    "handler_semantic_node",
    "in_flight",
    "limits",
    "listen_endpoint",
    "overload_count",
    "request_id",
    "response_evidence",
    "runtime_request_origin",
    "server_id",
    "shutdown_deadline",
    "shutdown_state",
}
OUTCOMES = {
    "accepted",
    "authority_denied",
    "cancelled",
    "deadline_exceeded",
    "io_error",
    "malformed_request",
    "overloaded",
    "response_sent",
    "server_stopped",
    "unsupported",
}
BLOCKERS = {1425, 1426, 1441, 1445, 1446, 1447}
FIXTURE_ID_PATTERN = "^axiom://http-server/fixture/[a-z0-9-]+$"
FIXTURE_FILE_PATTERN = r"^[a-z0-9-]+\.json$"
EVIDENCE_PATH_PATTERN = "^[a-zA-Z0-9_./-]+$"
FIXTURES = {
    "authorized-proxy-forged-forwarded-input": {
        "kind": "positive",
        "evidence_tier": "target",
        "authority": "proxy",
        "scenario": "receive a request whose authorized proxy replaces forged client Forwarded input with one canonical identity field",
        "operation": "trust_forwarded_headers",
        "expected": "accepted",
        "assertions": [
            "client-supplied Forwarded fields are removed before proxy emission",
            "proxy emits exactly one Forwarded field with one canonical for parameter",
            "effective forwarded client identity equals the canonical proxy value",
            "request peer remains the transport-observed proxy peer",
        ],
        "details": {
            "transport_peer": "192.0.2.10",
            "client_supplied_forwarded": "for=198.51.100.99",
            "proxy_emitted_forwarded": "for=203.0.113.7",
            "forwarded_field_count": 1,
            "forwarded_element_count": 1,
            "effective_client_identity": "203.0.113.7",
            "effective_peer": "192.0.2.10",
            "proxy_authority_match": True,
        },
    },
    "cancellation-shutdown": {
        "kind": "positive",
        "evidence_tier": "target",
        "scenario": "cancel the server scope while accepted requests remain active",
        "operation": "cancel_server",
        "expected": "cancelled",
        "assertions": [
            "accept stops before cancellation propagates",
            "active handlers receive cooperative cancellation",
            "deadline expiry force closes remaining resources",
            "terminal outcome is cancelled",
        ],
    },
    "conflicting-content-length": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "send two different Content-Length values before a request body",
        "operation": "parse_request",
        "expected": "malformed_request",
        "assertions": ["body is not allocated", "connection closes", "handler is not invoked"],
    },
    "current-loopback-floor": {
        "kind": "positive",
        "evidence_tier": "runtime",
        "authority": "net",
        "scenario": "serve a bounded fixed route on a loopback endpoint under the current net grant after build",
        "operation": "serve",
        "expected": "response_sent",
        "assertions": [
            "build reports no runtime replay",
            "loopback bind is runtime checked",
            "request content originates after build",
            "request count is bounded",
        ],
    },
    "current-loopback-policy-denial": {
        "kind": "negative",
        "evidence_tier": "runtime",
        "authority": "net",
        "scenario": "attempt a non-loopback bind under the current loopback-only runtime policy",
        "operation": "bind",
        "expected": "authority_denied",
        "assertions": [
            "current runtime reports the loopback-only denial",
            "listener is not created",
            "no fallback endpoint is selected",
        ],
    },
    "double-response": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "a handler attempts to commit a second response for one request",
        "operation": "respond",
        "expected": "io_error",
        "assertions": [
            "connection resource closes once",
            "first terminal response remains authoritative",
            "second response is rejected",
        ],
    },
    "dynamic-request-response": {
        "kind": "positive",
        "evidence_tier": "target",
        "scenario": "derive status headers and byte body from runtime method path query headers peer and body",
        "operation": "handle",
        "expected": "response_sent",
        "assertions": [
            "declared response headers are transmitted",
            "handler receives every request field",
            "response body preserves runtime bytes",
            "status is preserved",
        ],
    },
    "external-authorized-bind": {
        "kind": "positive",
        "evidence_tier": "target",
        "scenario": "listen on an exact non-loopback endpoint granted by listen authority",
        "operation": "bind",
        "expected": "accepted",
        "assertions": [
            "endpoint is resolved at runtime",
            "endpoint is revalidated before bind",
            "no broader wildcard is inferred",
            "resolved endpoint matches authority",
        ],
    },
    "graceful-drain": {
        "kind": "positive",
        "evidence_tier": "target",
        "scenario": "receive controlled shutdown while bounded handlers are active",
        "operation": "controlled_shutdown",
        "expected": "server_stopped",
        "assertions": [
            "accept stops before drain",
            "observability flushes before stop",
            "remaining handlers cancel at deadline",
            "resources close exactly once",
        ],
    },
    "head-response-body-suppressed": {
        "kind": "positive",
        "evidence_tier": "target",
        "scenario": "serve a HEAD response while suppressing handler body bytes on the wire",
        "operation": "respond",
        "expected": "response_sent",
        "assertions": [
            "handler body bytes are not transmitted",
            "Content-Length may describe the selected representation",
            "connection framing remains synchronized",
        ],
        "details": {"method": "HEAD", "selected_representation_bytes": 12, "transmitted_body_bytes": 0},
    },
    "handler-content-length": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "a handler attempts to set Content-Length instead of leaving response framing to the server",
        "operation": "respond",
        "expected": "io_error",
        "assertions": [
            "application framing header is rejected",
            "server does not transmit the handler Content-Length",
            "connection closes without response desynchronization",
        ],
    },
    "handler-queue-overload": {
        "kind": "positive",
        "evidence_tier": "target",
        "scenario": "dispatch an accepted request after the bounded handler queue reaches capacity",
        "operation": "dispatch_request",
        "expected": "overloaded",
        "assertions": [
            "accepted request receives bounded 503",
            "handler queue depth never exceeds limit",
            "in-flight handlers remain owned",
            "listener capacity policy is unchanged",
        ],
    },
    "handler-transfer-encoding": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "a handler attempts to set Transfer-Encoding instead of leaving response framing to the server",
        "operation": "respond",
        "expected": "io_error",
        "assertions": [
            "application framing header is rejected",
            "server does not transmit the handler Transfer-Encoding",
            "connection closes without response desynchronization",
        ],
    },
    "incomplete-content-length": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "close a request body after fewer bytes than its declared Content-Length",
        "operation": "read_body",
        "expected": "malformed_request",
        "assertions": [
            "partial body stays bounded",
            "premature end of stream is rejected",
            "handler is not invoked",
            "connection closes",
        ],
        "details": {"content_length": 5, "received_body_bytes": 3, "premature_eof": True},
    },
    "invalid-response-header-name": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "a handler returns a response header name that is not an RFC HTTP token",
        "operation": "respond",
        "expected": "io_error",
        "assertions": [
            "invalid field-name syntax is rejected",
            "ambiguous framing header is not transmitted",
            "connection closes without response desynchronization",
        ],
        "details": {"header_name": "Content-Length "},
    },
    "listener-saturation": {
        "kind": "positive",
        "evidence_tier": "target",
        "scenario": "reach the connection or listener capacity before another connection is accepted",
        "operation": "accept_capacity",
        "expected": "overloaded",
        "assertions": [
            "accept readiness pauses until capacity returns",
            "connection remains unaccepted by the application",
            "no HTTP response is emitted",
            "resume preserves the connection bound",
        ],
    },
    "malformed-request": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "send a malformed request line and invalid header name",
        "operation": "parse_request",
        "expected": "malformed_request",
        "assertions": [
            "bounded error may be emitted",
            "connection closes",
            "handler is not invoked",
            "parser remains within limits",
        ],
    },
    "oversize-header-bytes": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "send header bytes one byte above the published aggregate ceiling",
        "operation": "parse_request",
        "expected": "malformed_request",
        "assertions": ["handler is not invoked", "header storage stays bounded", "connection closes"],
        "details": {"observed_bytes": 65537, "maximum_bytes": 65536},
    },
    "oversize-start-line": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "send a request start line one byte above the published ceiling",
        "operation": "parse_request",
        "expected": "malformed_request",
        "assertions": ["handler is not invoked", "start-line storage stays bounded", "connection closes"],
        "details": {"observed_bytes": 8193, "maximum_bytes": 8192},
    },
    "oversize-body": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "declare a request body larger than the configured byte ceiling",
        "operation": "read_body",
        "expected": "malformed_request",
        "assertions": ["body allocation is bounded", "connection closes", "handler is not invoked"],
    },
    "proxy-http11": {
        "kind": "positive",
        "evidence_tier": "target",
        "scenario": "serve bounded HTTP/1.1 requests behind an explicitly trusted reverse proxy",
        "operation": "serve_http11",
        "expected": "response_sent",
        "authority": "listen+proxy",
        "assertions": [
            "forwarded headers require proxy authority",
            "keep alive request count is bounded",
            "response framing is valid",
            "untrusted peer remains transport observed",
        ],
    },
    "response-header-injection": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "a handler returns a response header containing a line break",
        "operation": "respond",
        "expected": "io_error",
        "assertions": [
            "control characters are rejected",
            "invalid header is not transmitted",
            "terminal connection cleanup runs",
        ],
    },
    "sigterm-shutdown": {
        "kind": "positive",
        "evidence_tier": "target",
        "scenario": "deliver SIGTERM while bounded handlers remain active",
        "operation": "signal_shutdown",
        "expected": "server_stopped",
        "assertions": [
            "accept stops before drain",
            "active handlers drain only to the shutdown deadline",
            "observability flushes before stop",
            "remaining handlers cancel after the deadline",
            "resources close exactly once",
        ],
    },
    "slow-client": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "send request headers too slowly to complete before the read deadline",
        "operation": "read_request",
        "expected": "deadline_exceeded",
        "assertions": [
            "connection closes at deadline",
            "handler slot is released",
            "partial input stays bounded",
        ],
    },
    "status-204-body-rejected": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "a handler returns body bytes for a 204 response",
        "operation": "respond",
        "expected": "io_error",
        "assertions": [
            "handler body bytes are rejected",
            "Content-Length is omitted",
            "connection framing remains synchronized",
        ],
        "details": {"status": 204, "handler_body_bytes": 1},
    },
    "status-205-body-rejected": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "a handler returns body bytes for a 205 response",
        "operation": "respond",
        "expected": "io_error",
        "assertions": [
            "handler body bytes are rejected",
            "Content-Length is emitted as zero",
            "connection framing remains synchronized",
        ],
        "details": {"status": 205, "handler_body_bytes": 1, "content_length": 0},
    },
    "status-304-response-body-suppressed": {
        "kind": "positive",
        "evidence_tier": "target",
        "scenario": "serve a 304 response while suppressing handler body bytes on the wire",
        "operation": "respond",
        "expected": "response_sent",
        "assertions": [
            "handler body bytes are not transmitted",
            "Content-Length may describe the selected representation",
            "connection framing remains synchronized",
        ],
        "details": {"status": 304, "selected_representation_bytes": 12, "transmitted_body_bytes": 0},
    },
    "thread-per-connection": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "select an unbounded independent execution context for every accepted connection",
        "operation": "schedule_handler",
        "expected": "unsupported",
        "assertions": [
            "bounded structured task scope is required",
            "connection count remains bounded",
            "fallback is rejected",
        ],
    },
    "too-many-headers": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "send one more header field than the published count ceiling",
        "operation": "parse_request",
        "expected": "malformed_request",
        "assertions": ["handler is not invoked", "header count stays bounded", "connection closes"],
        "details": {"observed_count": 65, "maximum_count": 64},
    },
    "transfer-encoding-content-length": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "send both Transfer-Encoding and Content-Length before a request body",
        "operation": "parse_request",
        "expected": "malformed_request",
        "assertions": ["ambiguous framing is rejected", "body is not allocated", "handler is not invoked", "connection closes"],
        "details": {"transfer_encoding": "chunked", "content_length": "5", "body_allocation_bytes": 0},
    },
    "unauthorized-bind": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "bind a wildcard or resolved endpoint outside exact listen authority",
        "operation": "bind",
        "expected": "authority_denied",
        "assertions": [
            "listener is not created",
            "no fallback endpoint is selected",
            "resolved endpoint is reported",
        ],
    },
    "unsupported-chunked-transfer": {
        "kind": "negative",
        "evidence_tier": "target",
        "scenario": "send chunked Transfer-Encoding when the v1 parser does not implement chunk decoding",
        "operation": "parse_request",
        "expected": "unsupported",
        "assertions": ["unsupported framing is rejected", "body is not allocated", "handler is not invoked", "connection closes"],
        "details": {"transfer_encoding": "chunked", "content_length": None, "body_allocation_bytes": 0},
    },
    "untrusted-forwarded-headers": {
        "kind": "negative",
        "evidence_tier": "target",
        "authority": "proxy",
        "scenario": "receive forwarded client metadata from a transport peer outside proxy authority",
        "operation": "trust_forwarded_headers",
        "expected": "authority_denied",
        "assertions": ["forwarded metadata is ignored", "transport peer remains authoritative", "handler cannot observe spoofed client identity"],
        "details": {"transport_peer": "198.51.100.7", "claimed_peer": "10.0.0.1", "proxy_authority_match": False, "effective_peer": "198.51.100.7"},
    },
}
CURRENT_IMPLEMENTATION = {
    "tier": "static_spike",
    "status": "partial",
    "runtime_backed_subset": True,
    "build_once_run_many": True,
    "loopback_only": True,
    "bounded_fixed_route": True,
    "dynamic_handler": False,
    "external_bind": False,
    "structured_concurrency": False,
    "http_1_1_proxy": False,
    "graceful_drain": False,
    "observability_flush": False,
}
CURRENT_EVIDENCE = [
    {
        "path": "docs/direct-native-runtime-abi-v0.md",
        "anchors": [
            "The HTTP server row remains implemented for its evidenced native subset:",
            "Scheduler-backed serving, concurrent clients, cancellation, timeouts, and",
        ],
    },
    {
        "path": "stage1/crates/axiomc/src/codegen.rs",
        "anchors": [
            "fn axiom_http_serve_route_on_listener(",
            "http server max_requests must be between 1 and 1024",
            "std::thread::spawn",
        ],
    },
    {
        "path": "stage1/crates/axiomc/src/stdlib.rs",
        "anchors": [
            "pub fn serve(bind: string, selected_route: HttpRoute, max_requests: int): bool",
            "pub fn serve_once(bind: string, body: string): bool",
        ],
    },
    {
        "path": "stage1/crates/axiomc/tests/support/lib_unit.rs",
        "anchors": [
            "fn stage1_stdlib_http_service_rejects_non_loopback_bind()",
            "fn stage1_stdlib_http_service_routes_multiple_requests()",
            "fn stage1_stdlib_http_service_serves_one_request()",
        ],
    },
]
CURRENT_HTTP_FUNCTIONS = {
    "accept",
    "close",
    "fixed_route",
    "get",
    "header",
    "listen",
    "local_port",
    "request",
    "respond",
    "response",
    "route",
    "route_response",
    "serve",
    "serve_once",
    "text_response",
}
FORBIDDEN_CAPTURE_TERMS = {"axum", "cargo", "cranelift", "hyper", "rust", "tokio"}
MAX_CHECKED_FILE_BYTES = 1024 * 1024
SCHEMA_SHA256 = "0b9339699f2b161134818ef1f43c5dd80c43748d06c149a91cfa11a0fe3059a2"


class ContractError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def resolve_checked_file(root: Path, relative: Path) -> Path:
    resolved_root = root.resolve(strict=True)
    require(
        bool(relative.parts) and not relative.is_absolute() and ".." not in relative.parts,
        f"unsafe checkout path: {relative}",
    )
    candidate = resolved_root
    for component in relative.parts:
        candidate /= component
        require(
            not candidate.is_symlink(),
            f"checkout path uses a symlink component: {relative}",
        )
    resolved = candidate.resolve(strict=True)
    require(resolved.is_relative_to(resolved_root), f"checkout path escapes root: {relative}")
    require(resolved.is_file(), f"checkout path is not a regular file: {relative}")
    require(
        resolved.stat().st_size <= MAX_CHECKED_FILE_BYTES,
        f"checkout file exceeds {MAX_CHECKED_FILE_BYTES} bytes: {relative}",
    )
    return resolved


def read_checked_text(root: Path, relative: Path) -> str:
    return resolve_checked_file(root, relative).read_text(encoding="utf-8")


def load_object(path: Path, *, root: Path | None = None) -> dict[str, Any]:
    try:
        content = read_checked_text(root, path) if root is not None else path.read_text(encoding="utf-8")
        value = json.loads(content)
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot read {path}: {error}") from error
    require(isinstance(value, dict), f"{path} must contain an object")
    return value


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


def json_equal(left: Any, right: Any) -> bool:
    left_number = isinstance(left, (int, float)) and not isinstance(left, bool)
    right_number = isinstance(right, (int, float)) and not isinstance(right, bool)
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
    return json_type(left) == json_type(right) and left == right


def matches_json_type(value: Any, expected: str) -> bool:
    if expected == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if expected == "integer":
        return (
            isinstance(value, (int, float))
            and not isinstance(value, bool)
            and (not isinstance(value, float) or value.is_integer())
        )
    return json_type(value) == expected


def matches_trusted_pattern(value: str, pattern: str) -> bool:
    if pattern == FIXTURE_ID_PATTERN:
        prefix = "axiom://http-server/fixture/"
        suffix = value.removeprefix(prefix)
        return (
            value.startswith(prefix)
            and bool(suffix)
            and all(character.isascii() and (character.islower() or character.isdigit() or character == "-") for character in suffix)
        )
    if pattern == FIXTURE_FILE_PATTERN:
        stem = value.removesuffix(".json")
        return (
            value.endswith(".json")
            and bool(stem)
            and all(character.isascii() and (character.islower() or character.isdigit() or character == "-") for character in stem)
        )
    if pattern == EVIDENCE_PATH_PATTERN:
        return bool(value) and all(
            character.isascii()
            and (character.isalpha() or character.isdigit() or character in "_./-")
            for character in value
        )
    raise ContractError("untrusted or unsupported schema pattern")


def validate_schema_node(
    value: Any,
    schema: dict[str, Any],
    path: str,
    definitions: dict[str, Any],
) -> None:
    for index, nested in enumerate(schema.get("allOf", [])):
        validate_schema_node(value, nested, f"{path}.allOf[{index}]", definitions)
    if "if" in schema:
        try:
            validate_schema_node(value, schema["if"], path, definitions)
        except ContractError:
            branch = schema.get("else")
        else:
            branch = schema.get("then")
        if branch is not None:
            validate_schema_node(value, branch, path, definitions)
    reference = schema.get("$ref")
    if reference is not None:
        prefix = "#/$defs/"
        require(
            isinstance(reference, str) and reference.startswith(prefix),
            f"{path}: unsupported schema reference",
        )
        name = reference.removeprefix(prefix)
        require(name in definitions, f"{path}: missing schema definition {name}")
        validate_schema_node(value, definitions[name], path, definitions)
        return
    if "const" in schema:
        require(json_equal(value, schema["const"]), f"{path}: expected {schema['const']!r}")
    if "enum" in schema:
        require(
            any(json_equal(value, candidate) for candidate in schema["enum"]),
            f"{path}: value is outside enum",
        )
    expected = schema.get("type")
    if expected is not None:
        require(matches_json_type(value, expected), f"{path}: expected {expected}, got {json_type(value)}")
    if isinstance(value, dict):
        required = schema.get("required", [])
        missing = sorted(set(required) - set(value))
        require(not missing, f"{path}: missing required fields {missing}")
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            unknown = sorted(set(value) - set(properties))
            require(not unknown, f"{path}: unknown fields {unknown}")
        for key, nested in value.items():
            if key in properties:
                validate_schema_node(nested, properties[key], f"{path}.{key}", definitions)
    if isinstance(value, list):
        require(len(value) >= schema.get("minItems", 0), f"{path}: too few items")
        if "maxItems" in schema:
            require(len(value) <= schema["maxItems"], f"{path}: too many items")
        if schema.get("uniqueItems"):
            require(
                all(
                    not json_equal(value[left], value[right])
                    for left in range(len(value))
                    for right in range(left + 1, len(value))
                ),
                f"{path}: duplicate items",
            )
        if "contains" in schema:
            matches = 0
            for index, item in enumerate(value):
                try:
                    validate_schema_node(item, schema["contains"], f"{path}[{index}]", definitions)
                except ContractError:
                    continue
                matches += 1
            require(matches >= schema.get("minContains", 1), f"{path}: too few matching items")
            if "maxContains" in schema:
                require(matches <= schema["maxContains"], f"{path}: too many matching items")
        item_schema = schema.get("items")
        if item_schema:
            for index, item in enumerate(value):
                validate_schema_node(item, item_schema, f"{path}[{index}]", definitions)
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: string is empty")
        pattern = schema.get("pattern")
        if pattern:
            require(matches_trusted_pattern(value, pattern), f"{path}: invalid string form")
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if "minimum" in schema:
            require(value >= schema["minimum"], f"{path}: value is below minimum")
        if "maximum" in schema:
            require(value <= schema["maximum"], f"{path}: value is above maximum")


def require_sorted_exact(values: list[Any], expected: set[Any], label: str) -> None:
    require(set(values) == expected, f"{label} are incomplete")
    require(values == sorted(values), f"{label} must be deterministically ordered")
    require(len(values) == len(set(values)), f"{label} must be unique")


def struct_fields(module: str, name: str) -> set[str]:
    match = re.search(rf"pub struct {re.escape(name)} \{{(.*?)\n\}}", module, re.DOTALL)
    require(match is not None, f"std/http.ax lost {name}")
    return set(re.findall(r"^([a-z][a-z0-9_]*):", match.group(1), re.MULTILINE))


def validate_fixture(root: Path, relative: Path, reference: dict[str, Any]) -> None:
    fixture = load_object(relative, root=root)
    name = reference["id"].rsplit("/", 1)[-1]
    semantics = FIXTURES[name]
    expected = {
        "schema_version": "axiom.http_server_fixture.v1",
        "id": reference["id"],
        "kind": semantics["kind"],
        "evidence_tier": semantics["evidence_tier"],
        "scenario": semantics["scenario"],
        "authority": semantics.get("authority", "listen"),
        "operation": semantics["operation"],
        "expected": semantics["expected"],
        "assertions": semantics["assertions"],
    }
    if "details" in semantics:
        expected["details"] = semantics["details"]
    require(fixture == expected, f"{relative.name}: exact fixture semantics drifted")
    require(reference["kind"] == semantics["kind"], f"{relative.name}: kind disagrees with snapshot")
    require(
        reference["evidence_tier"] == semantics["evidence_tier"],
        f"{relative.name}: evidence tier disagrees with snapshot",
    )
    require(fixture["expected"] in OUTCOMES, f"{relative.name}: unknown outcome")
    if fixture["kind"] == "negative":
        require(fixture["expected"] not in {"accepted", "response_sent"}, f"{relative.name}: negative fixture succeeded")


def validate_current_evidence(root: Path, evidence: list[dict[str, Any]]) -> None:
    require(evidence == CURRENT_EVIDENCE, "current HTTP server evidence set drifted")
    for item in evidence:
        relative = Path(item["path"])
        content = read_checked_text(root, relative)
        for anchor in item["anchors"]:
            require(anchor in content, f"evidence anchor missing from {relative}: {anchor}")


def validate_current_implementation(root: Path, snapshot: dict[str, Any]) -> None:
    stdlib = read_checked_text(root, Path("stage1/crates/axiomc/src/stdlib.rs"))
    start = stdlib.index('        "http.ax",')
    end = stdlib.index('        "http_async.ax",', start)
    http_module = stdlib[start:end]
    functions = set(re.findall(r"\bpub fn ([a-z][a-z0-9_]*)", http_module))
    require(functions == CURRENT_HTTP_FUNCTIONS, "current std/http.ax function floor drifted")
    require(
        struct_fields(http_module, "HttpRequest") == {"body", "method", "path", "stream"},
        "current HttpRequest floor drifted",
    )
    require(
        struct_fields(http_module, "HttpResponse") == {"body", "headers", "status"},
        "current HttpResponse floor drifted",
    )
    require(
        "return http_response_write(request.stream, status, body)" in http_module,
        "current response helper evidence drifted",
    )

    codegen = read_checked_text(root, Path("stage1/crates/axiomc/src/codegen.rs"))
    for marker in (
        "axiom_http_loopback_bind_addr",
        "http server max_requests must be between 1 and 1024",
        "std::thread::spawn",
        "HTTP/1.0",
        "Duration::from_secs(5)",
        "axiom_http_request_part",
    ):
        require(marker in codegen, f"current HTTP server evidence lost {marker}")

    runtime_doc = read_checked_text(root, Path("docs/direct-native-runtime-abi-v0.md"))
    for marker in ("HTTP server row remains implemented for its evidenced native subset", "native loopback servers", "Non-loopback policy coverage"):
        require(marker in runtime_doc, f"direct-native HTTP server evidence lost {marker}")

    require(snapshot["implementation"] == CURRENT_IMPLEMENTATION, "current implementation evidence was promoted or drifted")
    runtime_fixtures = {
        name for name, semantics in FIXTURES.items() if semantics["evidence_tier"] == "runtime"
    }
    require(
        runtime_fixtures == {"current-loopback-floor", "current-loopback-policy-denial"},
        "runtime fixture claims drifted",
    )


def validate_contract(root: Path) -> dict[str, Any]:
    schema_text = read_checked_text(root, SCHEMA)
    require(
        hashlib.sha256(schema_text.encode("utf-8")).hexdigest() == SCHEMA_SHA256,
        "HTTP server schema byte contract drifted",
    )
    schema = load_object(SCHEMA, root=root)
    snapshot = load_object(SNAPSHOT, root=root)
    require(schema.get("$id", "").endswith("axiom.runtime_http_server.v1.schema.json"), "HTTP server schema id mismatch")
    validate_schema_node(snapshot, schema, "$", schema.get("$defs", {}))
    require(
        (snapshot["schema_version"], snapshot["contract"], snapshot["issue"])
        == ("axiom.runtime_http_server.v1", "runtime.http_server", 1449),
        "HTTP server snapshot identity mismatch",
    )
    require_sorted_exact(snapshot["authority"]["endpoint_fields"], ENDPOINT_FIELDS, "endpoint fields")
    require_sorted_exact(snapshot["request"]["fields"], REQUEST_FIELDS, "request fields")
    require_sorted_exact(snapshot["request"]["body_representations"], {"bytes", "text"}, "request bodies")
    require_sorted_exact(snapshot["response"]["fields"], RESPONSE_FIELDS, "response fields")
    require_sorted_exact(snapshot["response"]["body_representations"], {"bytes", "text"}, "response bodies")
    require_sorted_exact(snapshot["protocol"]["later_work"], {"http_2", "websocket"}, "later protocol work")
    require_sorted_exact(snapshot["shutdown"]["states"], {"accepting", "draining", "starting", "stopped"}, "shutdown states")
    require_sorted_exact(snapshot["shutdown"]["triggers"], {"controlled", "sigterm"}, "shutdown triggers")
    require_sorted_exact(snapshot["inspection_fields"], INSPECTION_FIELDS, "inspection fields")
    require_sorted_exact(snapshot["outcomes"], OUTCOMES, "outcomes")
    require_sorted_exact(snapshot["migration"]["blocker_issues"], BLOCKERS, "blocker issues")

    limits = snapshot["limits"]
    require(limits["max_in_flight"] <= limits["max_connections"], "in-flight limit exceeds connections")
    require(limits["handler_queue"] <= limits["max_connections"], "handler queue exceeds connections")
    require(limits["max_requests_per_connection"] <= 1024, "keep-alive request bound exceeds v1 ceiling")
    require(limits["body_bytes"] == 1024 * 1024, "request body limit drifted from its fixture boundary")
    require(limits["start_line_bytes"] == 8192, "start-line limit drifted from its fixture boundary")
    require(limits["header_bytes"] == 64 * 1024, "header byte limit drifted from its fixture boundary")
    require(limits["header_count"] == 64, "header count drifted from its fixture boundary")
    require(
        limits["backpressure"]
        == {
            "bounded_queues": True,
            "listener_capacity": {
                "action": "pause_accept_until_capacity",
                "connection_state": "not_accepted",
                "http_response_possible": False,
            },
            "handler_queue_capacity": {
                "action": "reject_accepted_request",
                "connection_state": "accepted",
                "overload_status": 503,
                "preserve_in_flight": True,
            },
            "partial_write_progress": True,
            "zero_progress_rule": "wait_for_writable_or_deadline",
        },
        "listener and handler backpressure policies drifted",
    )
    require(snapshot["protocol"]["versions"] == ["HTTP/1.1"], "proxy protocol must be HTTP/1.1")
    require(snapshot["shutdown"]["flush_observability"], "shutdown must flush observability")

    fixture_refs = snapshot["fixtures"]
    require(
        [reference["id"] for reference in fixture_refs]
        == sorted(reference["id"] for reference in fixture_refs),
        "fixture references must be deterministically ordered",
    )
    fixture_names = {reference["id"].rsplit("/", 1)[-1] for reference in fixture_refs}
    require(fixture_names == set(FIXTURES), "HTTP server fixture coverage is incomplete")
    fixture_files = {path.name for path in (root / FIXTURE_DIR).glob("*.json")}
    require(fixture_files == {f"{name}.json" for name in FIXTURES}, "HTTP server fixture files drifted")
    for reference in fixture_refs:
        name = reference["id"].rsplit("/", 1)[-1]
        require(reference["file"] == f"{name}.json", f"fixture filename drifted for {name}")
        require(reference["kind"] == FIXTURES[name]["kind"], f"fixture kind drifted for {name}")
        validate_fixture(root, FIXTURE_DIR / reference["file"], reference)

    capture = json.dumps(
        {
            "authority": snapshot["authority"],
            "request": snapshot["request"],
            "response": snapshot["response"],
            "handler": snapshot["handler"],
            "limits": snapshot["limits"],
            "protocol": snapshot["protocol"],
            "shutdown": snapshot["shutdown"],
            "inspection_fields": snapshot["inspection_fields"],
        }
    ).casefold()
    require(
        not any(re.search(rf"\b{re.escape(term)}\b", capture) for term in FORBIDDEN_CAPTURE_TERMS),
        "HTTP server contract leaks host implementation vocabulary",
    )
    validate_current_evidence(root, snapshot["migration"]["current_evidence"])
    validate_current_implementation(root, snapshot)
    return {
        "schema": snapshot["schema_version"],
        "ok": True,
        "fixtures": len(FIXTURES),
        "request_fields": len(REQUEST_FIELDS),
        "inspection_fields": len(INSPECTION_FIELDS),
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        root = args.root.resolve(strict=True)
        require(root.is_dir(), "HTTP server checkout root must be a directory")
        result = validate_contract(root)
    except (ContractError, OSError, TypeError, KeyError, ValueError) as error:
        if args.json:
            print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True))
        else:
            print(f"http-server-v1: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "http-server-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
