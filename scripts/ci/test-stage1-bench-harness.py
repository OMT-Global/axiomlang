#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("run-stage1-bench-harness.py")
SPEC = importlib.util.spec_from_file_location("stage1_bench_harness", SCRIPT)
assert SPEC is not None
assert SPEC.loader is not None
stage1_bench_harness = importlib.util.module_from_spec(SPEC)
sys.modules["stage1_bench_harness"] = stage1_bench_harness
SPEC.loader.exec_module(stage1_bench_harness)


class Stage1BenchHarnessTests(unittest.TestCase):
    def test_command_for_phase_uses_stable_relative_project_path(self) -> None:
        project = stage1_bench_harness.REPO_ROOT / "stage1/examples/hello"
        command = stage1_bench_harness.command_for_phase(
            stage1_bench_harness.Phase("check", ("check", "{project}", "--json")),
            project,
        )
        self.assertEqual(command[1:], ["check", "stage1/examples/hello", "--json"])

    def test_median_handles_even_and_odd_samples(self) -> None:
        self.assertEqual(stage1_bench_harness.median_ms([1.0, 5.0, 3.0]), 3.0)
        self.assertEqual(stage1_bench_harness.median_ms([10.0, 20.0]), 15.0)


if __name__ == "__main__":
    unittest.main()
