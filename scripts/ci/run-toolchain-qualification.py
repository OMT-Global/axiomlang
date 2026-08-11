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
SUMMARY_ARTIFACT = "toolchain-qualification-summary.txt"

DEFAULT_CHECKS = [
    {"id": "full_crate_integration", "command": "RUST_MIN_STACK=8388608 cargo test --manifest-path stage1/Cargo.toml --workspace --all-targets --features run-native-tests --locked -- --test-threads=1"},
    {"id": "conformance", "command": "bash scripts/ci/run-stage1-conformance.sh"},
    {"id": "build_purity", "command": "bash scripts/ci/run-extended-stage1-checks.sh"},
    {"id": "proof_smoke", "command": "bash scripts/ci/run-stage1-proof-test.sh && bash scripts/ci/run-stage1-basic-smoke.sh && bash scripts/ci/run-stage1-stdlib-smoke.sh"},
    {"id": "schemas_protocol", "command": "cargo test --manifest-path stage1/Cargo.toml -p axiomc --test schema_metadata --test json_command_fixtures --test json_contract_snapshots --locked && bash scripts/ci/validate-capability-manifests.sh"},
    {"id": "lsp_protocol_smoke", "command": "cargo test --manifest-path stage1/Cargo.toml -p axiomc --lib --test lsp_stdio --locked lsp -- --test-threads=1 && python3 scripts/ci/check-command-lsp-boundary.py"},
    {"id": "direct_native_abi", "command": "CARGO_TARGET_DIR=stage1/target/direct-native-runtime-abi bash scripts/ci/run-direct-native-runtime-abi-evidence.sh"},
    {"id": "runtime_sensitivity", "command": "cargo test --manifest-path stage1/Cargo.toml -p axiomc --test cranelift_backend --locked -- --test-threads=1"},
    {
        "id": "benchmark_comparison",
        "command": "python3 scripts/ci/check-stage1-benchmarks.py && python3 scripts/ci/report-stage1-reference-comparison.py",
        "requiredTools": ["go"],
    },
    {
        "id": "stage1_quality_gate",
        "command": "python3 scripts/ci/run-stage1-quality-gate.py --expected-head \"$AXIOM_QUALIFICATION_HEAD_SHA\" --lcov-output .axiom-build/reports/stage1-coverage.lcov --output .axiom-build/reports/stage1-quality-report.json",
        "requiredTools": ["cargo-llvm-cov"],
        "artifactPaths": [
            ".axiom-build/reports/stage1-coverage.lcov",
            ".axiom-build/reports/stage1-quality-report.json",
        ],
    },
    {
        "id": "mutation_quality_smoke",
        "command": "python3 scripts/ci/run-mutation-rust-smoke.py --fail-on-survivors --per-mutant-budget-seconds 90 --total-budget-seconds 300 --expected-head \"$AXIOM_QUALIFICATION_HEAD_SHA\" --output .axiom-build/reports/mutation-rust-smoke.json",
        "artifactPaths": [".axiom-build/reports/mutation-rust-smoke.json"],
    },
    {"id": "supply_chain", "command": "bash scripts/ci/run-toolchain-supply-chain.sh", "requiredTools": ["cargo-vet"]},
    {"id": "readiness_self_tests", "command": "bash scripts/ci/test-check-production-language-readiness.sh && bash scripts/ci/test-check-self-hosting-language-readiness.sh && bash scripts/ci/test-check-snapshot-bootstrap-readiness.sh && bash scripts/ci/test-check-python-exit-readiness.sh && bash scripts/ci/test-check-rust-exit-readiness.sh && python3 scripts/ci/check-production-language-readiness.py --validate-only"},
    {
        "id": "readiness_gates",
        "command": "python3 scripts/ci/run-readiness-gates.py --output-dir artifacts/readiness --head-sha \"$AXIOM_QUALIFICATION_HEAD_SHA\" --require-issue-states",
        "artifactPaths": [
            "artifacts/readiness/readiness-gates.json",
            "artifacts/readiness/rust-exit-readiness.json",
            "artifacts/readiness/self-hosting-language-readiness.json",
            "artifacts/readiness/snapshot-bootstrap-readiness.json",
            "artifacts/readiness/native-build-purity.log",
            "artifacts/readiness/self-hosting-spike-parity.log",
        ],
    },
]


