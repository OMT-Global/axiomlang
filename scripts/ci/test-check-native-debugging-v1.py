#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-native-debugging-v1.py"
spec = importlib.util.spec_from_file_location("check_native_debugging_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


def copy_contract_root(temporary: str) -> Path:
    root = Path(temporary)
    for path in [checker.SCHEMA, checker.STATUS_SCHEMA, checker.SNAPSHOT, checker.DAP_SOURCE, checker.PROJECT_SOURCE, checker.DEBUG_DOC]:
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / path, root / path)
    shutil.copytree(ROOT / checker.FIXTURES, root / checker.FIXTURES)
    return root


class NativeDebuggingContractTests(unittest.TestCase):
    def test_contract_and_fixtures_pass(self) -> None:
        self.assertEqual(
            checker.validate_contract(ROOT),
            {"schema": "axiom.native_debugging.v1", "status_schema": "axiom.native_debug_status.v1", "ok": True, "fixtures": 4, "targets": 2, "operations": 13},
        )

    def test_actual_adapter_status_conforms_to_closed_published_schema(self) -> None:
        status = checker.validate_adapter_status(ROOT)
        self.assertEqual(status["schemaVersion"], "axiom.native_debug_status.v1")

        unexpected = dict(status)
        unexpected["contract"] = "native.debugging"
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(unexpected, checker.load(ROOT / checker.STATUS_SCHEMA))

    def test_adapter_execution_uses_checkout_isolated_target_directory(self) -> None:
        status = checker.load(ROOT / checker.FIXTURES / "current-source-simulator.json")
        body = {
            "seq": 1,
            "type": "response",
            "request_seq": 1,
            "success": True,
            "command": "axiom/debugStatus",
            "body": status,
        }
        payload = json.dumps(body, separators=(",", ":")).encode("utf-8")
        stdout = f"Content-Length: {len(payload)}\r\n\r\n".encode("ascii") + payload
        completed = subprocess.CompletedProcess([], 0, stdout=stdout, stderr=b"")

        with patch.object(checker.subprocess, "run", return_value=completed) as run:
            self.assertEqual(checker.read_adapter_status(ROOT), status)

        environment = run.call_args.kwargs["env"]
        self.assertEqual(
            environment["CARGO_TARGET_DIR"],
            str(checker.adapter_target_dir(ROOT)),
        )
        self.assertNotEqual(
            checker.adapter_target_dir(ROOT),
            checker.adapter_target_dir(ROOT.parent / "different-checkout"),
        )

    def test_cli_is_deterministic(self) -> None:
        first = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        second = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)
        self.assertTrue(json.loads(first.stdout)["ok"])

    def test_snapshot_rejects_process_backed_claim(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            snapshot = checker.load(root / checker.SNAPSHOT)
            snapshot["implementation"]["process_backed"] = True
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_normative_schema_accepts_a_fully_qualified_implementation(self) -> None:
        schema = checker.load(ROOT / checker.SCHEMA)
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["implementation"] = {
            "tier": "production_qualified",
            "dap_mode": "process_backed",
            "process_backed": True,
            "native_axiom_dwarf": True,
            "profile_symbolization": True,
            "blockers": [],
        }
        checker.validate_schema(snapshot, schema)

    def test_normative_schema_rejects_an_incomplete_qualified_claim(self) -> None:
        schema = checker.load(ROOT / checker.SCHEMA)
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["implementation"]["tier"] = "production_qualified"
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(snapshot, schema)

    def test_normative_schema_rejects_qualified_blockers_above_max_items(self) -> None:
        schema = checker.load(ROOT / checker.SCHEMA)
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["implementation"] = {
            "tier": "production_qualified",
            "dap_mode": "process_backed",
            "process_backed": True,
            "native_axiom_dwarf": True,
            "profile_symbolization": True,
            "blockers": [1436],
        }
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(snapshot, schema)

    def test_normative_schema_rejects_duplicate_array_items(self) -> None:
        schema = checker.load(ROOT / checker.SCHEMA)
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["implementation"]["blockers"] = [1436, 1436]
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(snapshot, schema)

    def test_normative_schema_rejects_numeric_boolean_substitutes(self) -> None:
        schema = checker.load(ROOT / checker.SCHEMA)
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["implementation"]["process_backed"] = 0
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(snapshot, schema)

    def test_snapshot_rejects_incomplete_target_matrix(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            snapshot = checker.load(root / checker.SNAPSHOT)
            snapshot["native_dwarf"]["targets"] = ["x86_64-unknown-linux-gnu"]
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_snapshot_rejects_authoritative_sidecars(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            snapshot = checker.load(root / checker.SNAPSHOT)
            snapshot["sidecars"]["supplemental_only"] = False
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_checker_rejects_unqualified_process_spawn(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            dap = root / checker.DAP_SOURCE
            dap.write_text(dap.read_text(encoding="utf-8") + "\n// Command::new(program).spawn()\n", encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_fixture_rejects_false_native_status(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "current-source-simulator.json")
        fixture["nativeAxiomDwarf"] = True
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("current-source-simulator", fixture)

    def test_snapshot_rejects_incomplete_fixture_set(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            snapshot = checker.load(root / checker.SNAPSHOT)
            snapshot["fixtures"] = snapshot["fixtures"][:-1]
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)


if __name__ == "__main__":
    unittest.main()
