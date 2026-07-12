#!/usr/bin/env python3
"""Run the extended toolchain qualification suite and emit durable evidence."""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

SCHEMA = "axiom.toolchain_qualification.v0"

DEFAULT_CHECKS = [
    {"id": "full_crate_integration", "command": "RUST_MIN_STACK=8388608 cargo test --manifest-path stage1/Cargo.toml --workspace --all-targets --features run-native-tests --locked -- --test-threads=1"},
    {"id": "conformance", "command": "bash scripts/ci/run-stage1-conformance.sh"},
    {"id": "build_purity", "command": "bash scripts/ci/run-extended-stage1-checks.sh"},
    {"id": "proof_smoke", "command": "bash scripts/ci/run-stage1-proof-test.sh && bash scripts/ci/run-stage1-basic-smoke.sh && bash scripts/ci/run-stage1-stdlib-smoke.sh"},
    {"id": "schemas_protocol", "command": "cargo test --manifest-path stage1/Cargo.toml -p axiomc --test schema_metadata --test json_command_fixtures --test json_contract_snapshots --locked && bash scripts/ci/validate-capability-manifests.sh"},
    {"id": "lsp_protocol_smoke", "command": "cargo test --manifest-path stage1/Cargo.toml -p axiomc lsp --locked && python3 scripts/ci/check-command-lsp-boundary.py"},
    {"id": "direct_native_abi", "command": "bash scripts/ci/run-direct-native-runtime-abi-evidence.sh"},
    {"id": "runtime_sensitivity", "command": "cargo test --manifest-path stage1/Cargo.toml -p axiomc --test cranelift_backend --locked -- --test-threads=1"},
    {"id": "benchmark_comparison", "command": "python3 scripts/ci/check-stage1-benchmarks.py && python3 scripts/ci/report-stage1-reference-comparison.py"},
    {"id": "supply_chain", "command": "bash scripts/ci/run-toolchain-supply-chain.sh", "requiredTools": ["cargo-vet"]},
    {"id": "readiness_self_tests", "command": "bash scripts/ci/test-check-production-language-readiness.sh && bash scripts/ci/test-check-self-hosting-language-readiness.sh && bash scripts/ci/test-check-snapshot-bootstrap-readiness.sh && bash scripts/ci/test-check-python-exit-readiness.sh && bash scripts/ci/test-check-rust-exit-readiness.sh && python3 scripts/ci/check-production-language-readiness.py --validate-only"},
]


def args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--plan", type=Path, default=None,
                        help="JSON check plan for hermetic orchestrator tests")
    parser.add_argument("--head-sha", default=None)
    parser.add_argument("--target", default=None)
    parser.add_argument("--trigger", default=None)
    parser.add_argument("--fixture-duration-ms", type=int, default=None,
                        help="fixed per-check duration; accepted only with --plan")
    return parser.parse_args()


def git_head(root: Path) -> str:
    result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    if result.returncode != 0:
        raise RuntimeError(f"cannot resolve HEAD: {result.stderr.strip()}")
    return result.stdout.strip()


def host_target() -> str:
    result = subprocess.run(["rustc", "-vV"], text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, check=False)
    for line in result.stdout.splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ")
    return f"{platform.machine()}-{platform.system().lower()}"


def load_plan(path: Path | None) -> list[dict[str, Any]]:
    if path is None:
        return DEFAULT_CHECKS
    payload = json.loads(path.read_text(encoding="utf-8"))
    checks = payload.get("checks")
    if not isinstance(checks, list) or not checks:
        raise ValueError("qualification plan must contain a non-empty checks array")
    result: list[dict[str, Any]] = []
    for check in checks:
        if not isinstance(check, dict) or not isinstance(check.get("id"), str) or not isinstance(check.get("command"), str):
            raise ValueError("each qualification check requires string id and command")
        required_tools = check.get("requiredTools", [])
        skip_reason = check.get("skipReason")
        if not isinstance(required_tools, list) or not all(isinstance(tool, str) for tool in required_tools):
            raise ValueError("requiredTools must be an array of tool names")
        if skip_reason is not None and not isinstance(skip_reason, str):
            raise ValueError("skipReason must be a string")
        result.append({"id": check["id"], "command": check["command"],
                       "requiredTools": required_tools, "skipReason": skip_reason})
    return result


def classify(returncode: int, command: str) -> tuple[str, str]:
    if returncode == 0:
        return "passed", "none"
    if returncode in (126, 127):
        return "failed", "infrastructure_failure"
    return "failed", "product_failure"