def args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--plan", type=Path, default=None,
                        help="JSON check plan for hermetic orchestrator tests")
    parser.add_argument("--head-sha", default=None)
    parser.add_argument(
        "--base-sha",
        default=os.environ.get("AXIOM_QUALIFICATION_BASE_SHA") or None,
        help=(
            "optional exact lowercase comparison commit; defaults to "
            "AXIOM_QUALIFICATION_BASE_SHA"
        ),
    )
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
        artifact_paths = check.get("artifactPaths", [])
        if not isinstance(required_tools, list) or not all(isinstance(tool, str) for tool in required_tools):
            raise ValueError("requiredTools must be an array of tool names")
        if skip_reason is not None and not isinstance(skip_reason, str):
            raise ValueError("skipReason must be a string")
        if not isinstance(artifact_paths, list) or not all(
            isinstance(path, str) and path for path in artifact_paths
        ):
            raise ValueError("artifactPaths must be an array of non-empty paths")
        for path in artifact_paths:
            candidate = Path(path)
            if candidate.is_absolute() or ".." in candidate.parts:
                raise ValueError("artifactPaths must be repo-relative without parent traversal")
        result.append({"id": check["id"], "command": check["command"],
                       "requiredTools": required_tools, "skipReason": skip_reason,
                       "artifactPaths": artifact_paths})
    return result


