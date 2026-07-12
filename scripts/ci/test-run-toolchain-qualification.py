#!/usr/bin/env python3
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts/ci/run-toolchain-qualification.py"
SHA = "0123456789abcdef0123456789abcdef01234567"


class QualificationTests(unittest.TestCase):
    def run_plan(self, checks):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        base = Path(temp.name)
        plan = base / "plan.json"
        output = base / "out"
        plan.write_text(json.dumps({"checks": checks}), encoding="utf-8")
        result = subprocess.run([
            "python3", str(RUNNER), "--repo-root", str(ROOT), "--output-dir", str(output),
            "--plan", str(plan), "--head-sha", SHA, "--target", "fixture-target",
            "--trigger", "fixture", "--fixture-duration-ms", "7",
        ], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        payload = json.loads((output / "toolchain-qualification.json").read_text())
        return result, payload, output

    def test_success_is_deterministic_and_exact(self):
        checks = [{"id": "full_crate_integration", "command": "printf pass"}]
        first, payload, _ = self.run_plan(checks)
        second, repeated, _ = self.run_plan(checks)
        self.assertEqual(0, first.returncode)
        self.assertEqual(0, second.returncode)
        self.assertEqual(payload, repeated)
        self.assertEqual(SHA, payload["headSha"])
        self.assertEqual("fixture-target", payload["target"])
        self.assertEqual("fixture", payload["trigger"])
        self.assertEqual(7, payload["durationMs"])
        self.assertEqual("none", payload["failureClass"])

    def test_product_failure_is_not_infrastructure_failure(self):
        result, payload, _ = self.run_plan([{"id": "conformance", "command": "exit 3"}])
        self.assertEqual(1, result.returncode)
        self.assertEqual("product_failure", payload["failureClass"])
        self.assertEqual(3, payload["checks"][0]["exitCode"])

    def test_missing_tool_is_infrastructure_failure(self):
        result, payload, _ = self.run_plan([{"id": "supply_chain", "command": "axiom-tool-that-does-not-exist"}])
        self.assertEqual(1, result.returncode)
        self.assertEqual("infrastructure_failure", payload["failureClass"])
        self.assertEqual("infrastructure_failure", payload["checks"][0]["failureClass"])

    def test_declared_missing_infrastructure_is_a_skip(self):
        result, payload, _ = self.run_plan([{
            "id": "supply_chain", "command": "exit 9",
            "requiredTools": ["axiom-tool-that-does-not-exist"],
        }])
        self.assertEqual(1, result.returncode)
        self.assertEqual("skipped", payload["status"])
        self.assertEqual("infrastructure_skip", payload["failureClass"])
        self.assertEqual("skipped", payload["checks"][0]["status"])

    def test_rejects_non_exact_head_before_running_checks(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        base = Path(temp.name)
        plan = base / "plan.json"
        marker = base / "must-not-exist"
        plan.write_text(json.dumps({"checks": [{
            "id": "conformance", "command": f"touch {marker}"
        }]}), encoding="utf-8")
        result = subprocess.run([
            "python3", str(RUNNER), "--repo-root", str(ROOT),
            "--output-dir", str(base / "out"), "--plan", str(plan),
            "--head-sha", "not-a-sha", "--target", "fixture-target",
        ], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        self.assertNotEqual(0, result.returncode)
        self.assertFalse(marker.exists())


if __name__ == "__main__":
    unittest.main()
