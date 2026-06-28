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
SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "issue-pr-traceability.py"

spec = importlib.util.spec_from_file_location("issue_pr_traceability", SCRIPT_PATH)
traceability = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules["issue_pr_traceability"] = traceability
spec.loader.exec_module(traceability)


class IssuePrTraceabilityTests(unittest.TestCase):
    def test_parses_issue_links_with_relationships(self) -> None:
        body = "Closes #852\nRefs OMT-Global/axiomlang#785\n"

        links = traceability.parse_issue_links(body, "OMT-Global/axiomlang")

        self.assertEqual(
            [(link.repo, link.number, link.relationship) for link in links],
            [
                ("OMT-Global/axiomlang", 852, "closes"),
                ("OMT-Global/axiomlang", 785, "refs"),
            ],
        )

    def test_ignores_repo_slug_with_path_traversal(self) -> None:
        body = "refs a-b/..#3\nsee ../secret#9\n"

        links = traceability.parse_issue_links(body, "OMT-Global/axiomlang")

        self.assertEqual(links, [])

    def test_missing_issue_without_exception_is_flagged(self) -> None:
        report = traceability.build_report(
            repo="OMT-Global/axiomlang",
            body="## Summary\n- Update docs.\n",
            pr_number=1,
            pr_title="No issue",
            head_sha="abc123",
            changed_files=[],
            resolver=None,
        )

        self.assertFalse(report["ok"])
        self.assertEqual(report["problems"][0]["code"], "missing_governing_issue")

    def test_no_issue_exception_is_advisory_ok(self) -> None:
        report = traceability.build_report(
            repo="OMT-Global/axiomlang",
            body="No governing issue for this generated docs-only update.",
            pr_number=1,
            pr_title="No issue exception",
            head_sha="abc123",
            changed_files=[],
            resolver=None,
        )

        self.assertTrue(report["ok"])
        self.assertTrue(report["no_issue_exception"])

    def test_resolver_marks_closed_issue_warning(self) -> None:
        def resolver(repo: str, number: int) -> dict[str, object]:
            return {"repo": repo, "number": number, "resolved": True, "status": "closed"}

        report = traceability.build_report(
            repo="OMT-Global/axiomlang",
            body="Closes #852",
            pr_number=1,
            pr_title="Closed issue",
            head_sha="abc123",
            changed_files=[],
            resolver=resolver,
        )

        self.assertTrue(report["ok"])
        self.assertEqual(report["problems"][0]["code"], "linked_issue_not_open")

    def test_semantic_hint_classifies_governance_before_docs(self) -> None:
        self.assertEqual(
            traceability.semantic_hint("docs/bootstrap/onboarding.md"),
            "governance",
        )
        self.assertEqual(traceability.semantic_hint("docs/vision.md"), "documentation")

    def test_cli_emits_json_report_for_body_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            body_path = Path(tmp) / "body.md"
            body_path.write_text("Closes #852\n", encoding="utf-8")
            result = subprocess.run(
                [
                    str(SCRIPT_PATH),
                    "--repo",
                    "OMT-Global/axiomlang",
                    "--body-file",
                    str(body_path),
                    "--changed-file",
                    "scripts/ci/issue-pr-traceability.py",
                    "--offline",
                    "--json",
                ],
                check=True,
                capture_output=True,
                text=True,
            )

        payload = json.loads(result.stdout)
        self.assertTrue(payload["ok"])
        self.assertEqual(payload["issue_links"][0]["number"], 852)
        self.assertEqual(payload["semantic_hints"][0]["semantic_hint"], "delivery_governance")


if __name__ == "__main__":
    unittest.main()
