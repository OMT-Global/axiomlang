#!/usr/bin/env python3
"""Produce exact-head, host-native Target Support v1 evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

from json_schema_v1 import validate_draft_2020_12


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "stage1/schemas/axiom-target-support-evidence-v1.schema.json"
SHA = re.compile(r"[0-9a-f]{40}\Z")
DIGEST = re.compile(r"[0-9a-f]{64}\Z")
TARGETS = {
    "aarch64-apple-darwin": {
        "platform": "macos-arm64",
        "uname_system": "Darwin",
        "uname_machine": "arm64",
        "object_format": "mach-o",
        "abi": "darwin-aarch64",
        "linker": "host-linker",
        "libc": "darwin-libsystem",
        "runtime": "darwin-system-runtime",
        "magic": bytes.fromhex("cffaedfe"),
        "architecture": "arm64",
        "mach_o_cpu_type": 0x0100000C,
        "negative_target": "x86_64-unknown-linux-gnu",
    },
    "x86_64-unknown-linux-gnu": {
        "platform": "linux-x86-64",
        "uname_system": "Linux",
        "uname_machine": "x86_64",
        "object_format": "elf",
        "abi": "sysv-amd64",
        "linker": "host-linker",
        "libc": "glibc-compatible",
        "runtime": "glibc-or-compatible-linux-runtime",
        "magic": bytes.fromhex("7f454c46"),
        "architecture": "x86_64",
        "elf_machine": 62,
        "negative_target": "aarch64-apple-darwin",
    },
}
CHECK_CLAIMS = {
    "debug-build": "locked offline debug compiler binary built on the declared host",
    "doctor-report": "doctor reported the exact supported host and complete target contract",
    "host-identity": "rustc and operating-system identity matched the declared host target",
    "native-smoke": "provider-neutral AxiOM code built for the requested target and its exact artifact ran successfully",
    "proof-workloads": "CLI worker and HTTP proof workloads executed or failed closed without generated Rust",
    "release-build": "locked offline release compiler binary built on the declared host",
    "target-contract": "target catalog schema and fail-closed selection tests passed",
    "unsupported-target": "a non-host target failed with target.unsupported and no host fallback",
}
TOP_LEVEL_FIELDS = {
    "schema_version",
    "evidence_status",
    "head_sha",
    "trigger",
    "expected_target",
    "observed_target",
    "platform",
    "backend",
    "target_selection",
    "offline_replay",
    "network_policy",
    "toolchain",
    "runner_labels",
    "profiles",
    "binary_metadata",
    "checks",
    "qualification",
}


class EvidenceError(ValueError):
    """Raised when evidence is malformed or overclaims qualification."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise EvidenceError(message)


