#!/usr/bin/env python3
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VALIDATOR = ROOT / "scripts/ci/validate-readiness-gates.py"
SPEC = importlib.util.spec_from_file_location("readiness_validator", VALIDATOR)
assert SPEC and SPEC.loader
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)

SHA = "0123456789abcdef0123456789abcdef01234567"


class ReadinessEvidenceTests(unittest.TestCase):
    def write_valid(self, root: Path) -> None:
        reports = {
            "rust-exit-readiness": "axiom.rust_exit.readiness.v1",
            "self-hosting-language-readiness": "axiom.self_hosting.language_readiness.v0",
            "snapshot-bootstrap-readiness": "axiom.self_hosting.snapshot_bootstrap_readiness.v0",
        }
        for name, schema in reports.items():
            (root / f"{name}.json").write_text(
                json.dumps(
                    {
                        "schema": schema,
                        "ready": False,
                        "headSha": SHA,
                        "executed": True,
                        "checks": [{"name": "blocked", "status": "fail", "detail": "expected"}],
                    }
                ),
                encoding="utf-8",
            )
        tests = {}
        for name in ("native-build-purity", "self-hosting-spike-parity"):
            tests[name] = {
                "headSha": SHA,
                "executed": True,
                "exitCode": 0,
                "testsRun": 1,
                "status": "passed",
            }
            (root / f"{name}.log").write_text("running 1 test\ntest result: ok\n", encoding="utf-8")
        (root / "readiness-gates.json").write_text(
            json.dumps(
                {
                    "schema": "axiom.readiness.gates.v1",
                    "headSha": SHA,
                    "executed": True,
                    "evidenceValid": True,
                    "status": "blocked",
                    "tests": tests,
                }
            ),
            encoding="utf-8",
        )

    def test_ready_false_reports_are_valid_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_valid(root)
            self.assertEqual([], module.validate(root, SHA))

    def test_stale_report_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_valid(root)
            report = json.loads((root / "rust-exit-readiness.json").read_text())
            report["headSha"] = "f" * 40
            (root / "rust-exit-readiness.json").write_text(json.dumps(report))
            errors = module.validate(root, SHA)
            self.assertTrue(any("stale head SHA" in error for error in errors))

    def test_zero_test_evidence_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_valid(root)
            aggregate = json.loads((root / "readiness-gates.json").read_text())
            aggregate["tests"]["native-build-purity"]["testsRun"] = 0
            (root / "readiness-gates.json").write_text(json.dumps(aggregate))
            errors = module.validate(root, SHA)
            self.assertTrue(any("zero-test evidence" in error for error in errors))

    def test_executed_blocked_test_is_valid_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_valid(root)
            aggregate = json.loads((root / "readiness-gates.json").read_text())
            aggregate["tests"]["self-hosting-spike-parity"].update(
                {"exitCode": 1, "status": "failed", "testsRun": 2}
            )
            (root / "readiness-gates.json").write_text(json.dumps(aggregate))
            self.assertEqual([], module.validate(root, SHA))


if __name__ == "__main__":
    unittest.main()
