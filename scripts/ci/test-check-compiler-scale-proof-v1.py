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
CHECKER = ROOT / "scripts/ci/check-compiler-scale-proof-v1.py"
spec = importlib.util.spec_from_file_location("check_compiler_scale_proof_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class CompilerScaleProofTests(unittest.TestCase):
    def copy_contract(self, root: Path) -> dict:
        required = [checker.SCHEMA, checker.SNAPSHOT, checker.PRODUCTION_READINESS, checker.SELF_HOSTING_READINESS, *checker.SOURCE_MARKERS]
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
        self.assertEqual(checker.validate_contract(ROOT), {"schema": "axiom.compiler_scale_proof.v1", "ok": True, "fixtures": 20, "bootstrap_pass": 5, "target_gaps": 15, "workload_dispatch_authorized": False, "readiness_promotable": False})

    def test_cli_is_deterministic(self) -> None:
        command = [sys.executable, str(CHECKER), "--json"]
        first = subprocess.run(command, check=True, capture_output=True, text=True)
        second = subprocess.run(command, check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)

    def test_rejects_each_completion_claim(self) -> None:
        for field in checker.COMPLETION_FIELDS:
            with self.subTest(field=field):
                self.reject_snapshot(lambda value, field=field: value["current_floor"].update({field: True}))

    def test_rejects_readiness_promotion(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(readiness_promotable=True))

    def test_rejects_workload_dispatch_authorization(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(workload_dispatch_authorized=True))

    def test_rejects_boolean_constant_replaced_by_integer(self) -> None:
        self.reject_snapshot(
            lambda value: value["target_contract"]["runtime_sensitivity"].update(rebuilds_between_inputs=False)
        )

    def test_rejects_scaffold_boundary_removal(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(fixture_scaffolding_only=False))

    def test_rejects_dependency_gate_removal(self) -> None:
        self.reject_snapshot(lambda value: value["qualification"].update(dependencies_must_be_runtime_complete=False))

    def test_rejects_scale_floor_reduction(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["scale_floor"].update(minimum_axiom_lines=200))

    def test_rejects_missing_package_role(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["package_roles"].pop())

    def test_rejects_missing_command_surface(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["command_surfaces"].pop())

    def test_rejects_missing_runtime_origin(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["runtime_origins"].pop())

    def test_rejects_missing_fallback_denial(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["prohibited_fallbacks"].pop())

    def test_rejects_gap_as_bootstrap_pass(self) -> None:
        self.reject_snapshot(lambda value: value["fixtures"][0].update(status="bootstrap_pass"))

    def test_rejects_missing_evidence_anchor(self) -> None:
        self.reject_snapshot(lambda value: value["fixtures"][1].update(evidence=["docs/compiler-scale-proof-v1.md::removed_marker"]))

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

    def test_rejects_coordinated_schema_and_snapshot_shortcut(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            snapshot = self.copy_contract(root)
            schema_path = root / checker.SCHEMA
            schema = json.loads(schema_path.read_text(encoding="utf-8"))
            changes = {
                ("runtimeSensitivity", "rebuilds_between_inputs"): 1,
                ("buildPurity", "generated_host_source"): "required",
                ("associativeState", "fixed_literal_tables_allowed"): True,
            }
            snapshot["target_contract"]["runtime_sensitivity"]["rebuilds_between_inputs"] = 1
            snapshot["target_contract"]["build_purity"]["generated_host_source"] = "required"
            snapshot["target_contract"]["associative_state"]["fixed_literal_tables_allowed"] = True
            for (definition, field), value in changes.items():
                schema["$defs"][definition]["properties"][field]["const"] = value
            schema_path.write_text(json.dumps(schema), encoding="utf-8")
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_rejects_host_capture(self) -> None:
        self.reject_snapshot(lambda value: value["target_contract"]["package_roles"].append("compiler.rust.runtime"))

    def test_rejects_production_readiness_overclaim(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            payload = json.loads((root / checker.PRODUCTION_READINESS).read_text(encoding="utf-8"))
            row = next(item for item in payload["rows"] if item["id"] == "compiler_scale_runtime_proof")
            row.update(currentTier="runtime_complete", status="implemented")
            (root / checker.PRODUCTION_READINESS).write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_rejects_self_hosting_readiness_overclaim(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            payload = json.loads((root / checker.SELF_HOSTING_READINESS).read_text(encoding="utf-8"))
            row = next(item for item in payload["rows"] if item["id"] == "compiler_command_surface")
            row.update(status="implemented", directNativeStatus="implemented")
            (root / checker.SELF_HOSTING_READINESS).write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_rejects_missing_source_marker(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            path = root / checker.DIAGNOSTICS_SPIKE
            path.write_text(path.read_text(encoding="utf-8").replace("compiler-diagnostics-spike", "removed-spike"), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)


if __name__ == "__main__":
    unittest.main()
