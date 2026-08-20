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


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-io-reactor-v1.py"
spec = importlib.util.spec_from_file_location("check_io_reactor_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


def copy_contract_root(temporary: str) -> Path:
    root = Path(temporary)
    paths = [
        checker.SCHEMA,
        checker.SNAPSHOT,
        checker.REACTOR_DOC,
        checker.TCP_DOC,
        checker.READINESS,
        checker.CAPABILITY_LEDGER,
        checker.CODEGEN_SOURCE,
        checker.STDLIB_SOURCE,
    ]
    for path in paths:
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / path, root / path)
    shutil.copytree(ROOT / checker.FIXTURES, root / checker.FIXTURES)
    return root


class IoReactorContractTests(unittest.TestCase):
    def test_contract_and_fixtures_pass(self) -> None:
        self.assertEqual(
            checker.validate_contract(ROOT),
            {"schema": "axiom.io_reactor.v1", "ok": True, "fixtures": 6, "operations": 12, "resources": 6, "targets": 2},
        )

    def test_cli_is_deterministic(self) -> None:
        first = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        second = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)
        self.assertTrue(json.loads(first.stdout)["ok"])

    def test_snapshot_rejects_runtime_backed_claim(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            snapshot = checker.load(root / checker.SNAPSHOT)
            snapshot["implementation"]["runtime_backed"] = True
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_current_checker_rejects_tier_promotion(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            snapshot = checker.load(root / checker.SNAPSHOT)
            snapshot["implementation"].update(
                {
                    "tier": "runtime_complete",
                    "runtime_backed": True,
                    "nonblocking_io": True,
                    "portable_adapters": True,
                    "thread_per_connection_free": True,
                    "blockers": [],
                }
            )
            for adapter in snapshot["adapters"]["targets"]:
                adapter["available"] = True
                adapter.pop("reason")
            checker.validate_schema(snapshot, checker.load(root / checker.SCHEMA))
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_schema_rejects_incomplete_runtime_complete_claim(self) -> None:
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["implementation"]["tier"] = "runtime_complete"
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(snapshot, checker.load(ROOT / checker.SCHEMA))

    def test_snapshot_rejects_incomplete_resource_model(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            snapshot = checker.load(root / checker.SNAPSHOT)
            snapshot["readiness_model"]["resources"] = snapshot["readiness_model"]["resources"][:-1]
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_snapshot_rejects_thread_per_connection_contract(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            snapshot = checker.load(root / checker.SNAPSHOT)
            snapshot["adapters"]["no_thread_per_connection"] = False
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_snapshot_rejects_adapter_matrix_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            snapshot = checker.load(root / checker.SNAPSHOT)
            snapshot["adapters"]["targets"][0]["adapter"] = "epoll"
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_schema_rejects_unavailable_adapter_without_reason(self) -> None:
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["adapters"]["targets"][0].pop("reason")
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(snapshot, checker.load(ROOT / checker.SCHEMA))

    def test_schema_const_rejects_boolean_for_integer(self) -> None:
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(True, {"const": 1})

    def test_schema_enum_rejects_boolean_for_integer(self) -> None:
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(False, {"enum": [0]})

    def test_current_fixture_requires_unavailable_adapter_reason(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "current-blocking-runtime.json")
        fixture.pop("adapter_unavailable_reason")
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("current-blocking-runtime", fixture)

    def test_current_fixture_requires_explicit_null_adapter(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "current-blocking-runtime.json")
        fixture.pop("adapter")
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("current-blocking-runtime", fixture)

    def test_partial_io_fixture_requires_complete_inspection(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "partial-io.json")
        fixture["inspection"].pop("deadline")
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("partial-io", fixture)

    def test_cancellation_fixture_requires_generation_bound_inspection(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "cancellation-race.json")
        fixture["inspection"]["operation_generation"] += 1
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("cancellation-race", fixture)

    def test_inspection_rejects_boolean_generation_binding(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "partial-io.json")
        fixture["operation_generation"] = 1
        fixture["inspection"]["operation_generation"] = True
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("partial-io", fixture)

    def test_cancellation_fixture_requires_positive_integer_buffer_bound(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "cancellation-race.json")
        fixture["buffer_bound"] = None
        fixture["inspection"]["buffer_bound"] = None
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("cancellation-race", fixture)

    def test_inspection_rejects_integral_float_bindings(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "partial-io.json")
        fixture["inspection"]["buffer_bound"] = 8.0
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("partial-io", fixture)
        fixture = checker.load(ROOT / checker.FIXTURES / "partial-io.json")
        fixture["inspection"]["operation_generation"] = 7.0
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("partial-io", fixture)

    def test_fixture_rejects_unbounded_operation_acceptance(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "unbounded-buffer-rejected.json")
        fixture["decision"] = "accepted"
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("unbounded-buffer-rejected", fixture)

    def test_cancellation_fixture_rejects_non_positive_generation(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "cancellation-race.json")
        fixture["operation_generation"] = 0
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("cancellation-race", fixture)

    def test_partial_io_fixture_rejects_boolean_generation(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "partial-io.json")
        fixture["operation_generation"] = True
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("partial-io", fixture)

    def test_partial_io_fixture_rejects_boolean_buffer_bound(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "partial-io.json")
        fixture["requested_bytes"] = 1
        fixture["completed_bytes"] = 0
        fixture["remaining_bytes"] = 1
        fixture["buffer_bound"] = True
        fixture["inspection"]["buffer_bound"] = 1
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("partial-io", fixture)

    def test_cancellation_fixture_rejects_event_generation_drift(self) -> None:
        fixture = checker.load(ROOT / checker.FIXTURES / "cancellation-race.json")
        fixture["events"][-1]["operation_generation"] += 1
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("cancellation-race", fixture)

    def test_checker_rejects_erased_blocking_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            codegen = root / checker.CODEGEN_SOURCE
            codegen.write_text(
                codegen.read_text(encoding="utf-8").replace("listener.set_nonblocking(false).ok()?;", ""),
                encoding="utf-8",
            )
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_checker_rejects_readiness_promotion_without_runtime_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            readiness = checker.load(root / checker.READINESS)
            row = next(item for item in readiness["rows"] if item["id"] == "io_reactor")
            row["currentTier"] = "static_spike"
            row["status"] = "partial"
            (root / checker.READINESS).write_text(json.dumps(readiness), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_checker_rejects_capability_ledger_promotion_without_runtime_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            ledger = checker.load(root / checker.CAPABILITY_LEDGER)
            row = next(
                item
                for item in ledger["schemas"]
                if item["name"] == "https://axiom-lang.org/schemas/axiom.io_reactor.v1.schema.json"
            )
            row["evidenceTier"] = "direct_runtime"
            (root / checker.CAPABILITY_LEDGER).write_text(json.dumps(ledger), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

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
