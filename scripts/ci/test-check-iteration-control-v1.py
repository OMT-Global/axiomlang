#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path
from typing import Callable


TRUSTED_ROOT = Path(__file__).resolve().parents[2]
CHECKER = TRUSTED_ROOT / "scripts/ci/check-iteration-control-v1.py"
spec = importlib.util.spec_from_file_location("check_iteration_control_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class IterationControlTests(unittest.TestCase):
    source_root = TRUSTED_ROOT

    def copy_file(self, root: Path, relative: os.PathLike[str] | str) -> None:
        target = root / os.fspath(relative)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(checker.safe_read_bytes(self.source_root, relative))

    def copy_contract(self, root: Path) -> dict:
        required = [
            checker.SCHEMA,
            checker.SNAPSHOT,
            checker.PRODUCTION_READINESS,
            *checker.SOURCE_MARKERS,
            *checker.TARGET_GAP_FIXTURES,
        ]
        for relative in required:
            self.copy_file(root, relative)
        snapshot = checker.load(self.source_root, checker.SNAPSHOT)
        evidence = set(snapshot["current_floor"]["bootstrap_evidence"])
        for fixture in snapshot["fixtures"]:
            evidence.update(checker.evidence_path(value) for value in fixture["evidence"])
        for value in evidence:
            target = root / value
            if not target.exists():
                self.copy_file(root, value)
        return snapshot

    def reject_snapshot(self, mutate: Callable[[dict], None]) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            snapshot = self.copy_contract(root)
            mutate(snapshot)
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def reject_target_fixture(self, index: int, mutate: Callable[[dict], None]) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            relative = checker.TARGET_GAP_FIXTURES[index]
            fixture = checker.load(root, relative)
            mutate(fixture)
            (root / relative).write_text(json.dumps(fixture), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def assert_unsafe_read(self, root: Path, relative: os.PathLike[str] | str) -> None:
        with self.assertRaises(checker.ContractError):
            checker.safe_read_text(root, relative)

    def test_contract_passes(self) -> None:
        self.assertEqual(
            checker.validate_contract(self.source_root),
            {
                "schema": "axiom.iteration_control.v1",
                "ok": True,
                "fixtures": 16,
                "bootstrap_pass": 4,
                "target_gaps": 12,
                "target_gap_fixtures": 4,
                "ready": False,
                "readiness_promotable": False,
            },
        )

    def test_cli_is_deterministic_and_root_selectable(self) -> None:
        command = [sys.executable, str(CHECKER), "--root", str(self.source_root), "--json"]
        first = subprocess.run(command, check=True, capture_output=True, text=True)
        second = subprocess.run(command, check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)

    def test_rejects_each_completion_claim(self) -> None:
        for field in checker.COMPLETION_FIELDS:
            with self.subTest(field=field):
                self.reject_snapshot(lambda value, field=field: value["current_floor"].update({field: True}))

    def test_rejects_global_ready_claim(self) -> None:
        self.reject_snapshot(lambda value: value.update(ready=True))

    def test_rejects_readiness_promotion(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(readiness_promotable=True))

    def test_rejects_shallow_nested_proof(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(minimum_nested_depth=1))

    def test_rejects_missing_collection_kind(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["collection_kinds"].pop())

    def test_rejects_missing_iteration_mode(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["iteration_modes"].pop())

    def test_rejects_missing_protocol_operation(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["protocol_operations"].pop())

    def test_rejects_order_policy_drift(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["order_rules"].pop())

    def test_rejects_terminal_edge_drift(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["terminal_edge_rules"].pop())

    def test_rejects_mutation_policy_drift(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["mutation_rules"].pop())

    def test_rejects_prefetch_or_outstanding_item_relaxation(self) -> None:
        for field, replacement in (("max_prefetched_items_per_iterator", 1), ("max_outstanding_items_per_iterator", 2)):
            with self.subTest(field=field):
                self.reject_snapshot(lambda value, field=field, replacement=replacement: value["target_contract"]["flow_control_bounds"].update({field: replacement}))

    def test_rejects_runtime_receipt_weakening(self) -> None:
        mutations = [
            lambda value: value["runtime_receipt_contract"].update(proof_executed=True),
            lambda value: value["runtime_receipt_contract"].update(minimum_post_build_runs=1),
            lambda value: value["runtime_receipt_contract"]["required_invariants"].remove("build_count_equals_one"),
            lambda value: value["runtime_receipt_contract"]["rejected_substitutions"].remove("compiler_known_value_substitution"),
            lambda value: value["runtime_receipt_contract"].update(declarations_are_proof=True),
        ]
        for index, mutate in enumerate(mutations):
            with self.subTest(index=index):
                self.reject_snapshot(mutate)

    def test_rejects_dynamic_dispatch_promotion(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"].update(dynamic_dispatch_policy="supported"))

    def test_rejects_gap_as_bootstrap_pass(self) -> None:
        self.reject_snapshot(lambda value: value["fixtures"][0].update(status="bootstrap_pass"))

    def test_rejects_missing_evidence_anchor(self) -> None:
        self.reject_snapshot(lambda value: value["fixtures"][4].update(evidence=["stage1/conformance/pass/runtime_loop_bodies/src/main_test.ax::removed_marker"]))

    def test_rejects_host_capture(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["protocol_operations"][0].update(semantics="use Rust iterator layout"))

    def test_rejects_structured_target_gap_mutations(self) -> None:
        mutations = [
            (0, lambda value: value["requirements"][0].update(contract="cleanup may be skipped")),
            (1, lambda value: value["negative_cases"][1].update(expected="mutation may partially apply")),
            (2, lambda value: value["requirements"][0].update(contract="prefetch is unbounded")),
            (3, lambda value: value["negative_cases"][0].update(expected="accepted as runtime proof")),
        ]
        for index, mutate in mutations:
            with self.subTest(fixture=index):
                self.reject_target_fixture(index, mutate)

    def test_rejects_production_readiness_overclaim(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            payload = checker.load(root, checker.PRODUCTION_READINESS)
            row = next(item for item in payload["rows"] if item["id"] == "iteration_control")
            row.update(currentTier="static_spike", status="partial")
            (root / checker.PRODUCTION_READINESS).write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_rejects_missing_source_marker(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            path = root / checker.FOR_FAILURE
            source = checker.safe_read_text(root, checker.FOR_FAILURE)
            path.write_text(source.replace("does not support `for` loops yet", "removed iteration gap"), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_rejects_unsafe_relative_paths(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "safe.txt").write_text("safe", encoding="utf-8")
            for relative in ("", ".", "..", "a/./b", "a/../b", "a//b", "a/", "a\\..\\b", "a\\\\b", "/etc/passwd", "C:\\Windows\\system.ini", "C:/Windows/system.ini", "\\\\server\\share\\file", "bad\x00name"):
                with self.subTest(relative=repr(relative)):
                    self.assert_unsafe_read(root, relative)

    @unittest.skipUnless(hasattr(os, "symlink"), "symlink support required")
    def test_rejects_intermediate_and_final_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            real = root / "real"
            real.mkdir()
            (real / "value.txt").write_text("value", encoding="utf-8")
            os.symlink(real, root / "linked-dir")
            os.symlink(real / "value.txt", root / "linked-file")
            self.assert_unsafe_read(root, "linked-dir/value.txt")
            self.assert_unsafe_read(root, "linked-file")

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFO support required")
    def test_rejects_directories_and_fifos_without_blocking(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "directory").mkdir()
            os.mkfifo(root / "fifo")
            self.assert_unsafe_read(root, "directory")
            self.assert_unsafe_read(root, "fifo")

    def test_rejects_oversize_and_invalid_utf8(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "oversize").write_bytes(b"x" * (checker.MAX_READ_BYTES + 1))
            (root / "invalid-utf8").write_bytes(b"\xff")
            self.assert_unsafe_read(root, "oversize")
            self.assert_unsafe_read(root, "invalid-utf8")

    def test_rejects_device_file_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "device").write_text("not actually a device", encoding="utf-8")
            real_fstat = os.fstat

            def report_device_for_regular_file(descriptor: int) -> os.stat_result:
                result = real_fstat(descriptor)
                if stat.S_ISREG(result.st_mode):
                    fields = list(result)
                    fields[0] = stat.S_IFCHR
                    return os.stat_result(fields)
                return result

            with mock.patch.object(checker.os, "fstat", side_effect=report_device_for_regular_file):
                self.assert_unsafe_read(root, "device")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run Iteration and Loop Control v1 checker regressions.")
    parser.add_argument("--root", type=Path, default=TRUSTED_ROOT)
    args = parser.parse_args(argv)
    IterationControlTests.source_root = args.root
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(IterationControlTests)
    return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
