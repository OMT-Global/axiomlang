#!/usr/bin/env python3
"""Validate direct-native or fail-closed evidence in stage1 smoke reports."""

from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
from typing import Any

SCHEMA_VERSION = "axiom.build-lowering-evidence.v1"
DIRECT_MODES = {
    "direct_native_runtime",
    "direct_native_runtime_with_static_folds",
}


def fail(message: str) -> None:
    raise SystemExit(message)


def assert_no_generated_rust(item: dict[str, Any], label: str) -> None:
    if item.get("generated_rust") is not None:
        fail(f"{label} emitted generated Rust")


def assert_no_binary(item: dict[str, Any], label: str) -> None:
    if item.get("binary") is not None:
        fail(f"{label} advertised a binary for not-produced lowering")


def assert_direct_native_lowering(lowering: Any, label: str) -> None:
    if not isinstance(lowering, dict):
        fail(f"{label} omitted versioned lowering evidence")
    mode = lowering.get("lowering_mode")
    expected = {
        "schema_version": SCHEMA_VERSION,
        "execution_mode": "direct_native_runtime",
        "direct_native_runtime": True,
        "known_value_static_folds": (
            mode == "direct_native_runtime_with_static_folds"
        ),
        "legacy_fallback_attempted": False,
    }
    if mode not in DIRECT_MODES or any(
        lowering.get(key) != value for key, value in expected.items()
    ):
        fail(f"{label} returned contradictory direct-native evidence")


def assert_blocked_lowering(lowering: Any, label: str) -> None:
    if not isinstance(lowering, dict):
        fail(f"{label} omitted versioned blocked-lowering evidence")
    expected = {
        "schema_version": SCHEMA_VERSION,
        "execution_mode": "not_produced",
        "lowering_mode": "runtime_lowering_required",
        "direct_native_runtime": False,
        "known_value_static_folds": False,
        "legacy_fallback_attempted": True,
    }
    if any(lowering.get(key) != value for key, value in expected.items()):
        fail(f"{label} returned contradictory blocked-lowering evidence")


def assert_bounded_static_lowering(lowering: Any, label: str) -> None:
    if not isinstance(lowering, dict):
        fail(f"{label} omitted versioned bounded-static evidence")
    expected = {
        "schema_version": SCHEMA_VERSION,
        "execution_mode": "bounded_static_output",
        "lowering_mode": "bounded_static_output",
        "direct_native_runtime": False,
        "known_value_static_folds": True,
        "legacy_fallback_attempted": False,
    }
    if any(lowering.get(key) != value for key, value in expected.items()):
        fail(f"{label} returned contradictory bounded-static evidence")


def validate_success(
    payload: dict[str, Any], command: str, project: str, expectation: str
) -> None:
    label = f"{command} for {project}"
    if payload.get("backend") != "cranelift":
        fail(
            f"{label} must run on cranelift, got {payload.get('backend')!r}"
        )
    if payload.get("ok") is not True:
        fail(f"{label} must pass on cranelift")
    assert_no_generated_rust(payload, label)
    assert_lowering = (
        assert_direct_native_lowering
        if expectation == "direct-native"
        else assert_bounded_static_lowering
    )
    if command == "build":
        assert_lowering(payload.get("lowering"), label)

    for package in payload.get("packages", []):
        if not isinstance(package, dict):
            continue
        package_label = (
            f"{command} package {package.get('package_root')} for {project}"
        )
        assert_no_generated_rust(package, package_label)
        assert_lowering(package.get("lowering"), package_label)

    cases = payload.get("cases", [])
    if command == "test" and (not isinstance(cases, list) or not cases):
        fail(f"{label} must report at least one test case")
    for case in cases:
        if not isinstance(case, dict):
            fail(f"{label} contains a non-object test case")
        case_label = f"{command} case {case.get('name')} for {project}"
        if case.get("ok") is not True:
            fail(f"{case_label} must pass")
        assert_no_generated_rust(case, case_label)
        assert_lowering(case.get("lowering"), case_label)


