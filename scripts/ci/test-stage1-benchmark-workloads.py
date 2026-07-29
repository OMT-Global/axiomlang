#!/usr/bin/env python3
import unittest
from pathlib import Path

from stage1_benchmark_workloads import (
    build_payload_matches_expected_lowering,
    load_workloads,
    semantic_outputs_match,
)

ROOT = Path(__file__).resolve().parents[2]


class Stage1BenchmarkWorkloadTests(unittest.TestCase):
    def test_manifest_names_real_executable_workloads_and_references(self):
        workloads = load_workloads(ROOT)
        self.assertEqual(
            [
                (item.name, item.kind, item.expected_lowering_mode)
                for item in workloads
            ],
            [
                ("hello", "compute", "direct_native_runtime"),
                ("stdlib_time", "io", "direct_native_runtime"),
                ("stdlib_sync", "concurrency", "bounded_static_output"),
            ],
        )
        names = {item.name for item in workloads}
        self.assertNotIn(
            "capabilities",
            names,
            "the broad capability sample is intentionally fail-closed",
        )
        self.assertNotIn(
            "stdlib_async",
            names,
            "async runtime execution is not implemented",
        )

    def test_semantic_output_parity_requires_all_three_exact_results(self):
        matching = {
            "axiom": (0, "same\n"),
            "go": (0, "same\n"),
            "rust": (0, "same\n"),
        }
        self.assertTrue(semantic_outputs_match(matching))
        self.assertFalse(
            semantic_outputs_match({**matching, "rust": (0, "different\n")})
        )
        self.assertFalse(
            semantic_outputs_match({**matching, "go": (1, "same\n")})
        )
        self.assertFalse(
            semantic_outputs_match(
                {"axiom": matching["axiom"], "go": matching["go"]}
            )
        )

    def test_expected_lowering_mode_is_fail_closed(self):
        workloads = {item.name: item for item in load_workloads(ROOT)}
        direct_payload = {
            "ok": True,
            "binary": "dist/example",
            "generated_rust": None,
            "lowering": {
                "lowering_mode": "direct_native_runtime",
                "execution_mode": "direct_native_runtime",
                "direct_native_runtime": True,
                "known_value_static_folds": False,
                "legacy_fallback_attempted": False,
            },
        }
        static_payload = {
            "ok": True,
            "binary": "dist/example",
            "generated_rust": None,
            "lowering": {
                "lowering_mode": "bounded_static_output",
                "execution_mode": "bounded_static_output",
                "direct_native_runtime": False,
                "known_value_static_folds": True,
                "legacy_fallback_attempted": False,
            },
        }
        self.assertTrue(
            build_payload_matches_expected_lowering(
                workloads["stdlib_time"], direct_payload
            )
        )
        self.assertTrue(
            build_payload_matches_expected_lowering(
                workloads["stdlib_sync"], static_payload
            )
        )
        self.assertFalse(
            build_payload_matches_expected_lowering(
                workloads["stdlib_sync"], direct_payload
            )
        )
        self.assertFalse(
            build_payload_matches_expected_lowering(
                workloads["stdlib_time"],
                {**direct_payload, "generated_rust": "dist/generated.rs"},
            )
        )


if __name__ == "__main__":
    unittest.main()
