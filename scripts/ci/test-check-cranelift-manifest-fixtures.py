#!/usr/bin/env python3
"""Positive and mutation controls for the narrow legacy-fixture exception."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("guard", Path(__file__).with_name("check-cranelift-manifest-fixtures.py"))
GUARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD)
SOURCE = (ROOT / "stage1/crates/axiomc/tests/cranelift_backend.rs").read_bytes()


class FixtureGuardTests(unittest.TestCase):
    def test_original_negative_fixture_is_permitted(self):
        self.assertEqual(SOURCE.count(GUARD.LEGACY), 1)
        self.assertEqual(GUARD.check(SOURCE), [])

    def test_real_legacy_fixture_before_and_after_contract_is_rejected(self):
        violation = b'\nfn regression() { let manifest = "[unsafe_rationale]"; }\n'
        for source in (violation + SOURCE, SOURCE + violation):
            with self.subTest(location=source[:30]):
                errors = GUARD.check(source)
                self.assertTrue(any("outside sealed rejection test" in e for e in errors), errors)

    def test_valid_capability_fixture_stays_allowed(self):
        self.assertEqual(GUARD.check(SOURCE + b'\n// [capabilities.unsafe_rationale]\n'), [])

    def test_removing_rejection_assertion_is_rejected(self):
        mutated = SOURCE.replace(b'assert!(rejected.is_err(),', b'assert!(true,', 1)
        self.assertNotEqual(mutated, SOURCE)
        self.assertTrue(GUARD.check(mutated))

    def test_weakening_parser_helper_is_rejected(self):
        mutated = SOURCE.replace(b'axiomc::manifest::parse_manifest_exact(content, path)', b'Ok::<(), ()>(())', 1)
        self.assertNotEqual(mutated, SOURCE)
        self.assertTrue(GUARD.check(mutated))

    def test_missing_duplicate_and_fake_contract_are_rejected(self):
        mutations = [
            SOURCE.replace(GUARD.START, b"", 1),
            SOURCE + GUARD.START,
            SOURCE.replace(GUARD.END, b"\nfn renamed_copy_fixture(", 1),
            SOURCE.replace(b"misplaced", b"changed", 1),
        ]
        for source in mutations:
            self.assertNotEqual(source, SOURCE)
            self.assertTrue(GUARD.check(source))

    def test_inserted_legacy_table_inside_exception_is_rejected(self):
        self.assertTrue(GUARD.check(SOURCE.replace(GUARD.START, GUARD.START + b'// [unsafe_rationale]\n', 1)))


if __name__ == "__main__":
    unittest.main()
