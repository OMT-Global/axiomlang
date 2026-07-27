#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("render-mutation-survivor-report.py")
SPEC = importlib.util.spec_from_file_location("mutation_survivor_report", SCRIPT)
assert SPEC is not None
assert SPEC.loader is not None
mutation_survivor_report = importlib.util.module_from_spec(SPEC)
sys.modules["mutation_survivor_report"] = mutation_survivor_report
SPEC.loader.exec_module(mutation_survivor_report)


class MutationSurvivorReportTests(unittest.TestCase):
    def test_render_report_groups_survivors_by_file(self) -> None:
        payload = {
            "schema_version": "axiom.stage1.mutation-smoke.v1",
            "governing_issue": {
                "number": 1463,
                "url": "https://github.com/OMT-Global/axiomlang/issues/1463",
            },
            "summary": {"total": 3, "killed": 1, "survived": 2},
            "survivors": [
                {
                    "name": "hir_missing_type_guard",
                    "area": "hir",
                    "file": "stage1/crates/axiomc/src/hir.rs",
                    "test_filter": "type_guard_test",
                    "reproducer": "python3 run-mutation.py --mutant hir_missing_type_guard",
                },
                {
                    "name": "parser_bad_recovery",
                    "area": "parser",
                    "file": "stage1/crates/axiomc/src/syntax.rs",
                    "test_filter": "parser_recovery_test",
                },
            ],
        }
        report = mutation_survivor_report.render_report(payload)
        self.assertIn("### `stage1/crates/axiomc/src/hir.rs`", report)
        self.assertIn("### `stage1/crates/axiomc/src/syntax.rs`", report)
        self.assertIn("Recommended fixture: `hir_hir_missing_type_guard_survivor_test.ax`", report)
        self.assertIn("Function/test focus: `parser_recovery_test`", report)
        self.assertIn(
            "Reproducer: `python3 run-mutation.py --mutant hir_missing_type_guard`",
            report,
        )

    def test_render_report_handles_zero_survivors(self) -> None:
        payload = {
            "schema_version": "axiom.stage1.mutation-smoke.v1",
            "status": "passed",
            "summary": {"total": 4, "killed": 4, "survived": 0},
            "survivors": [],
        }
        report = mutation_survivor_report.render_report(payload)
        self.assertIn("Overall status: `passed`", report)
        self.assertIn("Blocking count: `0`", report)
        self.assertIn("Fatal error: `none`", report)
        self.assertIn("Survived: `0`", report)
        self.assertIn(
            "No survivors were reported. No follow-up fixtures are recommended.",
            report,
        )

    def test_render_report_surfaces_each_blocking_outcome(self) -> None:
        statuses = (
            "baseline_failure",
            "timeout",
            "budget_exhausted",
            "missing_anchor",
            "duplicate_anchor",
            "stale_anchor",
            "execution_error",
            "failed",
            "future_blocker",
        )
        for status in statuses:
            with self.subTest(status=status):
                payload = {
                    "schema_version": "axiom.stage1.mutation-smoke.v1",
                    "status": "failed",
                    "summary": {
                        "total": 1,
                        "killed": 0,
                        "survived": 0,
                        "blocking": 1,
                        status: 1,
                    },
                    "survivors": [],
                }
                report = mutation_survivor_report.render_report(payload)
                self.assertIn("Overall status: `failed`", report)
                self.assertIn("Blocking count: `1`", report)
                self.assertIn(f"- `{status}`: `1`", report)
                self.assertIn("mutation qualification is blocked", report)
                self.assertNotIn("No follow-up fixtures are recommended", report)

    def test_render_report_surfaces_fatal_failure_without_mutant_outcomes(self) -> None:
        payload = {
            "schema_version": "axiom.stage1.mutation-smoke.v1",
            "status": "failed",
            "fatal_error": "expected exact head\nbut observed another",
            "summary": {
                "total": 0,
                "killed": 0,
                "survived": 0,
                "blocking": 0,
            },
            "survivors": [],
        }
        report = mutation_survivor_report.render_report(payload)
        self.assertIn("Overall status: `failed`", report)
        self.assertIn(
            "Fatal error: `expected exact head but observed another`",
            report,
        )
        self.assertIn("mutation qualification is blocked", report)
        self.assertNotIn("No follow-up fixtures are recommended", report)

    def test_render_report_treats_failed_status_as_blocking(self) -> None:
        report = mutation_survivor_report.render_report(
            {
                "status": "failed",
                "summary": {"total": 0, "killed": 0, "survived": 0},
                "survivors": [],
            }
        )
        self.assertIn("Overall status: `failed`", report)
        self.assertIn("mutation qualification is blocked", report)
        self.assertNotIn("No follow-up fixtures are recommended", report)

    def test_legacy_report_infers_blocking_counts_from_mutants(self) -> None:
        payload = {
            "schema_version": "axiom.stage1.mutation-smoke.v0",
            "summary": {"total": 1, "killed": 0, "survived": 0},
            "mutants": [
                {
                    "name": "legacy_timeout",
                    "status": "timeout",
                    "file": "sample.rs",
                    "reproducer": "python3 run-mutation.py --mutant legacy_timeout",
                }
            ],
        }
        report = mutation_survivor_report.render_report(payload)
        self.assertIn("Overall status: `failed`", report)
        self.assertIn("Blocking count: `1`", report)
        self.assertIn("- `timeout`: `1`", report)
        self.assertIn("### Blocking details", report)
        self.assertIn(
            "Reproducer: `python3 run-mutation.py --mutant legacy_timeout`",
            report,
        )
        self.assertIn("mutation qualification is blocked", report)

    def test_render_governing_issue_links_valid_reference(self) -> None:
        rendered = mutation_survivor_report.render_governing_issue(
            {"governing_issue": {"number": 1463, "url": "https://github.com/OMT-Global/axiomlang/issues/1463"}}
        )
        self.assertEqual(rendered, "[#1463](https://github.com/OMT-Global/axiomlang/issues/1463)")

    def test_render_report_marks_missing_governing_issue_unknown(self) -> None:
        report = mutation_survivor_report.render_report({"summary": {}, "survivors": []})
        self.assertIn("Governing issue: unknown", report)

    def test_legacy_survivors_remain_grouped_without_new_status_fields(self) -> None:
        report = mutation_survivor_report.render_report(
            {
                "summary": {"total": 1, "killed": 0, "survived": 1},
                "mutants": [
                    {
                        "name": "legacy_survivor",
                        "area": "parser",
                        "file": "syntax.rs",
                        "test_filter": "legacy_test",
                        "status": "survived",
                    }
                ],
            }
        )
        self.assertIn("Overall status: `unknown (legacy report)`", report)
        self.assertIn("### `syntax.rs`", report)
        self.assertIn("Recommended fixture: `parser_legacy_survivor_survivor_test.ax`", report)


if __name__ == "__main__":
    unittest.main()