def _exact_keys(value: Any, expected: set[str], location: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError(f"{location} must be an object")
    actual = set(value)
    if actual != expected:
        missing = sorted(expected - actual)
        unknown = sorted(actual - expected)
        raise ValueError(
            f"{location} keys differ; missing={missing}, unknown={unknown}"
        )
    return value


def _nonempty_string(value: Any, location: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{location} must be a non-empty string")
    return value


def _nonnegative_int(value: Any, location: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"{location} must be a nonnegative integer")
    return value


def validate_qualification_evidence(
    evidence: Any, schema_path: Path
) -> dict[str, Any]:
    try:
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read qualification schema: {error}") from error
    if (
        schema.get("properties", {}).get("schema", {}).get("const") != SCHEMA
        or schema.get("additionalProperties") is not False
        or schema.get("properties", {})
        .get("checks", {})
        .get("items", {})
        .get("additionalProperties")
        is not False
    ):
        raise ValueError(
            "qualification schema must pin its version and reject unknown fields"
        )

    root = _exact_keys(
        evidence,
        {
            "schema",
            "trigger",
            "headSha",
            "target",
            "status",
            "durationMs",
            "failureClass",
            "artifactPaths",
            "checks",
        },
        "evidence",
    )
    if root["schema"] != SCHEMA:
        raise ValueError(f"evidence.schema must be {SCHEMA}")
    _nonempty_string(root["trigger"], "evidence.trigger")
    head = _nonempty_string(root["headSha"], "evidence.headSha")
    if re.fullmatch(r"[0-9a-f]{40}", head) is None:
        raise ValueError("evidence.headSha must be an exact lowercase commit")
    _nonempty_string(root["target"], "evidence.target")
    if root["status"] not in {"passed", "failed", "skipped"}:
        raise ValueError("evidence.status is invalid")
    _nonnegative_int(root["durationMs"], "evidence.durationMs")
    if root["failureClass"] not in {
        "none",
        "product_failure",
        "infrastructure_failure",
        "infrastructure_skip",
    }:
        raise ValueError("evidence.failureClass is invalid")
    artifact_paths = root["artifactPaths"]
    if (
        not isinstance(artifact_paths, list)
        or not artifact_paths
        or not all(isinstance(item, str) and item for item in artifact_paths)
    ):
        raise ValueError(
            "evidence.artifactPaths must be a non-empty string array"
        )
    checks = root["checks"]
    if not isinstance(checks, list) or not checks:
        raise ValueError("evidence.checks must be a non-empty array")
    check_statuses: list[str] = []
    for index, raw_check in enumerate(checks):
        location = f"evidence.checks[{index}]"
        check = _exact_keys(
            raw_check,
            {
                "id",
                "command",
                "target",
                "required",
                "status",
                "durationMs",
                "failureClass",
                "exitCode",
                "artifacts",
            },
            location,
        )
        check_id = _nonempty_string(check["id"], f"{location}.id")
        if re.fullmatch(r"[a-z][a-z0-9_]*", check_id) is None:
            raise ValueError(f"{location}.id is invalid")
        _nonempty_string(check["command"], f"{location}.command")
        _nonempty_string(check["target"], f"{location}.target")
        if check["required"] is not True:
            raise ValueError(f"{location}.required must be true")
        if check["status"] not in {"passed", "failed", "skipped"}:
            raise ValueError(f"{location}.status is invalid")
        check_statuses.append(check["status"])
        _nonnegative_int(check["durationMs"], f"{location}.durationMs")
        if check["failureClass"] not in {
            "none",
            "product_failure",
            "infrastructure_failure",
            "infrastructure_skip",
        }:
            raise ValueError(f"{location}.failureClass is invalid")
        if isinstance(check["exitCode"], bool) or not isinstance(
            check["exitCode"], int
        ):
            raise ValueError(f"{location}.exitCode must be an integer")
        artifacts = check["artifacts"]
        if (
            not isinstance(artifacts, list)
            or not artifacts
            or not all(isinstance(item, str) and item for item in artifacts)
        ):
            raise ValueError(f"{location}.artifacts must be a non-empty string array")

    if root["status"] == "passed":
        if root["failureClass"] != "none" or any(
            status != "passed" for status in check_statuses
        ):
            raise ValueError("passing evidence cannot contain blockers")
    elif root["status"] == "failed":
        if root["failureClass"] not in {
            "product_failure",
            "infrastructure_failure",
        } or "failed" not in check_statuses:
            raise ValueError("failed evidence must identify a failed check")
    elif (
        root["failureClass"] != "infrastructure_skip"
        or "skipped" not in check_statuses
        or "failed" in check_statuses
    ):
        raise ValueError("skipped evidence must identify an infrastructure skip")
    return root


def classify(returncode: int, command: str) -> tuple[str, str]:
    if returncode == 0:
        return "passed", "none"
    if returncode in (126, 127):
        return "failed", "infrastructure_failure"
    return "failed", "product_failure"


def render_summary(
    *,
    records: list[dict[str, Any]],
    status: str,
    failure_class: str,
    evidence_name: str,
    head: str,
) -> str:
    counts = {
        check_status: sum(record["status"] == check_status for record in records)
        for check_status in ("passed", "skipped", "failed")
    }
    if status == "failed":
        result = "valid_red"
    elif status == "skipped":
        result = "skipped"
    else:
        result = "passed"
    lines = [
        (
            "qualification summary: "
            f"result={result} status={status} failureClass={failure_class} "
            f"passed={counts['passed']} skipped={counts['skipped']} "
            f"failed={counts['failed']} evidenceArtifact={evidence_name} headSha={head}"
        )
    ]
    for record in records:
        if record["status"] != "failed":
            continue
        lines.append(
            "FAILED "
            f"id={record['id']} status={record['status']} "
            f"failureClass={record['failureClass']} exitCode={record['exitCode']} "
            f"artifactPath={record['artifacts'][0]}"
        )
    return "\n".join(lines) + "\n"


def render_harness_summary(*, evidence_name: str, head: str) -> str:
    return (
        "qualification summary: "
        "result=harness_failure failureClass=schema_failure "
        f"evidenceArtifact={evidence_name} headSha={head}\n"
    )


def artifact_identity(path: Path, root: Path) -> str:
    try:
        return path.relative_to(root).as_posix()
    except ValueError:
        return path.name


def file_fingerprint(path: Path) -> tuple[int, int, int, int] | None:
    if not path.is_file():
        return None
    metadata = path.stat()
    return (
        metadata.st_ino,
        metadata.st_size,
        metadata.st_mtime_ns,
        metadata.st_ctime_ns,
    )


def main() -> int:
    options = args()
    root = options.repo_root.resolve()
    output = options.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if options.fixture_duration_ms is not None and options.plan is None:
        raise SystemExit("--fixture-duration-ms requires --plan")

    checks = load_plan(options.plan)
    reserved_artifact_names = {
        "toolchain-qualification.json",
        SUMMARY_ARTIFACT,
        *(f"{check['id']}.log" for check in checks),
    }
    artifact_destinations: dict[str, str] = {}
    for check in checks:
        for relative_path in check.get("artifactPaths", []):
            source = (root / relative_path).resolve()
            if not source.is_relative_to(root):
                raise SystemExit(
                    "artifactPaths must resolve within the qualification repo root"
                )
            destination_name = source.name
            if destination_name in reserved_artifact_names:
                raise SystemExit(
                    f"declared artifact {relative_path} collides with qualification output "
                    f"{destination_name}"
                )
            previous = artifact_destinations.setdefault(destination_name, relative_path)
            if previous != relative_path:
                raise SystemExit(
                    f"declared artifacts {previous} and {relative_path} share output name "
                    f"{destination_name}"
                )
    head = options.head_sha or os.environ.get("GITHUB_SHA") or git_head(root)
    base = options.base_sha
    target = options.target or os.environ.get("AXIOM_QUALIFICATION_TARGET") or host_target()
    trigger = options.trigger or os.environ.get("AXIOM_QUALIFICATION_TRIGGER") or os.environ.get("GITHUB_EVENT_NAME", "local")
    if re.fullmatch(r"[0-9a-f]{40}", head) is None:
        raise SystemExit("--head-sha must be the exact 40-character lowercase Git SHA")
    if base is not None and re.fullmatch(r"[0-9a-f]{40}", base) is None:
        raise SystemExit("--base-sha must be the exact 40-character lowercase Git SHA")
    if not target or not trigger:
        raise SystemExit("qualification target and trigger must be non-empty")
    if options.plan is None:
        actual_head = git_head(root)
        if actual_head != head:
            raise SystemExit(
                f"qualification head {head} does not match checkout HEAD {actual_head}"
            )
        dirty = subprocess.run(
            ["git", "status", "--porcelain=v1", "--untracked-files=no"],
            cwd=root,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        if dirty.returncode != 0:
            raise SystemExit(
                f"cannot inspect qualification checkout state: {dirty.stderr.strip()}"
            )
        if dirty.stdout.strip():
            raise SystemExit("qualification checkout has tracked changes")
    check_environment = os.environ.copy()
    check_environment["AXIOM_QUALIFICATION_HEAD_SHA"] = head
    check_environment["AXIOM_QUALIFICATION_BASE_SHA"] = base or ""
    started = time.monotonic_ns()
    records: list[dict[str, Any]] = []

    for check in checks:
        check_id = check["id"]
        command = check["command"]
        log_path = output / f"{check_id}.log"
        check_artifacts = [log_path.name]
        artifact_before: dict[str, tuple[int, int, int, int] | None] = {}
        for relative_path in check.get("artifactPaths", []):
            source = (root / relative_path).resolve()
            artifact_before[relative_path] = file_fingerprint(source)
            destination = output / source.name
            if destination != source and (
                destination.is_file() or destination.is_symlink()
            ):
                destination.unlink()
            elif destination != source and destination.exists():
                raise SystemExit(
                    f"artifact destination {destination} is not a regular file"
                )
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
                    env=check_environment,
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
        missing_artifacts: list[str] = []
        stale_artifacts: list[str] = []
        artifact_errors: list[str] = []
        for relative_path in check.get("artifactPaths", []):
            source = (root / relative_path).resolve()
            destination = output / source.name
            if not source.is_relative_to(root):
                artifact_errors.append(
                    f"{relative_path} resolved outside the qualification repo root"
                )
            elif not source.is_file():
                missing_artifacts.append(relative_path)
            elif (
                artifact_before[relative_path] is not None
                and file_fingerprint(source) == artifact_before[relative_path]
            ):
                stale_artifacts.append(relative_path)
            elif destination.is_symlink():
                artifact_errors.append(
                    f"{destination.name} is a symbolic link in the output directory"
                )
            else:
                try:
                    if source != destination.resolve():
                        shutil.copy2(source, destination)
                    check_artifacts.append(destination.name)
                except OSError as error:
                    artifact_errors.append(f"{relative_path}: {error}")
        if missing_artifacts or stale_artifacts or artifact_errors:
            with log_path.open("a", encoding="utf-8") as log:
                if missing_artifacts:
                    log.write(
                        "missing declared artifacts: "
                        + ", ".join(missing_artifacts)
                        + "\n"
                    )
                if stale_artifacts:
                    log.write(
                        "stale declared artifacts not regenerated: "
                        + ", ".join(stale_artifacts)
                        + "\n"
                    )
                if artifact_errors:
                    log.write(
                        "artifact collection failed: "
                        + "; ".join(artifact_errors)
                        + "\n"
                    )
            if artifact_errors:
                status, failure_class, returncode = (
                    "failed",
                    "infrastructure_failure",
                    127,
                )
            elif status == "passed":
                status, failure_class, returncode = "failed", "product_failure", 1
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
            "artifacts": check_artifacts,
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
    artifact_paths = [
        artifact for record in records for artifact in record["artifacts"]
    ]
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
    summary_path = output / SUMMARY_ARTIFACT
    evidence_identity = artifact_identity(evidence_path, root)
    summary = render_summary(
        records=records,
        status=evidence["status"],
        failure_class=overall_failure,
        evidence_name=evidence_identity,
        head=head,
    )
    evidence["artifactPaths"].insert(-1, summary_path.name)
    summary_path.write_text(summary, encoding="utf-8")
    schema_path = root / "stage1/schemas/axiom-toolchain-qualification-v0.schema.json"
    try:
        validate_qualification_evidence(evidence, schema_path)
    except ValueError:
        harness_summary = render_harness_summary(
            evidence_name=evidence_identity,
            head=head,
        )
        summary_path.write_text(harness_summary, encoding="utf-8")
        print(
            "toolchain qualification harness failure: schema validation failed",
            file=sys.stderr,
        )
        print(harness_summary, end="")
        return 1
    encoded = json.dumps(evidence, indent=2, sort_keys=True) + "\n"
    temporary_evidence_path = evidence_path.with_suffix(".json.tmp")
    temporary_evidence_path.write_text(encoded, encoding="utf-8")
    temporary_evidence_path.replace(evidence_path)
    print(summary, end="")
    return 1 if failures or overall_failure == "infrastructure_skip" else 0


if __name__ == "__main__":
    sys.exit(main())