def main() -> int:
    options = args()
    root = options.repo_root.resolve()
    output = options.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if options.fixture_duration_ms is not None and options.plan is None:
        raise SystemExit("--fixture-duration-ms requires --plan")

    checks = load_plan(options.plan)
    head = options.head_sha or os.environ.get("GITHUB_SHA") or git_head(root)
    target = options.target or os.environ.get("AXIOM_QUALIFICATION_TARGET") or host_target()
    trigger = options.trigger or os.environ.get("AXIOM_QUALIFICATION_TRIGGER") or os.environ.get("GITHUB_EVENT_NAME", "local")
    if re.fullmatch(r"[0-9a-f]{40}", head) is None:
        raise SystemExit("--head-sha must be the exact 40-character lowercase Git SHA")
    if not target or not trigger:
        raise SystemExit("qualification target and trigger must be non-empty")
    started = time.monotonic_ns()
    records: list[dict[str, Any]] = []

    for check in checks:
        check_id = check["id"]
        command = check["command"]
        log_path = output / f"{check_id}.log"
        check_started = time.monotonic_ns()
        missing_tools = [tool for tool in check.get("requiredTools", []) if shutil.which(tool) is None]
        skip_reason = check.get("skipReason")
        if missing_tools or skip_reason:
            reason = skip_reason or f"missing required infrastructure tools: {', '.join(missing_tools)}"
            log_path.write_text(f"infrastructure skip: {reason}\n", encoding="utf-8")
            status, failure_class, returncode = "skipped", "infrastructure_skip", 0
        else:
            try:
                result = subprocess.run(
                    ["bash", "-o", "pipefail", "-c", command],
                    cwd=root,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    check=False,
                )
                log_path.write_text(result.stdout, encoding="utf-8")
                status, failure_class = classify(result.returncode, command)
                returncode = result.returncode
            except OSError as error:
                log_path.write_text(
                    f"orchestrator could not start check: {error}\n", encoding="utf-8"
                )
                status, failure_class, returncode = (
                    "failed",
                    "infrastructure_failure",
                    127,
                )
        measured = (time.monotonic_ns() - check_started) // 1_000_000
        duration = options.fixture_duration_ms if options.fixture_duration_ms is not None else measured
        records.append({
            "id": check_id,
            "command": command,
            "target": target,
            "required": True,
            "status": status,
            "durationMs": duration,
            "failureClass": failure_class,
            "exitCode": returncode,
            "artifacts": [log_path.name],
        })

    failures = [record for record in records if record["status"] == "failed"]
    overall_failure = "none"
    if any(record["failureClass"] == "infrastructure_failure" for record in failures):
        overall_failure = "infrastructure_failure"
    elif failures:
        overall_failure = "product_failure"
    elif any(record["status"] == "skipped" for record in records):
        overall_failure = "infrastructure_skip"
    measured_total = (time.monotonic_ns() - started) // 1_000_000
    total_duration = (options.fixture_duration_ms * len(records)
                      if options.fixture_duration_ms is not None else measured_total)
    evidence_path = output / "toolchain-qualification.json"
    artifact_paths = [record["artifacts"][0] for record in records]
    artifact_paths.append(evidence_path.name)
    evidence = {
        "schema": SCHEMA,
        "trigger": trigger,
        "headSha": head,
        "target": target,
        "status": "failed" if failures else ("skipped" if overall_failure == "infrastructure_skip" else "passed"),
        "durationMs": total_duration,
        "failureClass": overall_failure,
        "artifactPaths": artifact_paths,
        "checks": records,
    }
    encoded = json.dumps(evidence, indent=2, sort_keys=True) + "\n"
    temporary_evidence_path = evidence_path.with_suffix(".json.tmp")
    temporary_evidence_path.write_text(encoded, encoding="utf-8")
    temporary_evidence_path.replace(evidence_path)
    schema_path = root / "stage1/schemas/axiom-toolchain-qualification-v0.schema.json"
    try:
        import jsonschema
    except ImportError as error:
        print(f"toolchain qualification evidence schema validation unavailable: {error}", file=sys.stderr)
        return 1
    try:
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        jsonschema.Draft202012Validator(schema).validate(evidence)
    except (json.JSONDecodeError, jsonschema.ValidationError) as error:
        print(f"toolchain qualification evidence schema validation failed: {error}", file=sys.stderr)
        return 1
    print(evidence_path)
    return 1 if failures or overall_failure == "infrastructure_skip" else 0


if __name__ == "__main__":
    sys.exit(main())
