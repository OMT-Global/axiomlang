#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
AXIOMC_MANIFEST = REPO_ROOT / "stage1/Cargo.toml"
AXIOMC_BIN = REPO_ROOT / "stage1/target/debug/axiomc"
REF_ROOT = REPO_ROOT / "stage1/benchmarks/reference"


@dataclass(frozen=True)
class Workload:
    name: str
    kind: str
    project: Path
    reference: Path


WORKLOADS = [
    Workload("hello", "compute", REPO_ROOT / "stage1/examples/hello", REF_ROOT / "hello"),
    Workload("capabilities", "io", REPO_ROOT / "stage1/examples/capabilities", REF_ROOT / "capabilities"),
    Workload("stdlib_async", "concurrency", REPO_ROOT / "stage1/examples/stdlib_async", REF_ROOT / "stdlib_async"),
]


def run_timed(cmd: list[str], *, cwd: Path | None = None) -> tuple[float, subprocess.CompletedProcess[str]]:
    started = time.perf_counter()
    completed = subprocess.run(
        cmd,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    return elapsed_ms, completed


def run_required(cmd: list[str], *, cwd: Path | None = None) -> tuple[float, str]:
    elapsed_ms, completed = run_timed(cmd, cwd=cwd)
    if completed.returncode != 0:
        if completed.stdout:
            sys.stdout.write(completed.stdout)
        if completed.stderr:
            sys.stderr.write(completed.stderr)
        raise RuntimeError(f"command failed: {' '.join(cmd)}")
    return elapsed_ms, completed.stdout


def ensure_tools() -> list[str]:
    required = ["cargo", "rustc", "go"]
    return [tool for tool in required if shutil.which(tool) is None]


def parse_json(text: str) -> dict[str, Any]:
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return {}


def parse_json_result(text: str) -> tuple[bool, dict[str, Any]]:
    try:
        payload = json.loads(text)
    except json.JSONDecodeError:
        return False, {}
    if not isinstance(payload, dict):
        return False, {}
    return True, payload


def file_size(path: Path) -> int | None:
    if path.exists():
        return path.stat().st_size
    return None


def diagnostics_quality(payload: dict[str, Any], returncode: int) -> dict[str, Any]:
    diagnostics = payload.get("diagnostics", [])
    return {
        "json": bool(payload),
        "schema_version": payload.get("schema_version"),
        "ok": payload.get("ok"),
        "returncode": returncode,
        "diagnostic_count": len(diagnostics) if isinstance(diagnostics, list) else None,
        "has_stable_codes": all("code" in diagnostic for diagnostic in diagnostics)
        if isinstance(diagnostics, list)
        else False,
    }


def capability_manifest_coverage(payload: dict[str, Any], *, json_valid: bool) -> dict[str, Any]:
    capabilities = payload.get("capabilities", {})
    if isinstance(capabilities, list):
        declared = [entry.get("name") for entry in capabilities if isinstance(entry, dict) and entry.get("name")]
        enabled = [
            entry.get("name")
            for entry in capabilities
            if isinstance(entry, dict) and entry.get("name") and entry.get("enabled")
        ]
        return {
            "json": json_valid,
            "declared": sorted(declared),
            "enabled": sorted(enabled),
            "enabled_count": len(enabled),
            "declared_count": len(declared),
        }
    if not isinstance(capabilities, dict):
        return {"json": json_valid, "declared": [], "enabled": []}
    enabled = [name for name, value in capabilities.items() if value]
    return {
        "json": json_valid,
        "declared": sorted(capabilities),
        "enabled": sorted(enabled),
        "enabled_count": len(enabled),
        "declared_count": len(capabilities),
    }


def output_matches(left: subprocess.CompletedProcess[str], right: subprocess.CompletedProcess[str]) -> bool:
    return left.returncode == right.returncode and left.stdout == right.stdout


def build_axiomc() -> None:
    subprocess.run(
        ["cargo", "build", "--manifest-path", str(AXIOMC_MANIFEST), "-p", "axiomc"],
        cwd=REPO_ROOT,
        check=True,
    )


def compare_workload(workload: Workload, temp_dir: Path) -> dict[str, Any]:
    shutil.rmtree(workload.project / "dist", ignore_errors=True)

    axiom_build_ms, axiom_build_stdout = run_required(
        [str(AXIOMC_BIN), "build", str(workload.project), "--json"],
        cwd=REPO_ROOT,
    )
    axiom_build = parse_json(axiom_build_stdout)
    axiom_binary = Path(axiom_build.get("binary", ""))
    if not axiom_binary.is_absolute():
        axiom_binary = REPO_ROOT / axiom_binary
    axiom_run_ms, axiom_run = run_timed([str(axiom_binary)], cwd=REPO_ROOT)

    go_binary = temp_dir / f"{workload.name}-go"
    go_build_ms, _ = run_required(
        ["go", "build", "-o", str(go_binary), str(workload.reference / "main.go")],
        cwd=temp_dir,
    )
    go_run_ms, go_run = run_timed([str(go_binary)], cwd=temp_dir)

    rust_binary = temp_dir / f"{workload.name}-rust"
    rust_build_ms, _ = run_required(
        ["rustc", str(workload.reference / "main.rs"), "-O", "-o", str(rust_binary)],
        cwd=temp_dir,
    )
    rust_run_ms, rust_run = run_timed([str(rust_binary)], cwd=temp_dir)

    check_ms, check = run_timed([str(AXIOMC_BIN), "check", str(workload.project), "--json"], cwd=REPO_ROOT)
    caps_ms, caps = run_timed([str(AXIOMC_BIN), "caps", str(workload.project), "--json"], cwd=REPO_ROOT)
    caps_json_valid, caps_payload = parse_json_result(caps.stdout)

    return {
        "kind": workload.kind,
        "build_ms": {
            "axiom": axiom_build_ms,
            "go": go_build_ms,
            "rust": rust_build_ms,
        },
        "run_ms": {
            "axiom": axiom_run_ms,
            "go": go_run_ms,
            "rust": rust_run_ms,
        },
        "binary_size_bytes": {
            "axiom": file_size(axiom_binary),
            "go": file_size(go_binary),
            "rust": file_size(rust_binary),
        },
        "diagnostics_quality": {
            "elapsed_ms": check_ms,
            **diagnostics_quality(parse_json(check.stdout), check.returncode),
        },
        "capability_manifest_coverage": {
            "elapsed_ms": caps_ms,
            "returncode": caps.returncode,
            **capability_manifest_coverage(caps_payload, json_valid=caps_json_valid),
        },
        "stdout_match": {
            "axiom_go": output_matches(axiom_run, go_run),
            "axiom_rust": output_matches(axiom_run, rust_run),
        },
    }


def main() -> int:
    os.chdir(REPO_ROOT)
    missing = ensure_tools()
    if missing:
        print(json.dumps({"blocking": False, "skipped": True, "missing_tools": missing}, indent=2))
        return 0

    try:
        build_axiomc()
        with tempfile.TemporaryDirectory(prefix="axiom-stage1-compare-") as temp_name:
            temp_dir = Path(temp_name)
            workloads = {workload.name: compare_workload(workload, temp_dir) for workload in WORKLOADS}
        report = {
            "schema_version": "axiom.stage1.reference-comparison.v1",
            "blocking": False,
            "workloads": workloads,
        }
    except Exception as error:  # non-blocking calibration signal
        report = {
            "schema_version": "axiom.stage1.reference-comparison.v1",
            "blocking": False,
            "error": str(error),
        }

    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