def load_json(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read {path}: {error}") from error
    require(isinstance(payload, dict), f"{path} must contain an object")
    return payload


def validate_schema_metadata(schema: dict[str, Any]) -> None:
    require(
        schema.get("$id")
        == "https://axiom.omt.global/schemas/axiom-target-support-evidence-v1.schema.json",
        "target evidence schema id drift",
    )
    require(schema.get("additionalProperties") is False, "target evidence schema must be closed")
    require(set(schema.get("required", [])) == TOP_LEVEL_FIELDS, "target evidence schema fields drift")
    require(
        schema.get("properties", {}).get("schema_version", {}).get("const")
        == "axiom.target_support_evidence.v1",
        "target evidence schema version drift",
    )


def validate_evidence(payload: dict[str, Any], schema: dict[str, Any]) -> None:
    validate_schema_metadata(schema)
    try:
        validate_draft_2020_12(payload, schema)
    except ValueError as error:
        raise EvidenceError(f"evidence violates published schema: {error}") from error
    require(set(payload) == TOP_LEVEL_FIELDS, "target evidence fields drift")
    require(payload["schema_version"] == "axiom.target_support_evidence.v1", "evidence schema drift")
    require(payload["evidence_status"] in {"failed", "passed"}, "invalid evidence status")
    require(isinstance(payload["head_sha"], str) and SHA.fullmatch(payload["head_sha"]) is not None, "invalid head SHA")
    require(isinstance(payload["trigger"], str) and 0 < len(payload["trigger"]) <= 64, "invalid trigger")
    expected_target = payload["expected_target"]
    require(expected_target in TARGETS, "unsupported expected target")
    target = TARGETS[expected_target]
    require(payload["platform"] == target["platform"], "platform does not match expected target")
    require(payload["backend"] == "cranelift", "target evidence backend drift")
    require(payload["target_selection"] == "exact-host-only", "target selection drift")
    require(payload["offline_replay"] is True, "target evidence must replay offline")
    require(
        payload["network_policy"] == "cargo_offline_registry_network_disabled",
        "target evidence network policy drift",
    )
    toolchain = payload["toolchain"]
    require(isinstance(toolchain, dict), "toolchain identity must be an object")
    require(
        set(toolchain)
        == {
            "rustc_version",
            "rustc_host",
            "rustc_verbose_sha256",
            "cargo_version",
            "cargo_verbose_sha256",
            "cargo_lock_sha256",
            "source_date_epoch",
        },
        "toolchain identity fields drift",
    )
    require(toolchain["rustc_host"] == payload["observed_target"], "toolchain host drift")
    for field in ("rustc_version", "cargo_version"):
        require(
            isinstance(toolchain[field], str) and 0 < len(toolchain[field]) <= 256,
            f"invalid {field}",
        )
    for field in ("rustc_verbose_sha256", "cargo_verbose_sha256", "cargo_lock_sha256"):
        require(
            isinstance(toolchain[field], str) and DIGEST.fullmatch(toolchain[field]) is not None,
            f"invalid {field}",
        )
    require(
        isinstance(toolchain["source_date_epoch"], int)
        and toolchain["source_date_epoch"] > 0,
        "invalid source-date epoch",
    )
    labels = payload["runner_labels"]
    require(isinstance(labels, list) and labels, "runner labels are missing")
    require(all(isinstance(label, str) and 0 < len(label) <= 64 for label in labels), "invalid runner label")
    require(labels == sorted(set(labels)), "runner labels must be sorted and unique")
    require(payload["profiles"] == ["debug", "release"], "profile coverage drift")

    checks = payload["checks"]
    require(isinstance(checks, list), "checks must be an array")
    require([check.get("id") for check in checks] == sorted(CHECK_CLAIMS), "check coverage or ordering drift")
    for check in checks:
        require(isinstance(check, dict), "each check must be an object")
        require(set(check) == {"id", "status", "claim"}, f"{check.get('id')}: check fields drift")
        require(check["status"] in {"failed", "passed", "skipped"}, f"{check['id']}: invalid status")
        require(check["claim"] == CHECK_CLAIMS[check["id"]], f"{check['id']}: claim drift")

    binaries = payload["binary_metadata"]
    require(isinstance(binaries, list), "binary metadata must be an array")
    profiles = [binary.get("profile") for binary in binaries]
    require(profiles == sorted(set(profiles)), "binary profiles must be sorted and unique")
    for binary in binaries:
        require(isinstance(binary, dict), "binary metadata must contain objects")
        require(
            set(binary)
            == {
                "name",
                "profile",
                "target",
                "object_format",
                "architecture",
                "bytes",
                "sha256",
            },
            "binary metadata fields drift",
        )
        require(binary["name"] == "axiomc", "binary identity drift")
        require(binary["profile"] in {"debug", "release"}, "binary profile drift")
        require(binary["target"] == expected_target, "binary target drift")
        require(binary["object_format"] == target["object_format"], "binary object format drift")
        require(binary["architecture"] == target["architecture"], "binary architecture drift")
        require(isinstance(binary["bytes"], int) and binary["bytes"] > 0, "binary size invalid")
        require(
            isinstance(binary["sha256"], str) and DIGEST.fullmatch(binary["sha256"]) is not None,
            "binary digest invalid",
        )

    qualification = payload["qualification"]
    require(
        qualification
        == {
            "status": "partial",
            "host_evidence": payload["evidence_status"] == "passed",
            "cross_compilation": False,
            "proof_workloads": "executed_or_fail_closed",
            "release_qualification": False,
        },
        "qualification boundary drift",
    )
    if payload["evidence_status"] == "passed":
        require(payload["observed_target"] == expected_target, "passed evidence target mismatch")
        require(all(check["status"] == "passed" for check in checks), "passed evidence has non-passing checks")
        require(profiles == ["debug", "release"], "passed evidence requires both binary profiles")
    else:
        require(any(check["status"] == "failed" for check in checks), "failed evidence has no failed check")


def parse_rustc_host(output: str) -> str | None:
    return next(
        (line.removeprefix("host: ") for line in output.splitlines() if line.startswith("host: ")),
        None,
    )


def first_output_line(output: str, label: str) -> str:
    line = next((line.strip() for line in output.splitlines() if line.strip()), "")
    require(bool(line), f"{label} did not report a version")
    return line


def run_command(argv: list[str], root: Path, environment: dict[str, str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        argv,
        cwd=root,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def report_failure(identifier: str, process: subprocess.CompletedProcess[str]) -> None:
    output = (process.stdout + "\n" + process.stderr).strip()
    if len(output) > 4000:
        output = output[-4000:]
    print(
        f"target-support-evidence: {identifier} failed with exit {process.returncode}\n{output}",
        file=sys.stderr,
    )


def command_status(
    identifier: str,
    argv: list[str],
    root: Path,
    environment: dict[str, str],
) -> tuple[dict[str, str], subprocess.CompletedProcess[str]]:
    process = run_command(argv, root, environment)
    status = "passed" if process.returncode == 0 else "failed"
    if status == "failed":
        report_failure(identifier, process)
    return {"id": identifier, "status": status, "claim": CHECK_CLAIMS[identifier]}, process


def checkout_source_date_epoch(
    root: Path,
    expected_head: str,
    environment: dict[str, str],
    *,
    reject_extra_paths: bool = True,
) -> int:
    head = run_command(["git", "rev-parse", "HEAD"], root, environment)
    require(head.returncode == 0, "cannot resolve checkout HEAD")
    actual_head = head.stdout.strip()
    require(actual_head == expected_head, f"checkout HEAD {actual_head} does not match {expected_head}")
    for argv, label in (
        (["git", "diff", "--quiet"], "tracked worktree"),
        (["git", "diff", "--cached", "--quiet"], "staged index"),
    ):
        clean = run_command(argv, root, environment)
        require(clean.returncode == 0, f"{label} differs from checkout HEAD")
    if reject_extra_paths:
        status = run_command(
            [
                "git",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--ignored=matching",
            ],
            root,
            environment,
        )
        require(status.returncode == 0, "cannot inspect checkout inputs")
        require(
            not status.stdout.strip(),
            "checkout contains untracked or ignored inputs",
        )
    timestamp = run_command(
        ["git", "show", "-s", "--format=%ct", expected_head],
        root,
        environment,
    )
    require(timestamp.returncode == 0, "cannot resolve checkout commit timestamp")
    try:
        source_date_epoch = int(timestamp.stdout.strip())
    except ValueError as error:
        raise EvidenceError("checkout commit timestamp is invalid") from error
    require(source_date_epoch > 0, "checkout commit timestamp must be positive")
    return source_date_epoch


def binary_architecture(data: bytes, target: dict[str, Any]) -> str:
    require(data.startswith(target["magic"]), "binary object format mismatch")
    if target["object_format"] == "elf":
        require(len(data) >= 20, "ELF binary header is truncated")
        require(data[5] == 1, "ELF binary is not little-endian")
        machine = int.from_bytes(data[18:20], "little")
        require(machine == target["elf_machine"], "ELF binary architecture mismatch")
    else:
        require(len(data) >= 8, "Mach-O binary header is truncated")
        cpu_type = int.from_bytes(data[4:8], "little")
        require(cpu_type == target["mach_o_cpu_type"], "Mach-O binary architecture mismatch")
    return target["architecture"]


def binary_metadata(
    path: Path,
    profile: str,
    expected_target: str,
    target: dict[str, Any],
) -> dict[str, Any]:
    require(path.is_file() and not path.is_symlink(), "compiler binary must be a regular file")
    data = path.read_bytes()
    architecture = binary_architecture(data, target)
    return {
        "name": "axiomc",
        "profile": profile,
        "target": expected_target,
        "object_format": target["object_format"],
        "architecture": architecture,
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def verify_binary_identity(
    path: Path,
    expected_metadata: dict[str, Any],
    expected_target: str,
    target: dict[str, Any],
) -> None:
    actual = binary_metadata(
        path,
        expected_metadata["profile"],
        expected_target,
        target,
    )
    require(
        actual == expected_metadata,
        f"{expected_metadata['profile']} compiler binary identity drift",
    )


def run_verified_compiler(
    argv: list[str],
    compiler: Path,
    compiler_metadata: dict[str, Any],
    expected_target: str,
    target: dict[str, Any],
    root: Path,
    environment: dict[str, str],
) -> subprocess.CompletedProcess[str]:
    verify_binary_identity(compiler, compiler_metadata, expected_target, target)
    process = run_command(argv, root, environment)
    verify_binary_identity(compiler, compiler_metadata, expected_target, target)
    return process


def path_without_symlinks(root: Path, relative: Path, label: str) -> Path:
    require(not relative.is_absolute(), f"{label} must be checkout-relative")
    cursor = root
    for component in relative.parts:
        require(component not in {"", ".", ".."}, f"{label} is not canonical")
        cursor = cursor / component
        require(not cursor.is_symlink(), f"{label} contains a symlink")
    return cursor


def anchored_project_artifact(
    root: Path,
    project: str,
    reported_path: Any,
    expected_name: str,
) -> Path:
    require(isinstance(reported_path, str) and reported_path, "native artifact path is missing")
    checkout_root = root.resolve(strict=True)
    project_relative = Path(project)
    project_root = path_without_symlinks(
        checkout_root,
        project_relative,
        "native project path",
    )
    require(project_root.is_dir(), "native project root is invalid")
    expected_relative = project_relative / "dist" / expected_name
    expected = path_without_symlinks(
        checkout_root,
        expected_relative,
        "native artifact path",
    )
    candidate = Path(reported_path)
    if candidate.is_absolute():
        candidate_relative = None
        for candidate_root in (root.absolute(), checkout_root):
            try:
                candidate_relative = candidate.relative_to(candidate_root)
                break
            except ValueError:
                continue
        require(candidate_relative is not None, "native artifact escapes checkout root")
    else:
        candidate_relative = candidate
    governed_candidate = path_without_symlinks(
        checkout_root,
        candidate_relative,
        "reported native artifact path",
    )
    require(
        governed_candidate == expected,
        "native artifact path is not the governed project output",
    )
    require(expected.is_file(), "native artifact is not a regular file")
    return expected


def target_dir(root: Path, environment: dict[str, str]) -> Path:
    configured = environment.get("CARGO_TARGET_DIR")
    if configured:
        candidate = Path(configured)
        return candidate if candidate.is_absolute() else root / candidate
    return root / "stage1/target"


def native_smoke_status(
    compiler: Path,
    compiler_metadata: dict[str, Any],
    expected_target: str,
    target: dict[str, Any],
    root: Path,
    environment: dict[str, str],
) -> dict[str, str]:
    project = "stage1/examples/stdlib_collection_lookup"
    commands = {
        "check": [str(compiler), "check", project, "--json"],
        "build": [
            str(compiler),
            "build",
            project,
            "--backend",
            "cranelift",
            "--target",
            expected_target,
            "--locked",
            "--offline",
            "--json",
        ],
    }
    processes = {
        name: run_verified_compiler(
            command,
            compiler,
            compiler_metadata,
            expected_target,
            target,
            root,
            environment,
        )
        for name, command in commands.items()
    }
    valid = all(process.returncode == 0 for process in processes.values())
    reports: dict[str, dict[str, Any]] = {}
    for name in ("check", "build"):
        try:
            report = json.loads(processes[name].stdout)
            if not isinstance(report, dict):
                raise TypeError
            reports[name] = report
        except (json.JSONDecodeError, TypeError):
            valid = False
    if valid:
        valid = (
            reports["check"].get("ok") is True
            and reports["check"].get("command") == "check"
            and reports["build"].get("ok") is True
            and reports["build"].get("command") == "build"
            and reports["build"].get("backend") == "cranelift"
            and reports["build"].get("target") == expected_target
            and reports["build"].get("generated_rust") is None
        )
    if valid:
        binary_value = reports["build"].get("binary")
        try:
            executable = "stdlib-collection-lookup.exe" if os.name == "nt" else "stdlib-collection-lookup"
            binary = anchored_project_artifact(root, project, binary_value, executable)
            binary_before = binary.read_bytes()
            binary_digest = hashlib.sha256(binary_before).hexdigest()
            binary_architecture(binary_before, target)
        except (OSError, TypeError):
            valid = False
        except EvidenceError:
            valid = False
    if valid:
        processes["run-targeted-artifact"] = run_command(
            [str(binary)],
            root,
            environment,
        )
        try:
            valid = (
                processes["run-targeted-artifact"].returncode == 0
                and hashlib.sha256(binary.read_bytes()).hexdigest() == binary_digest
            )
        except OSError:
            valid = False
    if not valid:
        for name, process in processes.items():
            report_failure(f"native-smoke-{name}", process)
    return {
        "id": "native-smoke",
        "status": "passed" if valid else "failed",
        "claim": CHECK_CLAIMS["native-smoke"],
    }


def produce_evidence(
    *,
    root: Path,
    expected_target: str,
    head_sha: str,
    trigger: str,
    runner_labels: list[str],
) -> dict[str, Any]:
    require(expected_target in TARGETS, "unsupported expected target")
    require(SHA.fullmatch(head_sha) is not None, "head SHA must be exact")
    target = TARGETS[expected_target]
    environment = dict(os.environ)
    source_date_epoch = checkout_source_date_epoch(root, head_sha, environment)
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["AXIOM_REGISTRY_NETWORK_DISABLED"] = "1"
    environment["SOURCE_DATE_EPOCH"] = str(source_date_epoch)
    environment.setdefault(
        "CARGO_TARGET_DIR",
        str(Path(environment.get("RUNNER_TEMP", "/tmp")) / f"axiom-target-evidence-{target['platform']}"),
    )
    checks: dict[str, dict[str, str]] = {}
    binaries: list[dict[str, Any]] = []

    rustc = run_command(["rustc", "-vV"], root, environment)
    cargo = run_command(["cargo", "--version", "--verbose"], root, environment)
    uname_system = run_command(["uname", "-s"], root, environment)
    uname_machine = run_command(["uname", "-m"], root, environment)
    require(rustc.returncode == 0, "rustc toolchain identity is unavailable")
    require(cargo.returncode == 0, "cargo toolchain identity is unavailable")
    observed_target = parse_rustc_host(rustc.stdout) if rustc.returncode == 0 else None
    require(observed_target is not None, "rustc did not report a host target")
    cargo_lock = (root / "stage1/Cargo.lock").read_bytes()
    toolchain = {
        "rustc_version": first_output_line(rustc.stdout, "rustc"),
        "rustc_host": observed_target,
        "rustc_verbose_sha256": hashlib.sha256(rustc.stdout.encode("utf-8")).hexdigest(),
        "cargo_version": first_output_line(cargo.stdout, "cargo"),
        "cargo_verbose_sha256": hashlib.sha256(cargo.stdout.encode("utf-8")).hexdigest(),
        "cargo_lock_sha256": hashlib.sha256(cargo_lock).hexdigest(),
        "source_date_epoch": source_date_epoch,
    }
    identity_passed = (
        rustc.returncode == 0
        and uname_system.returncode == 0
        and uname_machine.returncode == 0
        and observed_target == expected_target
        and uname_system.stdout.strip() == target["uname_system"]
        and uname_machine.stdout.strip() == target["uname_machine"]
    )
    checks["host-identity"] = {
        "id": "host-identity",
        "status": "passed" if identity_passed else "failed",
        "claim": CHECK_CLAIMS["host-identity"],
    }
    if not identity_passed:
        print(
            "target-support-evidence: host identity mismatch "
            f"expected={expected_target} observed={observed_target!r} "
            f"uname={uname_system.stdout.strip()!r}/{uname_machine.stdout.strip()!r}",
            file=sys.stderr,
        )
        for identifier in CHECK_CLAIMS:
            if identifier != "host-identity":
                checks[identifier] = {
                    "id": identifier,
                    "status": "skipped",
                    "claim": CHECK_CLAIMS[identifier],
                }
        evidence = {
            "schema_version": "axiom.target_support_evidence.v1",
            "evidence_status": "failed",
            "head_sha": head_sha,
            "trigger": trigger,
            "expected_target": expected_target,
            "observed_target": observed_target,
            "platform": target["platform"],
            "backend": "cranelift",
            "target_selection": "exact-host-only",
            "offline_replay": True,
            "network_policy": "cargo_offline_registry_network_disabled",
            "toolchain": toolchain,
            "runner_labels": sorted(set(runner_labels)),
            "profiles": ["debug", "release"],
            "binary_metadata": [],
            "checks": [checks[identifier] for identifier in sorted(CHECK_CLAIMS)],
            "qualification": {
                "status": "partial",
                "host_evidence": False,
                "cross_compilation": False,
                "proof_workloads": "executed_or_fail_closed",
                "release_qualification": False,
            },
        }
        validate_evidence(evidence, load_json(SCHEMA))
        return evidence

    checks["debug-build"], _ = command_status(
        "debug-build",
        [
            "cargo",
            "build",
            "--locked",
            "--offline",
            "--manifest-path",
            "stage1/Cargo.toml",
            "-p",
            "axiomc",
            "--target",
            expected_target,
        ],
        root,
        environment,
    )
    checks["release-build"], _ = command_status(
        "release-build",
        [
            "cargo",
            "build",
            "--locked",
            "--offline",
            "--manifest-path",
            "stage1/Cargo.toml",
            "-p",
            "axiomc",
            "--target",
            expected_target,
            "--release",
        ],
        root,
        environment,
    )
    executable = "axiomc.exe" if os.name == "nt" else "axiomc"
    build_root = target_dir(root, environment) / expected_target
    for profile in ("debug", "release"):
        binary = build_root / profile / executable
        if binary.is_file():
            try:
                binaries.append(binary_metadata(binary, profile, expected_target, target))
            except (OSError, EvidenceError) as error:
                checks[f"{profile}-build"]["status"] = "failed"
                print(f"target-support-evidence: {error}", file=sys.stderr)
    binary_by_profile = {item["profile"]: item for item in binaries}

    checks["target-contract"], _ = command_status(
        "target-contract",
        [
            "cargo",
            "test",
            "--locked",
            "--offline",
            "--manifest-path",
            "stage1/Cargo.toml",
            "-p",
            "axiomc",
            "--target",
            expected_target,
            "--lib",
            "target_support::tests",
        ],
        root,
        environment,
    )

    doctor_binary = build_root / "debug" / executable
    debug_metadata = binary_by_profile.get("debug")
    if doctor_binary.is_file() and debug_metadata is not None:
        doctor = run_verified_compiler(
            [str(doctor_binary), "doctor", "stage1/examples/proof_cli", "--json"],
            doctor_binary,
            debug_metadata,
            expected_target,
            target,
            root,
            environment,
        )
        doctor_ok = False
        if doctor.returncode == 0:
            try:
                payload = json.loads(doctor.stdout)
                support = payload["target_support"]
                row = next(
                    item for item in support["supported_targets"] if item["target"] == expected_target
                )
                doctor_ok = (
                    payload.get("ok") is True
                    and payload.get("target_triple") == expected_target
                    and support.get("host_target") == expected_target
                    and support.get("host_supported") is True
                    and support.get("target_selection") == "exact-host-only"
                    and row.get("platform") == target["platform"]
                    and row.get("object_format") == target["object_format"]
                    and row.get("abi") == target["abi"]
                    and row.get("linker") == target["linker"]
                    and row.get("libc") == target["libc"]
                    and row.get("runtime") == target["runtime"]
                    and row.get("profiles") == ["debug", "release"]
                    and row.get("provider_policy") == "capability-providers-qualified-separately"
                    and row.get("unsupported_features")
                    == ["cross-compilation", "direct-native-wasm", "windows-native"]
                    and row.get("status") == "supported-host-only"
                )
            except (json.JSONDecodeError, KeyError, StopIteration, TypeError):
                doctor_ok = False
        checks["doctor-report"] = {
            "id": "doctor-report",
            "status": "passed" if doctor_ok else "failed",
            "claim": CHECK_CLAIMS["doctor-report"],
        }
        if not doctor_ok:
            report_failure("doctor-report", doctor)
    else:
        checks["doctor-report"] = {
            "id": "doctor-report",
            "status": "failed",
            "claim": CHECK_CLAIMS["doctor-report"],
        }

    if doctor_binary.is_file() and debug_metadata is not None:
        checks["native-smoke"] = native_smoke_status(
            doctor_binary,
            debug_metadata,
            expected_target,
            target,
            root,
            environment,
        )
    else:
        checks["native-smoke"] = {
            "id": "native-smoke",
            "status": "failed",
            "claim": CHECK_CLAIMS["native-smoke"],
        }
    checks["proof-workloads"], _ = command_status(
        "proof-workloads",
        ["bash", "scripts/ci/run-stage1-proof-test.sh"],
        root,
        environment,
    )

    if doctor_binary.is_file() and debug_metadata is not None:
        negative = run_verified_compiler(
            [
                str(doctor_binary),
                "build",
                "stage1/examples/proof_cli",
                "--backend",
                "cranelift",
                "--target",
                target["negative_target"],
                "--json",
            ],
            doctor_binary,
            debug_metadata,
            expected_target,
            target,
            root,
            environment,
        )
        negative_ok = False
        if negative.returncode != 0:
            try:
                payload = json.loads(negative.stdout)
                negative_ok = (
                    payload.get("ok") is False
                    and payload.get("error", {}).get("code") == "target.unsupported"
                    and payload.get("command") == "build"
                )
            except (json.JSONDecodeError, TypeError):
                negative_ok = False
        checks["unsupported-target"] = {
            "id": "unsupported-target",
            "status": "passed" if negative_ok else "failed",
            "claim": CHECK_CLAIMS["unsupported-target"],
        }
        if not negative_ok:
            report_failure("unsupported-target", negative)
    else:
        checks["unsupported-target"] = {
            "id": "unsupported-target",
            "status": "failed",
            "claim": CHECK_CLAIMS["unsupported-target"],
        }

    final_source_date_epoch = checkout_source_date_epoch(
        root,
        head_sha,
        environment,
        reject_extra_paths=False,
    )
    require(final_source_date_epoch == source_date_epoch, "checkout timestamp drifted during evidence")
    final_lock_digest = hashlib.sha256((root / "stage1/Cargo.lock").read_bytes()).hexdigest()
    require(final_lock_digest == toolchain["cargo_lock_sha256"], "Cargo.lock drifted during evidence")
    for metadata in binaries:
        binary = build_root / metadata["profile"] / executable
        verify_binary_identity(binary, metadata, expected_target, target)

    ordered_checks = [checks[identifier] for identifier in sorted(CHECK_CLAIMS)]
    passed = all(check["status"] == "passed" for check in ordered_checks)
    evidence = {
        "schema_version": "axiom.target_support_evidence.v1",
        "evidence_status": "passed" if passed else "failed",
        "head_sha": head_sha,
        "trigger": trigger,
        "expected_target": expected_target,
        "observed_target": observed_target,
        "platform": target["platform"],
        "backend": "cranelift",
        "target_selection": "exact-host-only",
        "offline_replay": True,
        "network_policy": "cargo_offline_registry_network_disabled",
        "toolchain": toolchain,
        "runner_labels": sorted(set(runner_labels)),
        "profiles": ["debug", "release"],
        "binary_metadata": sorted(binaries, key=lambda item: item["profile"]),
        "checks": ordered_checks,
        "qualification": {
            "status": "partial",
            "host_evidence": passed,
            "cross_compilation": False,
            "proof_workloads": "executed_or_fail_closed",
            "release_qualification": False,
        },
    }
    validate_evidence(evidence, load_json(SCHEMA))
    return evidence


def write_evidence(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    run = subparsers.add_parser("run")
    run.add_argument("--repo-root", type=Path, default=ROOT)
    run.add_argument("--expected-target", choices=sorted(TARGETS), required=True)
    run.add_argument("--head-sha", required=True)
    run.add_argument("--trigger", required=True)
    run.add_argument("--runner-label", action="append", default=[])
    run.add_argument("--runner-labels-json")
    run.add_argument("--output", type=Path, required=True)
    validate = subparsers.add_parser("validate")
    validate.add_argument("--evidence", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    options = parse_args(argv)
    try:
        schema = load_json(SCHEMA)
        if options.command == "validate":
            validate_evidence(load_json(options.evidence), schema)
            print("target-support-evidence-v1: valid")
            return 0
        labels = list(options.runner_label)
        if options.runner_labels_json:
            try:
                decoded_labels = json.loads(options.runner_labels_json)
            except json.JSONDecodeError as error:
                raise EvidenceError(f"runner labels must be valid JSON: {error}") from error
            require(
                isinstance(decoded_labels, list)
                and all(isinstance(label, str) and label for label in decoded_labels),
                "runner labels JSON must be an array of non-empty strings",
            )
            labels.extend(decoded_labels)
        if not labels:
            labels = ["local"]
        evidence = produce_evidence(
            root=options.repo_root.resolve(),
            expected_target=options.expected_target,
            head_sha=options.head_sha,
            trigger=options.trigger,
            runner_labels=labels,
        )
        validate_evidence(evidence, schema)
        write_evidence(options.output, evidence)
    except (EvidenceError, OSError, subprocess.SubprocessError) as error:
        print(f"target-support-evidence-v1: {error}", file=sys.stderr)
        return 1
    print(
        f"target-support-evidence-v1: {evidence['evidence_status']} "
        f"target={evidence['observed_target']} output={options.output}"
    )
    return 0 if evidence["evidence_status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
