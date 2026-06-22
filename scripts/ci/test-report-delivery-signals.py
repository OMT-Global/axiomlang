#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("report-delivery-signals.py")
SPEC = importlib.util.spec_from_file_location("delivery_signals", SCRIPT)
assert SPEC is not None
assert SPEC.loader is not None
delivery_signals = importlib.util.module_from_spec(SPEC)
sys.modules["delivery_signals"] = delivery_signals
SPEC.loader.exec_module(delivery_signals)


def fixture_payload() -> dict:
    return {
        "issues": [
            {
                "number": 852,
                "state": "OPEN",
                "title": "Delivery signals: improve issue-to-PR traceability",
                "url": "https://github.com/OMT-Global/axiomlang/issues/852",
            },
            {
                "number": 700,
                "state": "CLOSED",
                "title": "Closed issue",
                "url": "https://github.com/OMT-Global/axiomlang/issues/700",
            },
        ],
        "prs": [
            {
                "number": 10,
                "title": "Trace delivery signals",
                "url": "https://github.com/OMT-Global/axiomlang/pull/10",
                "headRefName": "codex/delivery-signals",
                "baseRefName": "main",
                "headRefOid": "abc123",
                "body": "## Governing Issue\n\nCloses #852\n",
                "mergeStateStatus": "CLEAN",
                "reviewDecision": "APPROVED",
                "statusCheckRollup": [
                    {
                        "name": "CI Gate",
                        "workflowName": "PR Fast CI",
                        "status": "COMPLETED",
                        "conclusion": "SUCCESS",
                    }
                ],
                "closingIssuesReferences": [
                    {
                        "number": 852,
                        "state": "OPEN",
                        "title": "Delivery signals: improve issue-to-PR traceability",
                        "url": "https://github.com/OMT-Global/axiomlang/issues/852",
                    }
                ],
                "files": [
                    {"path": "scripts/ci/report-delivery-signals.py"},
                    {"path": "stage1/examples/proof_http_service/src/main.ax"},
                ],
                "isDraft": False,
            },
            {
                "number": 11,
                "title": "Missing issue link",
                "url": "https://github.com/OMT-Global/axiomlang/pull/11",
                "headRefName": "codex/no-issue",
                "baseRefName": "main",
                "headRefOid": "def456",
                "body": "## Summary\n\nNo link here.\n",
                "mergeStateStatus": "BLOCKED",
                "reviewDecision": "REVIEW_REQUIRED",
                "statusCheckRollup": [
                    {
                        "name": "CI Gate",
                        "workflowName": "PR Fast CI",
                        "status": "QUEUED",
                        "conclusion": "",
                    }
                ],
                "closingIssuesReferences": [],
                "files": [{"path": "docs/delivery-signals-v0.md"}],
                "isDraft": False,
            },
            {
                "number": 12,
                "title": "Closed issue link",
                "url": "https://github.com/OMT-Global/axiomlang/pull/12",
                "headRefName": "codex/closed-issue",
                "baseRefName": "main",
                "headRefOid": "fedcba",
                "body": "Fixes #700\n",
                "mergeStateStatus": "DIRTY",
                "reviewDecision": "CHANGES_REQUESTED",
                "statusCheckRollup": [
                    {
                        "name": "CI Gate",
                        "workflowName": "PR Fast CI",
                        "status": "COMPLETED",
                        "conclusion": "FAILURE",
                    }
                ],
                "closingIssuesReferences": [],
                "files": [{"path": "stage1/examples/proof_worker/src/main.ax"}],
                "isDraft": False,
            },
        ],
    }


class DeliverySignalsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.previous_collected_at = os.environ.get("AXIOM_DELIVERY_COLLECTED_AT")
        os.environ["AXIOM_DELIVERY_COLLECTED_AT"] = "2026-05-30T20:00:00Z"

    def tearDown(self) -> None:
        if self.previous_collected_at is None:
            os.environ.pop("AXIOM_DELIVERY_COLLECTED_AT", None)
        else:
            os.environ["AXIOM_DELIVERY_COLLECTED_AT"] = self.previous_collected_at

    def build_report(self, fixture: Path, *issues: int) -> dict:
        args = argparse.Namespace(
            repo="OMT-Global/axiomlang",
            state="open",
            limit=50,
            pr=None,
            issue=list(issues) or None,
            fixture=fixture,
            check_traceability=False,
        )
        return delivery_signals.build_report(args)

    def test_fixture_report_classifies_pr_queue_and_issue_links(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = Path(tmp) / "prs.json"
            fixture.write_text(json.dumps(fixture_payload()), encoding="utf-8")

            report = self.build_report(fixture)

        self.assertEqual(report["schema_version"], "axiom.delivery_signals.v0")
        self.assertEqual(report["collected_at"], "2026-05-30T20:00:00Z")
        self.assertEqual(report["summary"]["mergeable"], 1)
        self.assertEqual(report["summary"]["ci_pending"], 1)
        self.assertEqual(report["summary"]["ci_failing"], 1)
        self.assertEqual(report["summary"]["missing_issue_link"], 1)
        self.assertEqual(report["summary"]["closed_or_missing_issue"], 1)

        by_number = {pr["number"]: pr for pr in report["prs"]}
        self.assertIn("mergeable", by_number[10]["classification"])
        self.assertIn("missing_issue_link", by_number[11]["classification"])
        self.assertIn("closed_or_missing_issue", by_number[12]["classification"])
        self.assertEqual(by_number[10]["evidence"][0]["evidence_type"], "ci_status")
        self.assertEqual(by_number[10]["evidence"][1]["evidence_type"], "review_state")

    def test_issue_filter_resolves_pr_files_and_semantic_nodes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = Path(tmp) / "prs.json"
            fixture.write_text(json.dumps(fixture_payload()), encoding="utf-8")

            report = self.build_report(fixture, 852)

        self.assertEqual([pr["number"] for pr in report["prs"]], [10])
        self.assertEqual(report["issues"][0]["number"], 852)
        self.assertEqual(report["issues"][0]["changed_files"], [
            "scripts/ci/report-delivery-signals.py",
            "stage1/examples/proof_http_service/src/main.ax",
        ])
        self.assertTrue(
            any(
                node["kind"] == "package"
                and node["path"] == "stage1/examples/proof_http_service"
                for node in report["issues"][0]["semantic_nodes"]
            )
        )

    def test_check_traceability_exits_nonzero_for_missing_or_closed_issue(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = Path(tmp) / "prs.json"
            fixture.write_text(json.dumps(fixture_payload()), encoding="utf-8")
            completed = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--fixture",
                    str(fixture),
                    "--check-traceability",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )

        self.assertEqual(completed.returncode, 1)
        payload = json.loads(completed.stdout)
        self.assertEqual(payload["summary"]["missing_issue_link"], 1)


if __name__ == "__main__":
    unittest.main()
