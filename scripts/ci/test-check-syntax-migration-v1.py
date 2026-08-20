#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Callable


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-syntax-migration-v1.py"
spec = importlib.util.spec_from_file_location("check_syntax_migration_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class SyntaxMigrationContractTests(unittest.TestCase):
    def copy_path(self, root: Path, relative: Path) -> None:
        source = ROOT / relative
        target = root / relative
        if target.exists():
            return
        if source.is_dir():
            target.mkdir(parents=True, exist_ok=True)
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)

    def copy_contract(self, root: Path) -> dict:
        for relative in [
            checker.SCHEMA,
            checker.SNAPSHOT,
            checker.SYNTAX_SOURCE,
            checker.SYNTAX_TESTS,
            checker.FIXTURE_TESTS,
        ]:
            self.copy_path(root, relative)
        shutil.copytree(ROOT / checker.FIXTURE_ROOT, root / checker.FIXTURE_ROOT, dirs_exist_ok=True)
        snapshot = json.loads((ROOT / checker.SNAPSHOT).read_text(encoding="utf-8"))
        references = set(snapshot["current_floor"]["bootstrap_evidence"])
        for fixture in snapshot["fixtures"]:
            document = json.loads((ROOT / fixture["fixture_file"]).read_text(encoding="utf-8"))
            references.update(document["evidence"])
        for value in references:
            self.copy_path(root, checker.evidence_path(value))
        return snapshot

    def reject_snapshot(self, mutate: Callable[[dict], None]) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            snapshot = self.copy_contract(root)
            mutate(snapshot)
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_contract_passes(self) -> None:
        self.assertEqual(
            checker.validate_contract(ROOT),
            {
                "schema": "axiom.compiler.syntax_migration.v1",
                "ok": True,
                "fixtures": 17,
                "bootstrap_pass": 8,
                "target_gaps": 12,
                "cutover_permitted": False,
            },
        )

    def test_cli_is_deterministic_and_root_is_explicit(self) -> None:
        command = [sys.executable, str(CHECKER), "--root", str(ROOT), "--json"]
        first = subprocess.run(command, check=True, capture_output=True, text=True, cwd=ROOT)
        second = subprocess.run(command, check=True, capture_output=True, text=True, cwd=ROOT.parent)
        self.assertEqual(first.stdout, second.stdout)

    def test_rejects_completion_claims(self) -> None:
        for field in [
            "runtime_origin_source_proven",
            "axiom_package_present",
            "rust_path_disable_proven",
            "differential_parity_present",
            "canonical_axiom_node_ids",
        ]:
            with self.subTest(field=field):
                self.reject_snapshot(lambda value, field=field: value["current_floor"].update({field: True}))
        self.reject_snapshot(lambda value: value["cutover"].update(permitted=True))

    def test_rejects_omitted_issue_gates(self) -> None:
        self.reject_snapshot(lambda value: value["dependency_issues"].remove(1427))
        self.reject_snapshot(lambda value: value["dependency_issues"].remove(1468))
        self.reject_snapshot(lambda value: value["dependency_issues"].remove(1473))
        self.reject_snapshot(lambda value: value["entry_gates"].pop())
        self.reject_snapshot(
            lambda value: value["entry_gates"].__setitem__(
                1,
                {
                    "id": "maintainer_cutover_approval",
                    "issue": 1468,
                    "status": "blocked",
                    "required_proof": "duplicate gate",
                },
            )
        )
        self.reject_snapshot(lambda value: value["cutover"]["required_entry_gates"].remove("maintainer_cutover_approval"))

    def test_rejects_missing_target_gap(self) -> None:
        self.reject_snapshot(lambda value: value["current_floor"]["target_gaps"].pop())

    def test_rejects_missing_cutover_proof_for_fixture_or_declared_gap(self) -> None:
        self.reject_snapshot(lambda value: value["cutover"]["required_proofs"].remove("doc_comment_trivia_parity_passes"))
        self.reject_snapshot(lambda value: value["cutover"]["required_proofs"].remove("macro_limit_ceiling_vectors_pass"))

    def test_rejects_boolean_constants_encoded_as_integers(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["spans"].update(start_inclusive=1))

    def test_rejects_gap_reclassified_as_bootstrap_pass(self) -> None:
        self.reject_snapshot(lambda value: value["fixtures"][8].update(status="bootstrap_pass"))

    def test_rejects_mutated_structured_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            path = root / checker.FIXTURE_ROOT / "runtime-same-binary-ab.json"
            document = json.loads(path.read_text(encoding="utf-8"))
            document["input"]["artifact_sha256"] = "0" * 64
            path.write_text(json.dumps(document), encoding="utf-8")
            with self.assertRaisesRegex(checker.ContractError, "unproved target gap"):
                checker.validate_contract(root)

    def test_rejects_absolute_evidence_path(self) -> None:
        self.reject_snapshot(lambda value: value["current_floor"]["bootstrap_evidence"].__setitem__(0, "/etc/passwd"))

    def test_rejects_parent_traversal_evidence_path(self) -> None:
        self.reject_snapshot(lambda value: value["current_floor"]["bootstrap_evidence"].__setitem__(0, "../outside"))

    def test_rejects_symlink_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            snapshot = self.copy_contract(root)
            link = root / "evidence-link"
            os.symlink(root / checker.SYNTAX_SOURCE, link)
            snapshot["current_floor"]["bootstrap_evidence"][0] = "evidence-link"
            snapshot["current_floor"]["bootstrap_evidence"].sort()
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaisesRegex(checker.ContractError, "unsafe"):
                checker.validate_contract(root)

    def test_rejects_directory_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            snapshot = self.copy_contract(root)
            (root / "directory-evidence").mkdir()
            snapshot["current_floor"]["bootstrap_evidence"][0] = "directory-evidence"
            snapshot["current_floor"]["bootstrap_evidence"].sort()
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaisesRegex(checker.ContractError, "regular file"):
                checker.validate_contract(root)

    def test_rejects_oversized_anchored_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            snapshot = self.copy_contract(root)
            oversized = root / "oversized.txt"
            oversized.write_bytes(b"x" * (checker.MAX_EVIDENCE_BYTES + 1))
            snapshot["current_floor"]["bootstrap_evidence"][0] = "oversized.txt::x"
            snapshot["current_floor"]["bootstrap_evidence"].sort()
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaisesRegex(checker.ContractError, "exceeds"):
                checker.validate_contract(root)

    def test_rejects_missing_evidence_anchor(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            path = root / checker.FIXTURE_ROOT / "bootstrap-macro-provenance.json"
            document = json.loads(path.read_text(encoding="utf-8"))
            document["evidence"] = [
                "stage1/crates/axiomc/tests/support/lib_unit.rs::removed_macro_test"
            ]
            path.write_text(json.dumps(document), encoding="utf-8")
            with self.assertRaisesRegex(checker.ContractError, "anchor is missing"):
                checker.validate_contract(root)

    def test_rejects_missing_bootstrap_fixture_test(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            path = root / checker.FIXTURE_TESTS
            source = path.read_text(encoding="utf-8").replace(
                "bootstrap_node_identity_matches_fixture",
                "removed_node_identity_fixture_test",
            )
            path.write_text(source, encoding="utf-8")
            with self.assertRaisesRegex(checker.ContractError, "evidence anchor is missing"):
                checker.validate_contract(root)


if __name__ == "__main__":
    unittest.main()
