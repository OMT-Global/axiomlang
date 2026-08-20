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
CHECKER = ROOT / "scripts/ci/check-dynamic-aggregate-abi-v1.py"
spec = importlib.util.spec_from_file_location("check_dynamic_aggregate_abi_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class DynamicAggregateAbiContractTests(unittest.TestCase):
    def copy_contract(self, root: Path) -> dict:
        for relative in [
            checker.SCHEMA,
            checker.SNAPSHOT,
            checker.RUNTIME_LEDGER,
            checker.READINESS,
            checker.CONTRACT_DOC,
        ]:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text((ROOT / relative).read_text(encoding="utf-8"), encoding="utf-8")
        shutil.copytree(ROOT / checker.FIXTURES, root / checker.FIXTURES)
        return json.loads((root / checker.SNAPSHOT).read_text(encoding="utf-8"))

    def reject_snapshot(self, mutate: Callable[[dict], None]) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            snapshot = self.copy_contract(root)
            mutate(snapshot)
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_contract_layout_lifecycle_and_fixtures_pass(self) -> None:
        self.assertEqual(
            checker.validate_contract(ROOT),
            {
                "schema": "axiom.dynamic_aggregate_abi.v1",
                "ok": True,
                "fixtures": 18,
                "inspection_fields": 18,
                "supported_floor": 6,
            },
        )

    def test_cli_is_deterministic_and_accepts_checkout_root(self) -> None:
        command = [sys.executable, str(CHECKER), "--root", str(ROOT), "--json"]
        first = subprocess.run(command, check=True, capture_output=True, text=True)
        second = subprocess.run(command, check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)
        self.assertTrue(json.loads(first.stdout)["ok"])

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            rooted = subprocess.run(
                [sys.executable, str(CHECKER), "--checkout-root", str(root), "--json"],
                check=True,
                capture_output=True,
                text=True,
            )
            self.assertTrue(json.loads(rooted.stdout)["ok"])

    def test_target_layout_record_is_fully_determined(self) -> None:
        fixture = checker.load(checker.ROOT / checker.FIXTURES / "target-layout-record.json")
        record = checker.compute_layout_record(fixture["evidence"]["case"])
        self.assertEqual(
            record,
            {
                "abi_profile": "sysv64-v1",
                "alignment_bytes": 8,
                "argument_passing": "indirect_pointer",
                "byte_order": "little",
                "discriminant_offset_bytes": 0,
                "discriminant_width_bytes": 1,
                "field_offsets_bytes": [8, 16],
                "layout_id": "choice-step-v1",
                "payload_offset_bytes": 8,
                "pointer_width_bits": 64,
                "return_passing": "caller_provided_storage",
                "size_bytes": 24,
                "target_triple": "x86_64-unknown-linux-gnu",
                "variant_field_offsets_bytes": [
                    {"variant_ordinal": 0, "field_offsets_bytes": []},
                    {"variant_ordinal": 1, "field_offsets_bytes": [8, 16]},
                    {"variant_ordinal": 2, "field_offsets_bytes": [8]},
                ],
            },
        )

    def test_rejects_inconsistent_or_unknown_target_profiles(self) -> None:
        fixture = checker.load(checker.ROOT / checker.FIXTURES / "target-layout-record.json")
        case = fixture["evidence"]["case"]
        inconsistent = json.loads(json.dumps(case))
        inconsistent["target"]["pointer_width_bits"] = 32
        with self.assertRaisesRegex(checker.ContractError, "inputs are inconsistent"):
            checker.compute_layout_record(inconsistent)
        unknown = json.loads(json.dumps(case))
        unknown["target"]["abi_profile"] = "invented-v1"
        with self.assertRaisesRegex(checker.ContractError, "unsupported target ABI profile"):
            checker.compute_layout_record(unknown)

    def test_rejects_layouts_that_exceed_the_bounded_field_inventory(self) -> None:
        with self.assertRaisesRegex(checker.ContractError, "bounded inspection field limit"):
            checker.compute_layout_record(
                {
                    "layout_id": "too-many-fields",
                    "target": {
                        "abi_profile": "sysv64-v1",
                        "byte_order": "little",
                        "pointer_width_bits": 64,
                        "target_triple": "x86_64-unknown-linux-gnu",
                    },
                    "type_model": {
                        "kind": "array",
                        "count": checker.MAX_MATERIALIZED_LAYOUT_FIELDS + 1,
                        "element": {"kind": "scalar", "name": "u8"},
                    },
                }
            )

    def test_rejects_fabricated_layout_record(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            path = root / checker.FIXTURES / "target-layout-record.json"
            fixture = json.loads(path.read_text(encoding="utf-8"))
            fixture["evidence"]["expected"]["record"]["size_bytes"] = 16
            path.write_text(json.dumps(fixture), encoding="utf-8")
            with self.assertRaisesRegex(checker.ContractError, "trusted digest drifted"):
                checker.validate_contract(root)

    def test_rejects_schema_and_snapshot_rule_replacement(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            schema_path = root / checker.SCHEMA
            snapshot_path = root / checker.SNAPSHOT
            schema_path.write_text(schema_path.read_text(encoding="utf-8") + " ", encoding="utf-8")
            snapshot_path.write_text(snapshot_path.read_text(encoding="utf-8") + " ", encoding="utf-8")
            with self.assertRaisesRegex(checker.ContractError, "trusted schema digest drifted"):
                checker.validate_contract(root)

    def test_rejects_symlinked_pr_head_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            document = root / checker.CONTRACT_DOC
            backup = document.with_name("dynamic-aggregate-abi-v1.backup.md")
            document.replace(backup)
            document.symlink_to(backup.name)
            with self.assertRaisesRegex(checker.ContractError, "symlink component"):
                checker.validate_contract(root)

    def test_rejects_passing_rule_and_inspection_drift(self) -> None:
        self.reject_snapshot(
            lambda snapshot: snapshot["logical_layout"]["passing"].update(
                direct_limit_pointer_words=3
            )
        )
        self.reject_snapshot(lambda snapshot: snapshot["inspection_fields"].pop())

    def test_rejects_runtime_completion_claims(self) -> None:
        self.reject_snapshot(
            lambda snapshot: snapshot["current_floor"].update(
                tier="direct_runtime", runtime_origin_non_copy=True
            )
        )
        self.reject_snapshot(
            lambda snapshot: snapshot["current_floor"].update(
                static_projection_retired=True
            )
        )

    def test_rejects_lost_runtime_target_gap(self) -> None:
        self.reject_snapshot(
            lambda snapshot: snapshot["ownership"]["runtime_target_gaps"].pop()
        )

    def test_lifecycle_model_enforces_move_borrow_clone_and_drop(self) -> None:
        self.assertEqual(
            checker.execute_lifecycle(
                [
                    {"op": "create", "value": "owner"},
                    {"op": "clone", "source": "owner", "target": "copy"},
                    {"op": "borrow_shared", "value": "owner", "borrow": "read"},
                    {"op": "end_borrow", "value": "owner", "borrow": "read"},
                    {"op": "move", "source": "owner", "target": "moved"},
                    {"op": "drop", "value": "copy"},
                    {"op": "drop", "value": "moved"},
                ]
            ),
            {"outcome": "accepted", "diagnostic": "", "drop_order": ["copy", "moved"]},
        )
        self.assertEqual(
            checker.execute_lifecycle(
                [
                    {"op": "create", "value": "owner"},
                    {"op": "borrow_shared", "value": "owner", "borrow": "read"},
                    {"op": "borrow_mut", "value": "owner", "borrow": "write"},
                ]
            )["diagnostic"],
            "ownership.borrow_conflict",
        )
        self.assertEqual(
            checker.execute_lifecycle(
                [
                    {"op": "create", "value": "owner"},
                    {"op": "drop", "value": "owner"},
                    {"op": "drop", "value": "owner"},
                ]
            )["diagnostic"],
            "ownership.double_drop",
        )

    def test_lifecycle_rejects_owner_use_during_mutable_borrow(self) -> None:
        result = checker.execute_lifecycle(
            [
                {"op": "create", "value": "owner"},
                {"op": "borrow_mut", "value": "owner", "borrow": "write"},
                {"op": "use", "value": "owner"},
                {"op": "end_borrow", "value": "owner", "borrow": "write"},
                {"op": "drop", "value": "owner"},
            ]
        )
        self.assertEqual(result["diagnostic"], "ownership.use_while_mutably_borrowed")

    def test_lifecycle_rejects_events_after_early_exit(self) -> None:
        result = checker.execute_lifecycle(
            [
                {"op": "create", "value": "owner"},
                {"op": "early_exit"},
                {"op": "create", "value": "unreachable"},
                {"op": "drop", "value": "unreachable"},
            ]
        )
        self.assertEqual(result["diagnostic"], "ownership.events_after_exit")

    def test_rejects_replacement_native_evidence_before_execution(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            evidence_root = Path(temporary)
            self.copy_contract(evidence_root)
            program = evidence_root / checker.PROGRAMS / "aggregate-forwarding" / "src/main.ax"
            program.write_text("fn main(): int {\nreturn 0\n}\n", encoding="utf-8")
            with self.assertRaisesRegex(checker.ContractError, "source digest drifted"):
                checker.validate_contract(evidence_root)


if __name__ == "__main__":
    unittest.main()
