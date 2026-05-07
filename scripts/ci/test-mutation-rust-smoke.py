#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("run-mutation-rust-smoke.py")
SPEC = importlib.util.spec_from_file_location("mutation_rust_smoke", SCRIPT)
assert SPEC is not None
assert SPEC.loader is not None
mutation_rust_smoke = importlib.util.module_from_spec(SPEC)
sys.modules["mutation_rust_smoke"] = mutation_rust_smoke
SPEC.loader.exec_module(mutation_rust_smoke)


class MutationRustSmokeTests(unittest.TestCase):
    def test_apply_mutation_replaces_only_first_anchor(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            path = Path(temp_name) / "sample.rs"
            path.write_text("alpha beta alpha\n")
            original = mutation_rust_smoke.apply_mutation(path, "alpha", "omega")
            self.assertEqual(original, "alpha beta alpha\n")
            self.assertEqual(path.read_text(), "omega beta alpha\n")

    def test_profile_covers_expected_stage1_areas(self) -> None:
        areas = {mutant.area for mutant in mutation_rust_smoke.MUTANTS}
        self.assertEqual(areas, {"parser", "hir", "mir", "codegen"})


if __name__ == "__main__":
    unittest.main()
