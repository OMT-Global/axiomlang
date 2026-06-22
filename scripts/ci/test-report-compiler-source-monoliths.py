#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("report-compiler-source-monoliths.py")
SPEC = importlib.util.spec_from_file_location("compiler_source_monoliths", SCRIPT)
assert SPEC is not None
assert SPEC.loader is not None
compiler_source_monoliths = importlib.util.module_from_spec(SPEC)
sys.modules["compiler_source_monoliths"] = compiler_source_monoliths
SPEC.loader.exec_module(compiler_source_monoliths)


def write_lines(path: Path, count: int) -> None:
    path.write_text("".join(f"line {index}\n" for index in range(count)), encoding="utf-8")


class CompilerSourceMonolithTests(unittest.TestCase):
    def setUp(self) -> None:
        self.previous_collected_at = os.environ.get("AXIOM_COMPILER_SOURCE_COLLECTED_AT")
        os.environ["AXIOM_COMPILER_SOURCE_COLLECTED_AT"] = "2026-06-21T10:00:00Z"

    def tearDown(self) -> None:
        if self.previous_collected_at is None:
            os.environ.pop("AXIOM_COMPILER_SOURCE_COLLECTED_AT", None)
        else:
            os.environ["AXIOM_COMPILER_SOURCE_COLLECTED_AT"] = self.previous_collected_at

    def test_report_counts_top_files_and_package_boundaries(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source_root = Path(tmp) / "src"
            source_root.mkdir()
            write_lines(source_root / "cranelift_backend.rs", 5)
            write_lines(source_root / "hir.rs", 3)
            write_lines(source_root / "syntax.rs", 2)

            report = compiler_source_monoliths.build_report(source_root, top=2)

        self.assertEqual(report["schema_version"], "axiom.compiler_source.monoliths.v0")
        self.assertEqual(report["collected_at"], "2026-06-21T10:00:00Z")
        self.assertEqual(report["summary"]["total_files"], 3)
        self.assertEqual(report["summary"]["total_lines"], 10)
        self.assertEqual(report["summary"]["largest_file_lines"], 5)
        self.assertEqual(report["summary"]["top_file_lines"], 8)
        self.assertEqual(report["summary"]["top_file_line_share"], 0.8)
        self.assertEqual(report["top_files"][0]["package_boundaries"], ["compiler.backend.native"])
        self.assertEqual(report["top_files"][1]["package_boundaries"], ["compiler.hir"])

    def test_check_plan_requires_top_file_and_boundary_mentions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source_root = Path(tmp) / "src"
            source_root.mkdir()
            write_lines(source_root / "cranelift_backend.rs", 5)
            write_lines(source_root / "hir.rs", 3)
            plan = Path(tmp) / "plan.md"
            plan.write_text(
                "`cranelift_backend.rs` maps to `compiler.backend.native`\n"
                "`hir.rs` maps to `compiler.hir`\n",
                encoding="utf-8",
            )

            report = compiler_source_monoliths.build_report(source_root, top=2)
            errors = compiler_source_monoliths.check_plan(report, plan)

        self.assertEqual(errors, [])

    def test_cli_check_plan_fails_when_plan_is_incomplete(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source_root = Path(tmp) / "src"
            source_root.mkdir()
            write_lines(source_root / "cranelift_backend.rs", 5)
            plan = Path(tmp) / "plan.md"
            plan.write_text("missing mapping\n", encoding="utf-8")

            completed = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--source-root",
                    str(source_root),
                    "--plan",
                    str(plan),
                    "--top",
                    "1",
                    "--check-plan",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )

        self.assertEqual(completed.returncode, 1)
        payload = json.loads(completed.stdout)
        self.assertFalse(payload["plan_check"]["passed"])
        self.assertIn("cranelift_backend.rs", payload["plan_check"]["errors"][0])


if __name__ == "__main__":
    unittest.main()
