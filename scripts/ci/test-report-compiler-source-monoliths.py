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
            hir_dir = source_root / "hir"
            hir_dir.mkdir()
            write_lines(hir_dir / "generics.rs", 2)
            write_lines(hir_dir / "model.rs", 1)
            write_lines(hir_dir / "types.rs", 1)

            report = compiler_source_monoliths.build_report(source_root, top=5)

        self.assertEqual(report["schema_version"], "axiom.compiler_source.monoliths.v0")
        self.assertEqual(report["collected_at"], "2026-06-21T10:00:00Z")
        self.assertEqual(report["summary"]["total_files"], 5)
        self.assertEqual(report["summary"]["total_lines"], 12)
        self.assertEqual(report["summary"]["largest_file_lines"], 5)
        self.assertEqual(report["summary"]["top_file_lines"], 12)
        self.assertEqual(report["summary"]["top_file_line_share"], 1.0)
        self.assertEqual(report["top_files"][0]["package_boundaries"], ["compiler.backend.native"])
        self.assertEqual(report["top_files"][1]["package_boundaries"], ["compiler.hir"])
        self.assertEqual(report["top_files"][2]["package_boundaries"], ["compiler.hir"])
        self.assertEqual(report["top_files"][3]["package_boundaries"], ["compiler.hir"])
        self.assertEqual(report["top_files"][4]["package_boundaries"], ["compiler.hir"])

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

    def test_check_ratchet_passes_when_current_counts_are_within_ceilings(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source_root = Path(tmp) / "src"
            source_root.mkdir()
            first = source_root / "cranelift_backend.rs"
            second = source_root / "hir.rs"
            write_lines(first, 5)
            write_lines(second, 3)
            report = compiler_source_monoliths.build_report(source_root, top=2)
            plan = Path(tmp) / "plan.md"
            plan.write_text(
                "## Ratchet Ceilings\n\n"
                "| Tracked item | Ceiling |\n"
                "| --- | ---: |\n"
                "| `summary.top_file_line_share` | 1.0000 |\n"
                "| `summary.top_file_lines` | 8 |\n"
                f"| `{first.as_posix()}` | 5 |\n"
                f"| `{second.as_posix()}` | 3 |\n",
                encoding="utf-8",
            )

            errors = compiler_source_monoliths.check_ratchet(report, plan)

        self.assertEqual(errors, [])

    def test_check_ratchet_fails_when_counts_rise_above_ceilings(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source_root = Path(tmp) / "src"
            source_root.mkdir()
            first = source_root / "cranelift_backend.rs"
            second = source_root / "hir.rs"
            write_lines(first, 5)
            write_lines(second, 3)
            report = compiler_source_monoliths.build_report(source_root, top=2)
            plan = Path(tmp) / "plan.md"
            plan.write_text(
                "## Ratchet Ceilings\n\n"
                "| Tracked item | Ceiling |\n"
                "| --- | ---: |\n"
                "| `summary.top_file_line_share` | 0.7000 |\n"
                "| `summary.top_file_lines` | 7 |\n"
                f"| `{first.as_posix()}` | 4 |\n"
                f"| `{second.as_posix()}` | 3 |\n",
                encoding="utf-8",
            )

            errors = compiler_source_monoliths.check_ratchet(report, plan)

        self.assertTrue(any("top file line share" in error for error in errors))
        self.assertTrue(any("top file lines" in error for error in errors))
        self.assertTrue(any("cranelift_backend.rs" in error for error in errors))

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

    def test_cli_check_ratchet_fails_when_plan_is_incomplete(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source_root = Path(tmp) / "src"
            source_root.mkdir()
            write_lines(source_root / "cranelift_backend.rs", 5)
            plan = Path(tmp) / "plan.md"
            plan.write_text("no ratchet table\n", encoding="utf-8")

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
                    "--check-ratchet",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )

        self.assertEqual(completed.returncode, 1)
        payload = json.loads(completed.stdout)
        self.assertFalse(payload["ratchet_check"]["passed"])
        self.assertIn("Ratchet Ceilings", payload["ratchet_check"]["errors"][0])


if __name__ == "__main__":
    unittest.main()
