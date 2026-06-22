#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "pr-queue-remediation.py"

spec = importlib.util.spec_from_file_location("pr_queue_remediation", SCRIPT_PATH)
queue = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules["pr_queue_remediation"] = queue
spec.loader.exec_module(queue)


def check(name: str, status: str, conclusion: str = "") -> dict[str, str]:
    return {"name": name, "status": status, "conclusion": conclusion}


def pr(
    number: int,
    *,
    mergeable: str = "MERGEABLE",
    review_decision: str = "REVIEW_REQUIRED",
    checks: list[dict[str, str]] | None = None,
    is_draft: bool = False,
) -> dict[str, object]:
    return {
        "number": number,
        "title": f"PR {number}",
        "url": f"https://github.com/OMT-Global/axiomlang/pull/{number}",
        "baseRefName": "main",
        "headRefName": f"codex/pr-{number}",
        "headRefOid": f"sha-{number}",
        "isDraft": is_draft,
        "mergeable": mergeable,
        "reviewDecision": review_decision,
        "statusCheckRollup": checks
        if checks is not None
        else [check("CI Gate", "COMPLETED", "SUCCESS")],
    }


class PrQueueRemediationTests(unittest.TestCase):
    def test_classifies_open_queue_by_remediation_state(self) -> None:
        report = queue.build_report(
            repo="OMT-Global/axiomlang",
            rechecked_at="2026-05-31T14:00:00Z",
            source="fixture",
            prs=[
                pr(5),
                pr(1, mergeable="CONFLICTING", review_decision="APPROVED"),
                pr(3, review_decision="CHANGES_REQUESTED"),
                pr(2, review_decision="APPROVED", checks=[check("CI Gate", "COMPLETED", "FAILURE")]),
                pr(4, checks=[check("Fast Checks", "QUEUED")]),
                pr(6, review_decision="APPROVED"),
            ],
        )

        classifications = {
            item["number"]: item["classification"] for item in report["pull_requests"]
        }
        self.assertEqual(
            classifications,
            {
                1: "needs_rebase",
                2: "ci_failing",
                3: "review_blocked",
                4: "ci_pending",
                5: "awaiting_review",
                6: "merge_ready",
            },
        )
        self.assertEqual([item["number"] for item in report["worklist"]], [1, 2, 3, 4, 5, 6])

    def test_missing_check_rollup_requires_recheck_before_review_wait(self) -> None:
        report = queue.build_report(
            repo="OMT-Global/axiomlang",
            rechecked_at="2026-05-31T14:00:00Z",
            source="fixture",
            prs=[pr(10, checks=[])],
        )

        item = report["pull_requests"][0]
        self.assertEqual(item["checks"]["state"], "missing")
        self.assertEqual(item["classification"], "needs_recheck")

    def test_repeated_classification_is_deterministic_for_same_snapshot(self) -> None:
        snapshot = [
            pr(9, checks=[check("CI Gate", "IN_PROGRESS")]),
            pr(8, review_decision="APPROVED"),
        ]

        first = queue.build_report(
            repo="OMT-Global/axiomlang",
            rechecked_at="2026-05-31T14:00:00Z",
            source="fixture",
            prs=snapshot,
        )
        second = queue.build_report(
            repo="OMT-Global/axiomlang",
            rechecked_at="2026-05-31T14:05:00Z",
            source="fixture",
            prs=list(reversed(snapshot)),
        )

        first_order = [(item["number"], item["classification"]) for item in first["worklist"]]
        second_order = [(item["number"], item["classification"]) for item in second["worklist"]]
        self.assertEqual(first_order, second_order)

    def test_report_declares_non_mutating_operator_guards(self) -> None:
        report = queue.build_report(
            repo="OMT-Global/axiomlang",
            rechecked_at="2026-05-31T14:00:00Z",
            source="fixture",
            prs=[pr(1)],
        )

        self.assertFalse(report["operator_guards"]["auto_merge"])
        self.assertFalse(report["operator_guards"]["force_push"])
        self.assertFalse(report["operator_guards"]["mutates_branches"])
        self.assertTrue(report["operator_guards"]["requires_fresh_recheck_after_remediation"])

    def test_cli_emits_json_from_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "prs.json"
            path.write_text(json.dumps([pr(7, review_decision="APPROVED")]), encoding="utf-8")
            result = subprocess.run(
                [
                    str(SCRIPT_PATH),
                    "--input",
                    str(path),
                    "--rechecked-at",
                    "2026-05-31T14:00:00Z",
                    "--json",
                ],
                check=True,
                capture_output=True,
                text=True,
            )

        payload = json.loads(result.stdout)
        self.assertEqual(payload["schema_version"], "axiom.pr_queue_remediation.v0")
        self.assertEqual(payload["worklist"][0]["classification"], "merge_ready")


if __name__ == "__main__":
    unittest.main()
