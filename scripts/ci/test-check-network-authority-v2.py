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
CHECKER = ROOT / "scripts/ci/check-network-authority-v2.py"
spec = importlib.util.spec_from_file_location("check_network_authority_v2", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class NetworkAuthorityContractTests(unittest.TestCase):
    def test_contract_and_fixtures_pass(self) -> None:
        self.assertEqual(checker.validate_contract(ROOT), {"schema": "axiom.runtime_network_authority.v2", "ok": True, "fixtures": 4, "directions": 4})

    def test_cli_is_deterministic(self) -> None:
        first = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        second = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)

    def test_external_listen_cannot_become_default_allow(self) -> None:
        snapshot = json.loads((ROOT / checker.SNAPSHOT).read_text(encoding="utf-8"))
        snapshot["rules"]["inbound_listen"]["decision"] = "allow"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / checker.SCHEMA).parent.mkdir(parents=True)
            (root / checker.SNAPSHOT).parent.mkdir(parents=True)
            (root / checker.FIXTURES).mkdir(parents=True)
            (root / checker.SCHEMA).write_text((ROOT / checker.SCHEMA).read_text(encoding="utf-8"), encoding="utf-8")
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            for fixture in snapshot["fixtures"]:
                source = ROOT / checker.FIXTURES / fixture["path"]
                (root / checker.FIXTURES / fixture["path"]).write_text(source.read_text(encoding="utf-8"), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_dynamic_endpoint_fixture_requires_deny(self) -> None:
        fixture = {"direction": "outbound_connect", "dynamic_endpoint": True, "expected": "allow"}
        with self.assertRaises(checker.ContractError):
            checker.validate_fixture("runtime-endpoint", fixture, checker.load(ROOT / checker.SNAPSHOT))


if __name__ == "__main__":
    unittest.main()
