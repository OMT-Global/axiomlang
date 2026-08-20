#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-runtime-observability-v1.py"
spec = importlib.util.spec_from_file_location("check_runtime_observability_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class RuntimeObservabilityContractTests(unittest.TestCase):
    data_root = ROOT

    def copy_contract(self, destination: Path) -> None:
        paths = [checker.SCHEMA, checker.EVIDENCE_SCHEMA, checker.SNAPSHOT]
        snapshot = checker.load(self.data_root, checker.SNAPSHOT)
        fixtures = snapshot.get("fixtures")
        checker.require(isinstance(fixtures, list), "fixtures must be an array")
        checker.require(
            len(fixtures) <= checker.MAX_FIXTURE_FILES,
            "fixture list exceeds safe copy bound",
        )
        seen_fixture_paths: set[tuple[str, ...]] = set()
        for fixture in fixtures:
            checker.require(
                isinstance(fixture, dict) and isinstance(fixture.get("path"), str),
                "fixture path must be a string",
            )
            fixture_parts = checker.relative_parts(Path(fixture["path"]))
            relative = checker.FIXTURES.joinpath(*fixture_parts)
            safe_parts = checker.relative_parts(relative)
            checker.require(safe_parts not in seen_fixture_paths, "duplicate fixture path")
            seen_fixture_paths.add(safe_parts)
            paths.append(Path(*safe_parts))

        payloads: list[tuple[Path, bytes]] = []
        for relative in paths:
            safe_relative = Path(*checker.relative_parts(relative))
            payloads.append((safe_relative, checker.read_bytes(self.data_root, safe_relative)))
        for safe_relative, payload in payloads:
            target = destination / safe_relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(payload)

    def test_contract_and_fixtures_pass(self) -> None:
        self.assertEqual(
            checker.validate_contract(self.data_root),
            {
                "schema": "axiom.runtime_observability.v1",
                "ok": True,
                "fixtures": 6,
                "runtime_evidence": "checked_in_runtime_fixture",
            },
        )

    def test_external_runtime_proof_reports_executed_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            proof_path = root / checker.FIXTURES / "runtime-core-golden.json"
            self.assertEqual(
                checker.validate_contract(root, proof_path)["runtime_evidence"],
                "executed_rust_runtime",
            )

    def test_published_inspection_schema_enforces_runtime_ceilings(self) -> None:
        schema = checker.load(self.data_root, checker.EVIDENCE_SCHEMA)
        properties = schema["$defs"]["inspection"]["properties"]
        for field, value in {
            "queue_capacity": 4097,
            "max_queued_bytes": 67108865,
            "max_event_bytes": 65537,
            "max_fields": 33,
            "flush_timeout_ms": 60001,
        }.items():
            with self.subTest(field=field), self.assertRaises(checker.ContractError):
                checker.validate_schema(value, properties[field], f"$.{field}", schema)

    def test_published_typed_fields_enforce_runtime_numeric_domains(self) -> None:
        schema = checker.load(self.data_root, checker.EVIDENCE_SCHEMA)
        typed_field = schema["$defs"]["typed_field"]
        for field_type, value in (
            ("integer", 9223372036854775808),
            ("unsigned", 18446744073709551616),
            ("float", checker.Decimal("1.7976931348623158e308")),
        ):
            with self.subTest(field_type=field_type), self.assertRaises(checker.ContractError):
                checker.validate_schema(
                    {"type": field_type, "value": value},
                    typed_field,
                    "$.typed_field",
                    schema,
                )

    def test_schema_number_accepts_integer_and_numeric_equality(self) -> None:
        checker.validate_schema(1, {"type": "number"}, "$")
        checker.validate_schema(1.0, {"type": "integer"}, "$")
        checker.validate_schema(1, {"const": 1.0}, "$")

    def test_schema_json_equality_is_recursive_and_unique_items_is_numeric(self) -> None:
        checker.validate_schema({"items": [1]}, {"const": {"items": [1.0]}}, "$")
        with self.assertRaises(checker.ContractError):
            checker.validate_schema({"items": [True]}, {"const": {"items": [1]}}, "$")
        with self.assertRaises(checker.ContractError):
            checker.validate_schema([1, 1.0], {"type": "array", "uniqueItems": True}, "$")

    def test_exact_json_numbers_do_not_round_or_underflow(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            precise = root / "precise.json"
            precise.write_text("1.00000000000000001", encoding="utf-8")
            value = checker.load(root, Path("precise.json"))
            with self.assertRaises(checker.ContractError):
                checker.validate_schema(value, {"const": 1}, "$")
            checker.validate_schema([1, value], {"type": "array", "uniqueItems": True}, "$")

            tiny = root / "tiny.json"
            tiny.write_text("-1e-10000", encoding="utf-8")
            value = checker.load(root, Path("tiny.json"))
            with self.assertRaises(checker.ContractError):
                checker.validate_schema(value, {"type": "integer", "minimum": 0}, "$")

    def test_runtime_event_size_uses_unescaped_utf8_bytes(self) -> None:
        proof = checker.load(
            self.data_root,
            checker.FIXTURES / "runtime-core-golden.json",
        )
        for index in range(3):
            proof["event"]["fields"][f"unicode_{index}"] = {
                "type": "string",
                "value": "😀" * 4000,
            }
        snapshot = checker.load(self.data_root, checker.SNAPSHOT)
        schema = checker.load(self.data_root, checker.EVIDENCE_SCHEMA)
        self.assertLessEqual(
            len(checker.encode_json(proof["event"]).encode("utf-8")),
            snapshot["event"]["max_event_bytes"],
        )
        checker.validate_runtime_proof(proof, snapshot, schema)

    def test_nonfinite_json_numbers_are_rejected(self) -> None:
        for value in ("NaN", "Infinity", "-Infinity", "1e10000", "9" * 309):
            with self.subTest(value=value), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                path = root / "value.json"
                path.write_text(value, encoding="utf-8")
                with self.assertRaises(checker.ContractError):
                    checker.load(root, Path("value.json"))
                with self.assertRaises(checker.ContractError):
                    checker.load_external(path)

    def test_filter_target_level_names_use_the_event_target_grammar(self) -> None:
        schema = checker.load(self.data_root, checker.EVIDENCE_SCHEMA)
        target_levels = schema["$defs"]["filter"]["properties"]["target_levels"]
        checker.validate_schema({"runtime/http": "info"}, target_levels, "$.target_levels", schema)
        for target in ("", "contains space", "x" * 129):
            with self.subTest(target=target), self.assertRaises(checker.ContractError):
                checker.validate_schema({target: "info"}, target_levels, "$.target_levels", schema)

    def test_cli_is_deterministic_and_uses_explicit_data_root(self) -> None:
        command = [sys.executable, str(CHECKER), "--root", str(self.data_root), "--json"]
        first = subprocess.run(command, check=True, capture_output=True, text=True)
        second = subprocess.run(command, check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)
        self.assertTrue(json.loads(first.stdout)["ok"])

    def test_split_checkout_never_executes_pr_head_checker(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            untrusted = root / "scripts/ci/check-runtime-observability-v1.py"
            untrusted.parent.mkdir(parents=True)
            untrusted.write_text("raise SystemExit(97)\n", encoding="utf-8")
            result = subprocess.run(
                [sys.executable, str(CHECKER), "--root", str(root), "--json"],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertTrue(json.loads(result.stdout)["ok"])

    def test_snapshot_rejects_unredacted_sink(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            snapshot = checker.load(root, checker.SNAPSHOT)
            snapshot["redaction"]["before_sink"] = False
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_snapshot_rejects_duplicate_labels(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            snapshot = checker.load(root, checker.SNAPSHOT)
            snapshot["event"]["labels"] = ["component", "component"]
            (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_runtime_proof_rejects_secret_value(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            proof_path = checker.FIXTURES / "runtime-core-golden.json"
            proof = checker.load(root, proof_path)
            proof["event"]["fields"]["password"] = {
                "type": "string",
                "value": "marker-secret",
            }
            (root / proof_path).write_text(json.dumps(proof), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_runtime_proof_requires_structured_correlation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            proof_path = checker.FIXTURES / "runtime-core-golden.json"
            proof = checker.load(root, proof_path)
            proof["event"].pop("correlation")
            (root / proof_path).write_text(json.dumps(proof), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_runtime_proof_applies_the_published_evidence_schema(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            proof_path = checker.FIXTURES / "runtime-core-golden.json"
            proof = checker.load(root, proof_path)
            proof["event"]["unexpected"] = True
            external = root / "runtime-evidence.json"
            external.write_text(json.dumps(proof), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root, external)

    def test_evidence_schema_rejects_plaintext_error_messages(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            proof_path = checker.FIXTURES / "runtime-core-golden.json"
            proof = checker.load(root, proof_path)
            proof["event"]["error"]["message"] = {
                "type": "string",
                "value": "plain error detail",
            }
            (root / proof_path).write_text(json.dumps(proof), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_runtime_proof_rejects_false_flush_success(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.copy_contract(root)
            proof_path = checker.FIXTURES / "runtime-core-golden.json"
            proof = checker.load(root, proof_path)
            proof["shutdown_report"]["queue_remaining"] = 1
            (root / proof_path).write_text(json.dumps(proof), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_safe_reader_rejects_absolute_windows_traversal_dot_and_nul_paths(self) -> None:
        for path in [
            Path("/etc/passwd"),
            Path("C:\\Windows\\system.ini"),
            Path("../escape.json"),
            Path("./dot.json"),
            Path("bad\x00path.json"),
        ]:
            with self.subTest(path=path), self.assertRaises(checker.ContractError):
                checker.read_bytes(self.data_root, path)

    def test_contract_copy_rejects_unsafe_duplicate_and_unbounded_fixture_paths_before_writing(self) -> None:
        for name, mutate in (
            (
                "absolute",
                lambda snapshot, escape: snapshot["fixtures"][0].update(
                    {"path": str(escape / "created" / "fixture.json")}
                ),
            ),
            (
                "traversal",
                lambda snapshot, _escape: snapshot["fixtures"][0].update(
                    {"path": "../../created/fixture.json"}
                ),
            ),
            (
                "duplicate",
                lambda snapshot, _escape: snapshot["fixtures"].append(
                    {**snapshot["fixtures"][0], "id": "observability-v1/duplicate"}
                ),
            ),
            (
                "unbounded",
                lambda snapshot, _escape: snapshot.update(
                    {"fixtures": snapshot["fixtures"] * (checker.MAX_FIXTURE_FILES + 1)}
                ),
            ),
        ):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary) / "source"
                destination = Path(temporary) / "destination"
                escape = Path(temporary) / "escape"
                self.copy_contract(root)
                snapshot = checker.load(root, checker.SNAPSHOT)
                mutate(snapshot, escape)
                (root / checker.SNAPSHOT).write_text(json.dumps(snapshot), encoding="utf-8")
                original_root = self.data_root
                try:
                    self.data_root = root
                    with self.assertRaises(checker.ContractError):
                        self.copy_contract(destination)
                finally:
                    self.data_root = original_root
                self.assertFalse(destination.exists())
                self.assertFalse(escape.exists())

    def test_safe_reader_rejects_symlink_root_component_and_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary)
            real = parent / "real"
            real.mkdir()
            (real / "safe.json").write_text("{}", encoding="utf-8")
            root_link = parent / "root-link"
            root_link.symlink_to(real, target_is_directory=True)
            with self.assertRaises(checker.ContractError):
                checker.read_bytes(root_link, Path("safe.json"))

            outside = parent / "outside"
            outside.mkdir()
            (outside / "secret.json").write_text("{}", encoding="utf-8")
            (real / "component").symlink_to(outside, target_is_directory=True)
            with self.assertRaises(checker.ContractError):
                checker.read_bytes(real, Path("component/secret.json"))

            (real / "file-link.json").symlink_to(outside / "secret.json")
            with self.assertRaises(checker.ContractError):
                checker.read_bytes(real, Path("file-link.json"))

    def test_safe_reader_rejects_directory_fifo_device_oversize_and_invalid_utf8(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "directory").mkdir()
            with self.assertRaises(checker.ContractError):
                checker.read_bytes(root, Path("directory"))

            fifo = root / "fifo"
            os.mkfifo(fifo)
            with self.assertRaises(checker.ContractError):
                checker.read_bytes(root, Path("fifo"))

            oversized = root / "oversized"
            oversized.write_bytes(b"x" * (checker.MAX_SOURCE_BYTES + 1))
            with self.assertRaises(checker.ContractError):
                checker.read_bytes(root, Path("oversized"))

            invalid = root / "invalid.json"
            invalid.write_bytes(b"\xff")
            with self.assertRaises(checker.ContractError):
                checker.read_text(root, Path("invalid.json"))

        if Path("/dev/null").exists():
            with self.assertRaises(checker.ContractError):
                checker.read_bytes(Path("/dev"), Path("null"))


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    args, unittest_args = parser.parse_known_args(argv)
    RuntimeObservabilityContractTests.data_root = args.root
    program = unittest.main(argv=[sys.argv[0], *unittest_args], exit=False)
    return 0 if program.result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