def validate_blocked(
    payload: dict[str, Any],
    command: str,
    project: str,
    expected_successes: list[str],
    expected_bounded_static: list[str],
    expected_blocked: list[str] | None,
) -> None:
    label = f"{command} for {project}"
    if payload.get("ok") is not False:
        fail(f"{label} must fail closed")
    assert_no_generated_rust(payload, label)

    if command == "build":
        assert_no_binary(payload, label)
        error = payload.get("error")
        code = error.get("code") if isinstance(error, dict) else None
        if code != "backend.runtime_lowering_required":
            fail(f"{label} returned unexpected error {code!r}")
        assert_blocked_lowering(payload.get("lowering"), label)

    cases = payload.get("cases", [])
    if not isinstance(cases, list):
        fail(f"{label} cases must be an array")
    actual_successes: list[str] = []
    actual_bounded_static: list[str] = []
    actual_blocked: list[str] = []
    for case in cases:
        if not isinstance(case, dict):
            fail(f"{label} contains a non-object test case")
        name = case.get("name")
        if not isinstance(name, str):
            fail(f"{label} contains a test case without a string name")
        if case.get("ok") is not True:
            actual_blocked.append(name)
            continue
        lowering = case.get("lowering")
        mode = lowering.get("lowering_mode") if isinstance(lowering, dict) else None
        if mode in DIRECT_MODES:
            actual_successes.append(name)
        elif mode == "bounded_static_output":
            actual_bounded_static.append(name)
        else:
            fail(f"{command} case {name} for {project} returned unknown lowering evidence")

    if Counter(actual_successes) != Counter(expected_successes):
        fail(
            f"{label} changed direct-native cases: expected "
            f"{sorted(expected_successes)}, got {sorted(actual_successes)}"
        )
    if Counter(actual_bounded_static) != Counter(expected_bounded_static):
        fail(
            f"{label} changed bounded-static cases: expected "
            f"{sorted(expected_bounded_static)}, got {sorted(actual_bounded_static)}"
        )
    if expected_blocked is not None and Counter(actual_blocked) != Counter(
        expected_blocked
    ):
        fail(
            f"{label} changed blocked cases: expected "
            f"{sorted(expected_blocked)}, got {sorted(actual_blocked)}"
        )
    if command == "test" and not actual_blocked:
        fail(f"{label} did not exercise a blocked case")

    for case in cases:
        if not isinstance(case, dict):
            fail(f"{label} contains a non-object test case")
        case_label = f"{command} case {case.get('name')} for {project}"
        assert_no_generated_rust(case, case_label)
        if case.get("ok") is True:
            mode = case.get("lowering", {}).get("lowering_mode")
            if mode in DIRECT_MODES:
                assert_direct_native_lowering(case.get("lowering"), case_label)
            else:
                assert_bounded_static_lowering(case.get("lowering"), case_label)
            continue
        assert_no_binary(case, case_label)
        error = case.get("error")
        code = error.get("code") if isinstance(error, dict) else None
        if code != "backend.runtime_lowering_required":
            fail(f"{case_label} returned unexpected error {code!r}")
        assert_blocked_lowering(case.get("lowering"), case_label)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--command", required=True, choices=("build", "test"))
    parser.add_argument("--project", required=True)
    parser.add_argument(
        "--expect",
        required=True,
        choices=("direct-native", "bounded-static", "blocked"),
    )
    parser.add_argument("--expected-success-case", action="append", default=[])
    parser.add_argument(
        "--expected-bounded-static-case", action="append", default=[]
    )
    parser.add_argument("--expected-blocked-case", action="append")
    args = parser.parse_args()

    payload = json.loads(args.report.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        fail("smoke report must be a JSON object")
    if args.expect != "blocked":
        validate_success(payload, args.command, args.project, args.expect)
        return
    validate_blocked(
        payload,
        args.command,
        args.project,
        args.expected_success_case,
        args.expected_bounded_static_case,
        (
            args.expected_blocked_case
            if args.expected_blocked_case is not None
            else None
        ),
    )


if __name__ == "__main__":
    main()
