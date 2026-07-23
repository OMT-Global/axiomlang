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

    def test_render_report_handles_zero_survivors(self) -> None:
        payload = {
            "schema_version": "axiom.stage1.mutation-smoke.v1",
            "summary": {"total": 4, "killed": 4, "survived": 0},
            "survivors": [],
        }
        report = mutation_survivor_report.render_report(payload)
        self.assertIn("Survived: `0`", report)
        self.assertIn("No survivors were reported", report)


    def test_render_governing_issue_links_valid_reference(self) -> None:
        rendered = mutation_survivor_report.render_governing_issue(
            {"governing_issue": {"number": 1463, "url": "https://github.com/OMT-Global/axiomlang/issues/1463"}}
        )
        self.assertEqual(rendered, "[#1463](https://github.com/OMT-Global/axiomlang/issues/1463)")

    def test_render_report_marks_missing_governing_issue_unknown(self) -> None:
        report = mutation_survivor_report.render_report({"summary": {}, "survivors": []})
        self.assertIn("Governing issue: unknown", report)


if __name__ == "__main__":
    unittest.main()
