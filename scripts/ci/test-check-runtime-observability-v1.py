#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-runtime-observability-v1.py"
spec = importlib.util.spec_from_file_location("check_runtime_observability_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class RuntimeObservabilityContractTests(unittest.TestCase):
    def test_contract_and_fixtures_pass(self) -> None:
        self.assertEqual(checker.validate_contract(ROOT), {"schema": "axiom.runtime_observability.v1", "ok": True, "fixtures": 5})

    def test_cli_is_deterministic(self) -> None:
        first = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        second = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)
        self.assertTrue(json.loads(first.stdout)["ok"])

    def test_snapshot_rejects_unredacted_sink(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / checker.SCHEMA).parent.mkdir(parents=True)
            (root / checker.SNAPSHOT).parent.mkdir(parents=True)
            (root / checker.FIXTURES).mkdir(parents=True)
            (root / checker.SCHEMA).write_text((ROOT / checker.SCHEMA).read_text(encoding="utf-8"), encoding="utf-8")
            snapshot = json.loads((ROOT / checker.SNAPSHOT).read_text(encoding="utf-8"))
            snapshot["redaction"]["before_sink"] = False
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            for fixture in snapshot["fixtures"]:
                source = ROOT / checker.FIXTURES / fixture["path"]
                (root / checker.FIXTURES / fixture["path"]).write_text(source.read_text(encoding="utf-8"), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_redaction_fixture_rejects_secret_value(self) -> None:
        fixture = json.loads(
            (ROOT / checker.FIXTURES / "redaction-negative.json").read_text(encoding="utf-8")
        )
        fixture["sink_payload"]["password"] = "user-entered"
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture(
                "redaction-negative", fixture, checker.load(ROOT / checker.SNAPSHOT)
            )


if __name__ == "__main__":
    unittest.main()
