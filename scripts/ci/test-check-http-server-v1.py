#!/usr/bin/env python3
"""Hermetic regressions for the HTTP server v1 evidence checker."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-http-server-v1.py"


def run(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(CHECKER), "--root", str(root), "--json"],
        cwd=root,
        text=True,
        capture_output=True,
        check=False,
    )


def write(path: Path, value: dict[str, object]) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def expect_failure(root: Path, message: str) -> None:
    result = run(root)
    if result.returncode == 0:
        raise SystemExit(message)


def copy_contract_root(root: Path) -> None:
    paths = [
        Path("stage1/compiler-contracts/schemas/axiom.runtime_http_server.v1.schema.json"),
        Path("stage1/compiler-contracts/snapshots/http-server-v1.json"),
        Path("stage1/crates/axiomc/src/codegen.rs"),
        Path("stage1/crates/axiomc/src/stdlib.rs"),
        Path("stage1/crates/axiomc/tests/support/lib_unit.rs"),
        Path("docs/direct-native-runtime-abi-v0.md"),
    ]
    for path in paths:
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / path, root / path)
    shutil.copytree(
        ROOT / "stage1/compiler-contracts/fixtures/http-server-v1",
        root / "stage1/compiler-contracts/fixtures/http-server-v1",
    )


def main() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory) / "repo"
        copy_contract_root(root)

        valid = run(root)
        if valid.returncode != 0:
            raise SystemExit(f"valid HTTP server v1 evidence was rejected: {valid.stdout}{valid.stderr}")
        if json.loads(valid.stdout)["fixtures"] != 33:
            raise SystemExit("HTTP server v1 fixture count drifted")
        second = run(root)
        if valid.stdout != second.stdout:
            raise SystemExit("HTTP server v1 JSON output is not deterministic")

        snapshot_path = root / "stage1/compiler-contracts/snapshots/http-server-v1.json"
        original = json.loads(snapshot_path.read_text(encoding="utf-8"))

        value = json.loads(json.dumps(original))
        value["implementation"]["external_bind"] = True
        write(snapshot_path, value)
        expect_failure(root, "unqualified external bind was accepted")

        value = json.loads(json.dumps(original))
        value["request"]["fields"].remove("peer")
        write(snapshot_path, value)
        expect_failure(root, "incomplete request envelope was accepted")

        value = json.loads(json.dumps(original))
        value["limits"]["body_bytes"] = 8 * 1024 * 1024
        write(snapshot_path, value)
        expect_failure(root, "oversized request budget was accepted")

        value = json.loads(json.dumps(original))
        value["shutdown"]["flush_observability"] = False
        write(snapshot_path, value)
        expect_failure(root, "shutdown without observability flush was accepted")

        value = json.loads(json.dumps(original))
        value["authority"]["default_deny"] = 1
        write(snapshot_path, value)
        expect_failure(root, "numeric value was accepted for a boolean schema constant")

        value = json.loads(json.dumps(original))
        value["limits"]["max_requests_per_connection"] = 2048
        write(snapshot_path, value)
        expect_failure(root, "unbounded keep-alive request count was accepted")

        value = json.loads(json.dumps(original))
        del value["limits"]["max_requests_per_connection"]
        write(snapshot_path, value)
        expect_failure(root, "missing keep-alive request bound was accepted")

        value = json.loads(json.dumps(original))
        value["limits"]["backpressure"]["listener_capacity"]["http_response_possible"] = True
        write(snapshot_path, value)
        expect_failure(root, "listener saturation was allowed to emit an HTTP response")

        value = json.loads(json.dumps(original))
        value["limits"]["backpressure"]["handler_queue_capacity"]["overload_status"] = 429
        write(snapshot_path, value)
        expect_failure(root, "handler queue overload status drift was accepted")

        value = json.loads(json.dumps(original))
        value["unexpected"] = True
        write(snapshot_path, value)
        expect_failure(root, "unknown snapshot field was accepted")

        schema = json.loads(
            (root / "stage1/compiler-contracts/schemas/axiom.runtime_http_server.v1.schema.json").read_text(
                encoding="utf-8"
            )
        )
        promoted = json.loads(json.dumps(original))
        promoted["implementation"].update(
            {
                "tier": "runtime_complete",
                "status": "complete",
                "loopback_only": False,
                "dynamic_handler": True,
                "external_bind": True,
                "structured_concurrency": True,
                "http_1_1_proxy": True,
                "graceful_drain": True,
                "observability_flush": True,
            }
        )
        for reference in promoted["fixtures"]:
            reference["evidence_tier"] = "runtime"
        from importlib.util import module_from_spec, spec_from_file_location

        spec = spec_from_file_location("check_http_server_v1", CHECKER)
        if spec is None or spec.loader is None:
            raise SystemExit("cannot import HTTP server checker")
        checker = module_from_spec(spec)
        spec.loader.exec_module(checker)
        unsafe_schema = json.loads(json.dumps(schema))
        unsafe_schema["$defs"]["fixture_ref"]["properties"]["id"]["pattern"] = "^(a+)+$"
        try:
            checker.validate_schema_node(original, unsafe_schema, "$", unsafe_schema.get("$defs", {}))
        except checker.ContractError:
            pass
        else:
            raise SystemExit("trusted checker accepted an untrusted schema regular expression")

        checker.validate_schema_node(promoted, schema, "$", schema.get("$defs", {}))
        write(snapshot_path, promoted)
        expect_failure(root, "current checker accepted an unproved runtime-complete promotion")

        incomplete = json.loads(json.dumps(original))
        incomplete["implementation"]["tier"] = "runtime_complete"
        try:
            checker.validate_schema_node(incomplete, schema, "$", schema.get("$defs", {}))
        except checker.ContractError:
            pass
        else:
            raise SystemExit("schema accepted an incomplete runtime-complete claim")

        omitted = json.loads(json.dumps(promoted))
        omitted["fixtures"] = omitted["fixtures"][:-1]
        try:
            checker.validate_schema_node(omitted, schema, "$", schema.get("$defs", {}))
        except checker.ContractError:
            pass
        else:
            raise SystemExit("schema accepted runtime promotion with an omitted fixture")

        duplicated = json.loads(json.dumps(promoted))
        duplicated["fixtures"][-1] = json.loads(json.dumps(duplicated["fixtures"][0]))
        try:
            checker.validate_schema_node(duplicated, schema, "$", schema.get("$defs", {}))
        except checker.ContractError:
            pass
        else:
            raise SystemExit("schema accepted runtime promotion with a duplicated fixture")

        informational = json.loads(json.dumps(original))
        informational["response"]["status_minimum"] = 100
        try:
            checker.validate_schema_node(informational, schema, "$", schema.get("$defs", {}))
        except checker.ContractError:
            pass
        else:
            raise SystemExit("schema accepted an informational status as a terminal response")

        for field, value in (("start_line_bytes", 4096), ("header_count", 32), ("header_bytes", 32768)):
            drifted = json.loads(json.dumps(original))
            drifted["limits"][field] = value
            write(snapshot_path, drifted)
            expect_failure(root, f"{field} drifted away from its fixture boundary")

        write(snapshot_path, original)
        fixture_path = root / "stage1/compiler-contracts/fixtures/http-server-v1/unauthorized-bind.json"
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        fixture["expected"] = "accepted"
        write(fixture_path, fixture)
        expect_failure(root, "successful unauthorized bind fixture was accepted")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/http-server-v1/unauthorized-bind.json",
            fixture_path,
        )
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        fixture["operation"] = "serve"
        write(fixture_path, fixture)
        expect_failure(root, "fixture operation drift was accepted")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/http-server-v1/unauthorized-bind.json",
            fixture_path,
        )
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        fixture["assertions"][0] = "arbitrary prose"
        write(fixture_path, fixture)
        expect_failure(root, "fixture assertion drift was accepted")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/http-server-v1/unauthorized-bind.json",
            fixture_path,
        )
        sigterm = root / "stage1/compiler-contracts/fixtures/http-server-v1/sigterm-shutdown.json"
        fixture = json.loads(sigterm.read_text(encoding="utf-8"))
        fixture["evidence_tier"] = "runtime"
        write(sigterm, fixture)
        expect_failure(root, "SIGTERM target fixture was promoted without runtime evidence")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/http-server-v1/sigterm-shutdown.json",
            sigterm,
        )
        proxy_denial = root / "stage1/compiler-contracts/fixtures/http-server-v1/untrusted-forwarded-headers.json"
        fixture = json.loads(proxy_denial.read_text(encoding="utf-8"))
        fixture["details"]["proxy_authority_match"] = True
        write(proxy_denial, fixture)
        expect_failure(root, "proxy mismatch fixture claimed a matching authority")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/http-server-v1/untrusted-forwarded-headers.json",
            proxy_denial,
        )
        sigterm.unlink()
        expect_failure(root, "missing SIGTERM fixture was accepted")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/http-server-v1/sigterm-shutdown.json",
            sigterm,
        )
        write(snapshot_path, original)
        value = json.loads(json.dumps(original))
        value["migration"]["current_evidence"][0]["path"] = "docs/missing.md"
        write(snapshot_path, value)
        expect_failure(root, "evidence path drift was accepted")

        value = json.loads(json.dumps(original))
        value["migration"]["current_evidence"][0]["anchors"][0] = "missing evidence anchor"
        write(snapshot_path, value)
        expect_failure(root, "evidence anchor drift was accepted")

        write(snapshot_path, original)
        evidence_path = root / "docs/direct-native-runtime-abi-v0.md"
        evidence_backup = root / "docs/direct-native-runtime-abi-v0.backup.md"
        evidence_path.replace(evidence_backup)
        evidence_path.symlink_to(evidence_backup.name)
        expect_failure(root, "symlinked PR-head evidence was accepted")

    print("HTTP server v1 checker tests passed")


if __name__ == "__main__":
    main()
