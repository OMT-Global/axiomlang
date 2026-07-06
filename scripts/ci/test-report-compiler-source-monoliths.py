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
            write_lines(source_root / "diagnostics.rs", 1)
            write_lines(source_root / "hir.rs", 3)
            hir_dir = source_root / "hir"
            hir_dir.mkdir()
            write_lines(hir_dir / "generics.rs", 2)
            write_lines(hir_dir / "capabilities.rs", 1)
            write_lines(hir_dir / "definitions.rs", 1)
            write_lines(hir_dir / "diagnostics.rs", 1)
            write_lines(hir_dir / "expressions.rs", 1)
            write_lines(hir_dir / "model.rs", 1)
            write_lines(hir_dir / "ownership.rs", 1)
            write_lines(hir_dir / "properties.rs", 1)
            write_lines(hir_dir / "reachability.rs", 1)
            write_lines(hir_dir / "signatures.rs", 1)
            write_lines(hir_dir / "symbols.rs", 1)
            write_lines(hir_dir / "types.rs", 1)

            report = compiler_source_monoliths.build_report(source_root, top=15)

        self.assertEqual(report["schema_version"], "axiom.compiler_source.monoliths.v0")
        self.assertEqual(report["collected_at"], "2026-06-21T10:00:00Z")
        self.assertEqual(report["summary"]["total_files"], 15)
        self.assertEqual(report["summary"]["total_lines"], 22)
        self.assertEqual(report["summary"]["largest_file_lines"], 5)
        self.assertEqual(report["summary"]["top_file_lines"], 22)
        self.assertEqual(report["summary"]["top_file_line_share"], 1.0)
        boundaries_by_suffix = {
            Path(item["path"]).as_posix().removeprefix(source_root.as_posix() + "/"): item[
                "package_boundaries"
            ]
            for item in report["top_files"]
        }
        self.assertEqual(
            boundaries_by_suffix["cranelift_backend.rs"], ["compiler.backend.native"]
        )
        self.assertEqual(boundaries_by_suffix["diagnostics.rs"], ["compiler.diagnostics"])
        for path in [
            "hir.rs",
            "hir/capabilities.rs",
            "hir/definitions.rs",
            "hir/diagnostics.rs",
            "hir/expressions.rs",
            "hir/generics.rs",
            "hir/model.rs",
            "hir/ownership.rs",
            "hir/properties.rs",
            "hir/reachability.rs",
            "hir/signatures.rs",
            "hir/symbols.rs",
            "hir/types.rs",
        ]:
            self.assertEqual(boundaries_by_suffix[path], ["compiler.hir"])

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

    def test_check_ratchet_uses_unrounded_share_against_ceiling(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            plan = Path(tmp) / "plan.md"
            plan.write_text(
                "## Ratchet Ceilings\n"
                "| `summary.top_file_line_share` | 0.8539 |\n"
                "| `summary.top_file_lines` | 100 |\n"
                "| `stage1/crates/axiomc/src/hir.rs` | 50 |\n",
                encoding="utf-8",
            )
            report = {
                "summary": {
                    "top_file_line_share": 0.85395,
                    "top_file_lines": 90,
                },
                "top_files": [
                    {"path": "stage1/crates/axiomc/src/hir.rs", "lines": 40},
                ],
            }

            errors = compiler_source_monoliths.check_ratchet(report, plan)

        self.assertEqual(
            errors,
            ["top file line share 0.8539 exceeds ratchet ceiling 0.8539"],
        )

    def test_cli_check_ratchet_uses_checkout_root_for_paths_and_counts(self) -> None:
        # Regression for PR #1377 review: PR Fast CI runs the trusted script
        # from one checkout while source/plan live in the PR data checkout.
        # The script (from its own real location) must relativize top-file
        # paths against --checkout-root and read non-top tracked file counts
        # from that same root, so repo-relative ceilings keep matching.
        with tempfile.TemporaryDirectory() as tmp:
            data_root = Path(tmp) / "data"
            source_root = data_root / "stage1/crates/axiomc/src"
            source_root.mkdir(parents=True)
            write_lines(source_root / "cranelift_backend.rs", 5)
            write_lines(source_root / "hir.rs", 3)
            # main.rs is outside the top-2 window, so its count is resolved
            # via --checkout-root rather than the report's top-file list.
            write_lines(source_root / "main.rs", 2)
            plan = data_root / "docs/compiler-source-decomposition-plan.md"
            plan.parent.mkdir(parents=True)
            plan.write_text(
                "`stage1/crates/axiomc/src/cranelift_backend.rs` maps to "
                "`compiler.backend.native`\n"
                "`stage1/crates/axiomc/src/hir.rs` maps to `compiler.hir`\n\n"
                "## Ratchet Ceilings\n\n"
                "| Tracked item | Ceiling |\n"
                "| --- | ---: |\n"
                "| `summary.top_file_line_share` | 1.0000 |\n"
                "| `summary.top_file_lines` | 8 |\n"
                "| `stage1/crates/axiomc/src/cranelift_backend.rs` | 5 |\n"
                "| `stage1/crates/axiomc/src/hir.rs` | 3 |\n"
                "| `stage1/crates/axiomc/src/main.rs` | 2 |\n",
                encoding="utf-8",
            )

            completed = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--checkout-root",
                    str(data_root),
                    "--top",
                    "2",
                    "--check-plan",
                    "--check-ratchet",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )

        self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)
        payload = json.loads(completed.stdout)
        self.assertTrue(payload["plan_check"]["passed"], payload["plan_check"])
        self.assertTrue(payload["ratchet_check"]["passed"], payload["ratchet_check"])
        top_paths = {item["path"] for item in payload["top_files"]}
        self.assertIn("stage1/crates/axiomc/src/cranelift_backend.rs", top_paths)
        self.assertEqual(
            payload["source_root"], "stage1/crates/axiomc/src", payload["source_root"]
        )

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
