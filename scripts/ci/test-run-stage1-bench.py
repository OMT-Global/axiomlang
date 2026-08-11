#!/usr/bin/env python3
import importlib.util
import json
import unittest
from pathlib import Path
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("run-stage1-bench.py")
SPEC = importlib.util.spec_from_file_location("run_stage1_bench", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class Stage1BenchHarnessTests(unittest.TestCase):
    def test_benchmark_command_requests_execution_report(self):
        payload = {
            "schema_version": "axiom.stage1.bench.v1",
            "benches": [{"id": "src/example_bench", "ok": True}],
            "failed": 0,
        }
        with patch.object(
            MODULE,
            "run_timed",
            return_value=(12.5, json.dumps(payload)),
        ) as run_timed:
            report = MODULE.measure_benchmark_entrypoints(3)

        command = run_timed.call_args.args[0]
        self.assertEqual(command[-1], "--json")
        self.assertIn("bench", command)
        self.assertIn("--iterations", command)
        self.assertIn("3", command)
        self.assertEqual(report["report"], payload)
        self.assertEqual(report["elapsed_ms"], 12.5)

    def test_benchmark_command_rejects_failed_entrypoints(self):
        payload = {
            "schema_version": "axiom.stage1.bench.v1",
            "benches": [{"id": "src/example_bench", "ok": False}],
            "failed": 1,
        }
        with patch.object(
            MODULE,
            "run_timed",
            return_value=(1.0, json.dumps(payload)),
        ), self.assertRaisesRegex(SystemExit, "failed benchmark"):
            MODULE.measure_benchmark_entrypoints(1)

    def test_benchmark_command_rejects_missing_entrypoints(self):
        payload = {
            "schema_version": "axiom.stage1.bench.v1",
            "benches": [],
            "failed": 0,
        }
        with patch.object(
            MODULE,
            "run_timed",
            return_value=(1.0, json.dumps(payload)),
        ), self.assertRaisesRegex(SystemExit, "no benchmark entrypoints"):
            MODULE.measure_benchmark_entrypoints(1)

    def test_benchmark_command_rejects_non_object_reports(self):
        with patch.object(
            MODULE,
            "run_timed",
            return_value=(1.0, json.dumps(["not", "a", "report"])),
        ), self.assertRaisesRegex(SystemExit, "non-object report"):
            MODULE.measure_benchmark_entrypoints(1)


if __name__ == "__main__":
    unittest.main()
