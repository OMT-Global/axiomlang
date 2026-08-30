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
CHECKER = ROOT / "scripts/ci/check-http-client-v1.py"
spec = importlib.util.spec_from_file_location("check_http_client_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class HttpClientContractTests(unittest.TestCase):
    def test_contract_and_negative_fixtures_pass(self) -> None:
        result = checker.validate_contract(ROOT)
        self.assertEqual(result, {"schema": "axiom.runtime_http_client.v1", "ok": True, "fixtures": 6})

    def test_cli_json_is_deterministic(self) -> None:
        first = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        second = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)
        self.assertEqual(json.loads(first.stdout)["ok"], True)

    def test_error_details_are_code_specific(self) -> None:
        snapshot = checker.load_json(ROOT / checker.SNAPSHOT)
        mismatch = checker.load_json(ROOT / checker.FIXTURES / "structured-error-mismatch.json")
        self.assertFalse(checker.structured_error_matches(mismatch["error"], snapshot))

    def test_snapshot_rejects_unverified_tls_and_redirects(self) -> None:
        snapshot_path = ROOT / checker.SNAPSHOT
        with tempfile.TemporaryDirectory() as temporary:
            temp_root = Path(temporary)
            (temp_root / checker.SCHEMA).parent.mkdir(parents=True)
            (temp_root / checker.SNAPSHOT).parent.mkdir(parents=True)
            (temp_root / checker.FIXTURES).mkdir(parents=True)
            (temp_root / checker.SCHEMA).write_text((ROOT / checker.SCHEMA).read_text(encoding="utf-8"), encoding="utf-8")
            snapshot = json.loads(snapshot_path.read_text(encoding="utf-8"))
            snapshot["policies"]["tls"] = "disabled"
            (temp_root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            for fixture in snapshot["fixtures"]:
                source = ROOT / checker.FIXTURES / fixture["path"]
                (temp_root / checker.FIXTURES / fixture["path"]).write_text(source.read_text(encoding="utf-8"), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(temp_root)


if __name__ == "__main__":
    unittest.main()
