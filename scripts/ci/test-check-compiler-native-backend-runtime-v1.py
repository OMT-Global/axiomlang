#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Callable


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-compiler-native-backend-runtime-v1.py"
spec = importlib.util.spec_from_file_location("check_compiler_native_backend_runtime_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class CompilerNativeBackendRuntimeTests(unittest.TestCase):
    def copy_contract(self, root: Path) -> dict:
        required = [checker.SCHEMA, checker.SNAPSHOT, checker.PRODUCTION_READINESS, *checker.SOURCE_MARKERS]
        for relative in required:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)
        snapshot = json.loads((ROOT / checker.SNAPSHOT).read_text(encoding="utf-8"))
        evidence = set(snapshot["current_floor"]["bootstrap_evidence"])
        for fixture in snapshot["fixtures"]:
            evidence.update(checker.evidence_path(value).as_posix() for value in fixture["evidence"])
        for value in evidence:
            relative, target = Path(value), root / value
            if not target.exists():
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / relative, target)
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
                "schema": "axiom.compiler_native_backend_runtime.v1",
                "ok": True,
                "fixtures": 18,
                "bootstrap_pass": 6,
                "target_gaps": 12,
                "backend_dispatch_authorized": False,
                "semantic_cutover_authorized": False,
                "legacy_retirement_authorized": False,
                "readiness_promotable": False,
            },
        )

    def test_cli_is_deterministic(self) -> None:
        command = [sys.executable, str(CHECKER), "--json"]
        first = subprocess.run(command, check=True, capture_output=True, text=True)
        second = subprocess.run(command, check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)

    def test_rejects_each_completion_claim(self) -> None:
        for field in checker.COMPLETION_FIELDS:
            with self.subTest(field=field):
                self.reject_snapshot(lambda value, field=field: value["current_floor"].update({field: True}))

    def test_rejects_backend_dispatch_authorization(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(backend_dispatch_authorized=True))

    def test_rejects_semantic_cutover_authorization(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(semantic_cutover_authorized=True))

    def test_rejects_legacy_retirement_authorization(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(legacy_retirement_authorized=True))

    def test_rejects_readiness_promotion(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(readiness_promotable=True))

    def test_rejects_boolean_constant_replaced_by_integer(self) -> None:
        self.reject_snapshot(
            lambda value: value["target_contract"]["runtime_sensitivity"].update(runtime_origin_required=1)
        )

    def test_rejects_scaffold_boundary_removal(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(fixture_scaffolding_only=False))

    def test_rejects_dependency_gate_removal(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(dependencies_must_be_runtime_complete=False))

    def test_rejects_human_cutover_reassignment(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(human_cutover_issue=1474))

    def test_rejects_missing_semantic_input(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["semantic_inputs"].pop())

    def test_rejects_missing_unsupported_dimension(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["unsupported_dimensions"].pop())

    def test_rejects_rebuild_between_runtime_inputs(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["runtime_sensitivity"].update(rebuilds_between_inputs=1))

    def test_rejects_missing_build_purity_denial(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["build_purity"]["forbidden_runtime_authority"].pop())

    def test_rejects_gap_as_bootstrap_pass(self) -> None:
        self.reject_snapshot(lambda value: value["fixtures"][0].update(status="bootstrap_pass"))

    def test_rejects_missing_evidence_anchor(self) -> None:
        self.reject_snapshot(lambda value: value["fixtures"][0].update(evidence=["docs/compiler-native-backend-runtime-v1.md::removed_marker"]))

    def test_rejects_empty_evidence_anchor(self) -> None:
        self.reject_snapshot(lambda value: value["fixtures"][0].update(evidence=["docs/compiler-native-backend-runtime-v1.md::"]))

    def test_rejects_evidence_paths_outside_repository(self) -> None:
        for path_kind in ("absolute", "traversal", "symlink"):
            with self.subTest(path_kind=path_kind), tempfile.TemporaryDirectory() as temporary:
                base = Path(temporary)
                root = base / "repo"
                root.mkdir()
                snapshot = self.copy_contract(root)
                outside = base / "outside-evidence.txt"
                outside.write_text("outside-marker", encoding="utf-8")
                if path_kind == "absolute":
                    evidence = f"{outside}::outside-marker"
                elif path_kind == "traversal":
                    evidence = "../outside-evidence.txt::outside-marker"
                else:
                    link = root / "outside-link.txt"
                    link.symlink_to(outside)
                    evidence = "outside-link.txt::outside-marker"
                snapshot["fixtures"][0]["evidence"] = [evidence]
                (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
                with self.assertRaises(checker.ContractError):
                    checker.validate_contract(root)

    def test_rejects_coordinated_schema_and_snapshot_build_shortcut(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            snapshot = self.copy_contract(root)
            schema_path = root / checker.SCHEMA
            schema = json.loads(schema_path.read_text(encoding="utf-8"))
            schema["$defs"]["buildPurity"]["properties"]["generated_host_projection_required"]["const"] = True
            snapshot["target_contract"]["build_purity"]["generated_host_projection_required"] = True
            schema_path.write_text(json.dumps(schema), encoding="utf-8")
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_rejects_host_capture(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["semantic_inputs"].append("rust_backend_state"))

    def test_rejects_production_readiness_overclaim(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            payload = json.loads((root / checker.PRODUCTION_READINESS).read_text(encoding="utf-8"))
            row = next(item for item in payload["rows"] if item["id"] == "compiler_native_backend_source")
            row.update(currentTier="runtime_complete", status="implemented")
            (root / checker.PRODUCTION_READINESS).write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_rejects_missing_production_mutation_gate(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            payload = json.loads((root / checker.PRODUCTION_READINESS).read_text(encoding="utf-8"))
            row = next(item for item in payload["rows"] if item["id"] == "compiler_native_backend_source")
            row["validatingCommand"] = "make stage1-mir-backend-boundary"
            (root / checker.PRODUCTION_READINESS).write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)


if __name__ == "__main__":
    unittest.main()
