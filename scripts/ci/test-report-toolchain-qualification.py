#!/usr/bin/env python3

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REPORTER = ROOT / "scripts/ci/report-toolchain-qualification.py"
SHA = "0123456789abcdef0123456789abcdef01234567"


def evidence(checks, status="failed", failure_class="product_failure"):
    return {
        "schema": "axiom.toolchain_qualification.v0",
        "trigger": "fixture",
        "headSha": SHA,
        "target": "fixture-target",
        "status": status,
        "durationMs": 7,
        "failureClass": failure_class,
        "artifactPaths": [
            artifact for check in checks for artifact in check["artifacts"]
        ]
        + ["toolchain-qualification.json"],
        "checks": checks,
    }


def check(check_id, status, failure_class, exit_code, artifact):
    return {
        "id": check_id,
        "command": "printf 'CAPTURED_VALUE=do-not-print'",
        "target": "fixture-target",
        "required": True,
        "status": status,
        "durationMs": 7,
        "failureClass": failure_class,
        "exitCode": exit_code,
        "artifacts": [artifact],
    }


class QualificationReportTests(unittest.TestCase):
    def run_report(self, payload, expected_sha=SHA):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            evidence_path = root / "toolchain-qualification.json"
            evidence_path.write_text(json.dumps(payload), encoding="utf-8")
            result = subprocess.run(
                [
                    sys.executable,
                    str(REPORTER),
                    "--evidence",
                    str(evidence_path),
                    "--expected-head-sha",
                    expected_sha,
                    "--repo-root",
                    str(ROOT),
                ],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            return result

    def test_single_and_multiple_failures_are_metadata_only(self):
        payload = evidence(
            [
                check("build_purity", "failed", "product_failure", 3, "build_purity.log"),
                check(
                    "proof_smoke",
                    "failed",
                    "infrastructure_failure",
                    127,
                    "proof_smoke.log",
                ),
            ]
        )
        result = self.run_report(payload)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn("status=failed", result.stdout)
        self.assertIn("passed=0 skipped=0 failed=2", result.stdout)
        self.assertIn("id=build_purity", result.stdout)
        self.assertIn("id=proof_smoke", result.stdout)
        self.assertIn("failure_class=product_failure exit_code=3", result.stdout)
        self.assertIn("failure_class=infrastructure_failure exit_code=127", result.stdout)
        self.assertIn(SHA, result.stdout)
        self.assertIn("toolchain-qualification.json", result.stdout)
        self.assertNotIn("CAPTURED_VALUE", result.stdout)
        self.assertNotIn("CAPTURED_VALUE", result.stderr)

    def test_pass_and_skip_counts_are_reported_without_failure_rows(self):
        passed = check("conformance", "passed", "none", 0, "conformance.log")
        skipped = check(
            "supply_chain", "skipped", "infrastructure_skip", 0, "supply_chain.log"
        )
        result = self.run_report(
            evidence(
                [passed, skipped],
                status="skipped",
                failure_class="infrastructure_skip",
            )
        )
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn("passed=1 skipped=1 failed=0", result.stdout)
        self.assertNotIn("qualification failure:", result.stdout)

    def test_malformed_or_stale_evidence_is_a_harness_failure(self):
        payload = evidence(
            [check("conformance", "passed", "none", 0, "conformance.log")],
            status="passed",
            failure_class="none",
        )
        result = self.run_report(
            payload, expected_sha="fedcba9876543210fedcba9876543210fedcba98"
        )
        self.assertEqual(1, result.returncode)
        self.assertIn("status=harness_failure", result.stdout)
        self.assertIn("failure_class=harness_failure", result.stdout)
        self.assertNotIn("CAPTURED_VALUE", result.stdout)

    def test_malformed_json_is_a_harness_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence_path = Path(directory) / "toolchain-qualification.json"
            evidence_path.write_text("not-json\n", encoding="utf-8")
            result = subprocess.run(
                [
                    sys.executable,
                    str(REPORTER),
                    "--evidence",
                    str(evidence_path),
                    "--expected-head-sha",
                    SHA,
                    "--repo-root",
                    str(ROOT),
                ],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
        self.assertEqual(1, result.returncode)
        self.assertIn("status=harness_failure", result.stdout)

    def test_secret_like_log_content_is_never_read_or_printed(self):
        payload = evidence(
            [check("conformance", "failed", "product_failure", 1, "conformance.log")]
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            evidence_path = root / "toolchain-qualification.json"
            evidence_path.write_text(json.dumps(payload), encoding="utf-8")
            (root / "conformance.log").write_text(
                "TOKEN=secret-value\nENV=ENV_VALUE_SHOULD_NOT_PRINT\n",
                encoding="utf-8",
            )
            result = subprocess.run(
                [
                    sys.executable,
                    str(REPORTER),
                    "--evidence",
                    str(evidence_path),
                    "--expected-head-sha",
                    SHA,
                    "--repo-root",
                    str(ROOT),
                ],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertNotIn("secret-value", result.stdout + result.stderr)
        self.assertNotIn("ENV_VALUE_SHOULD_NOT_PRINT", result.stdout + result.stderr)

    def test_control_character_in_artifact_name_fails_closed(self):
        payload = evidence(
            [check("conformance", "failed", "product_failure", 1, "bad\nname.log")]
        )
        result = self.run_report(payload)
        self.assertEqual(1, result.returncode)
        self.assertIn("status=harness_failure", result.stdout)
        self.assertNotIn("bad\nname", result.stdout)


if __name__ == "__main__":
    unittest.main()
