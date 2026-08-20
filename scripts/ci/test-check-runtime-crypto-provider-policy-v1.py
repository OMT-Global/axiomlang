#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import copy
import importlib.util
import json
import os
import socket
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Callable


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-runtime-crypto-provider-policy-v1.py"
SPEC = importlib.util.spec_from_file_location("check_runtime_crypto_provider_policy_v1", CHECKER)
assert SPEC and SPEC.loader
checker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checker)
DATA_ROOT = ROOT


class RuntimeCryptoProviderPolicyTests(unittest.TestCase):
    def copy_contract(self, root: Path, source_root: Path | None = None) -> None:
        source = DATA_ROOT if source_root is None else source_root
        paths = [
            checker.SCHEMA,
            checker.SNAPSHOT,
            *[
                f"{checker.FIXTURES}/{name}"
                for name in sorted(checker.EXPECTED_FIXTURES)
            ],
        ]
        with checker.RepositoryReader(source) as reader:
            for relative in paths:
                destination = root / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(reader.read_bytes(relative))

    def mutate_snapshot(self, mutation: Callable[[dict], None]) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            self.copy_contract(root)
            path = root / checker.SNAPSHOT
            snapshot = json.loads(path.read_text(encoding="utf-8"))
            mutation(snapshot)
            path.write_text(json.dumps(snapshot), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def mutate_fixture(self, name: str, mutation: Callable[[dict], None]) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            self.copy_contract(root)
            path = root / checker.FIXTURES / name
            fixture = json.loads(path.read_text(encoding="utf-8"))
            mutation(fixture)
            path.write_text(json.dumps(fixture), encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def run_checker(self, root: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(CHECKER), "--root", str(root), "--json"],
            check=False,
            capture_output=True,
            text=True,
        )

    def test_contract_and_fixtures_pass(self) -> None:
        result = checker.validate_contract(DATA_ROOT)
        self.assertEqual(result["algorithms"], 7)
        self.assertEqual(result["fixtures"], 4)
        self.assertTrue(result["ok"])
        self.assertEqual(result["targets"], ["linux-x86_64", "macos-arm64"])

    def test_schema_const_and_enum_preserve_json_boolean_type(self) -> None:
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(True, {"const": 1})
        with self.assertRaises(checker.ContractError):
            checker.validate_schema(False, {"enum": [0]})

    def test_cli_is_deterministic_and_root_selectable(self) -> None:
        command = [
            sys.executable,
            str(CHECKER),
            "--root",
            str(DATA_ROOT),
            "--json",
        ]
        first = subprocess.run(command, check=True, capture_output=True, text=True)
        second = subprocess.run(command, check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)
        self.assertTrue(json.loads(first.stdout)["ok"])

    def test_split_checkout_reads_only_the_explicit_root(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pull_request_root = Path(temporary).resolve()
            self.copy_contract(pull_request_root)
            snapshot_path = pull_request_root / checker.SNAPSHOT
            snapshot = json.loads(snapshot_path.read_text(encoding="utf-8"))
            snapshot["status"] = "retired"
            snapshot_path.write_text(json.dumps(snapshot), encoding="utf-8")
            result = self.run_checker(pull_request_root)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(
                set(json.loads(result.stdout)),
                {"error", "ok"},
            )

    def test_repository_paths_reject_lexical_escapes(self) -> None:
        rejected = [
            "/etc/passwd",
            r"C:\Windows\system.ini",
            r"\\server\share\policy.json",
            "policy\x00.json",
            ".",
            "..",
            "./policy.json",
            "fixtures/../policy.json",
            "fixtures//policy.json",
            "fixtures/",
        ]
        with tempfile.TemporaryDirectory() as temporary:
            with checker.RepositoryReader(Path(temporary).resolve()) as reader:
                for value in rejected:
                    with self.subTest(value=repr(value)):
                        with self.assertRaises(checker.ContractError):
                            reader.read_bytes(value)

    def test_repository_reader_rejects_root_and_component_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            real = root / "real"
            real.mkdir()
            (real / "policy.json").write_text("{}", encoding="utf-8")
            (root / "middle").symlink_to(real, target_is_directory=True)
            (root / "final.json").symlink_to(real / "policy.json")
            (root / "regular").write_text("{}", encoding="utf-8")
            root_link = root.parent / f"{root.name}-link"
            root_link.symlink_to(root, target_is_directory=True)
            try:
                with checker.RepositoryReader(root) as reader:
                    for relative in (
                        "middle/policy.json",
                        "final.json",
                        "regular/child.json",
                    ):
                        with self.subTest(relative=relative):
                            with self.assertRaises(checker.ContractError):
                                reader.read_bytes(relative)
                with self.assertRaises(checker.ContractError):
                    checker.RepositoryReader(root_link)
            finally:
                root_link.unlink(missing_ok=True)

    def test_repository_reader_rejects_nonregular_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "directory").mkdir()
            os.mkfifo(root / "fifo")
            with checker.RepositoryReader(root) as reader:
                for relative in ("directory", "fifo"):
                    with self.subTest(relative=relative):
                        with self.assertRaises(checker.ContractError):
                            reader.read_bytes(relative)
            unix_socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                try:
                    unix_socket.bind(str(root / "socket"))
                except PermissionError:
                    socket_path = next(
                        (
                            candidate
                            for candidate in (
                                Path("/private/var/run/syslog"),
                                Path("/run/systemd/private"),
                                Path("/run/docker.sock"),
                                Path("/var/run/docker.sock"),
                            )
                            if candidate.exists()
                            and checker._kind(candidate.lstat().st_mode) == "socket"
                        ),
                        None,
                    )
                    self.assertIsNotNone(socket_path, "a socket fixture is required")
                    with checker.RepositoryReader(Path("/")) as reader:
                        with self.assertRaises(checker.ContractError):
                            reader.read_bytes(str(socket_path).removeprefix("/"))
                else:
                    self.assertEqual(checker._kind((root / "socket").lstat().st_mode), "socket")
                    with checker.RepositoryReader(root) as reader:
                        with self.assertRaises(checker.ContractError):
                            reader.read_bytes("socket")
            finally:
                unix_socket.close()
        with checker.RepositoryReader(Path("/")) as reader:
            for relative in ("dev/null",):
                with self.subTest(relative=relative):
                    with self.assertRaises(checker.ContractError):
                        reader.read_bytes(relative)

    def test_repository_reader_rejects_oversize_and_invalid_utf8(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "oversize.json").write_bytes(
                b"x" * (checker.MAX_CONTRACT_BYTES + 1)
            )
            (root / "invalid.json").write_bytes(b'{"bad":"\xff"}')
            with checker.RepositoryReader(root) as reader:
                with self.assertRaises(checker.ContractError):
                    reader.read_bytes("oversize.json")
                with self.assertRaises(checker.ContractError):
                    reader.read_json("invalid.json")

    def test_cli_contract_error_json_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            self.copy_contract(root)
            (root / checker.SNAPSHOT).write_bytes(b"\xff")
            first = self.run_checker(root)
            second = self.run_checker(root)
            self.assertEqual(first.returncode, 1)
            self.assertEqual(first.stdout, second.stdout)
            self.assertEqual(
                json.loads(first.stdout),
                {
                    "error": (
                        "repository file is not valid UTF-8: "
                        + checker.SNAPSHOT
                    ),
                    "ok": False,
                },
            )

    def test_malformed_fixture_roots_return_structured_contract_errors(self) -> None:
        for name in sorted(checker.EXPECTED_FIXTURES):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                self.copy_contract(root)
                fixture = root / checker.FIXTURES / name
                fixture.write_text(json.dumps(["not", "an", "object"]), encoding="utf-8")
                result = self.run_checker(root)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(set(json.loads(result.stdout)), {"error", "ok"})
                self.assertNotIn("Traceback", result.stderr)
                harness = subprocess.run(
                    [sys.executable, __file__, "--root", str(root)],
                    check=False,
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(harness.returncode, 1)
                self.assertEqual(set(json.loads(harness.stdout)), {"error", "ok"})
                self.assertNotIn("Traceback", harness.stderr)

    def test_json_parser_limits_return_structured_contract_errors(self) -> None:
        hostile_documents = (
            "[" * 2000 + "]" * 2000,
            '{"integer":' + "9" * 5000 + "}",
        )
        for document in hostile_documents:
            with self.subTest(prefix=document[:16]), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                self.copy_contract(root)
                (root / checker.SNAPSHOT).write_text(document, encoding="utf-8")
                result = self.run_checker(root)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(set(json.loads(result.stdout)), {"error", "ok"})
                self.assertNotIn("Traceback", result.stderr)

    def test_harness_copy_rejects_source_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as source_temporary:
            with tempfile.TemporaryDirectory() as destination_temporary:
                source = Path(source_temporary).resolve()
                destination = Path(destination_temporary).resolve()
                self.copy_contract(source)
                snapshot = source / checker.SNAPSHOT
                snapshot.unlink()
                snapshot.symlink_to(source / checker.SCHEMA)
                with self.assertRaises(checker.ContractError):
                    self.copy_contract(destination, source)

    def test_ambient_provider_loading_is_rejected(self) -> None:
        self.mutate_snapshot(
            lambda snapshot: snapshot["algorithm_provider"].update(
                {"ambient_host_loading": True}
            )
        )

    def test_provider_target_mismatch_is_rejected(self) -> None:
        self.mutate_snapshot(
            lambda snapshot: snapshot["targets"][0].update(
                {"entropy_source": "apple-security-secrandom"}
            )
        )

    def test_provider_matrix_rejects_numeric_boolean_substitutes(self) -> None:
        self.mutate_fixture(
            "provider-matrix.json",
            lambda fixture: fixture["algorithm_provider"]["requirements"].update(
                {"ambient_host_loading": 0}
            ),
        )
        self.mutate_fixture(
            "provider-matrix.json",
            lambda fixture: fixture["qualification"].update({"qualified": 0}),
        )

    def test_provider_cannot_be_qualified_without_target_evidence(self) -> None:
        self.mutate_snapshot(
            lambda snapshot: snapshot["qualification"].update(
                {"qualified": True, "status": "qualified"}
            )
        )

    def test_activation_requires_a_separate_checked_in_change(self) -> None:
        self.mutate_snapshot(
            lambda snapshot: snapshot["activation"].update(
                {"merge_effect": "active"}
            )
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            self.copy_contract(root)
            artifact = root / checker.ACTIVATION_ARTIFACT
            artifact.parent.mkdir(parents=True, exist_ok=True)
            artifact.write_text("{}", encoding="utf-8")
            with self.assertRaises(checker.ContractError):
                checker.validate_contract(root)

    def test_compile_time_crypto_is_rejected(self) -> None:
        self.mutate_snapshot(
            lambda snapshot: snapshot["execution"].update(
                {"compile_time_effects": "allowed"}
            )
        )

    def test_algorithm_nonce_drift_is_rejected(self) -> None:
        self.mutate_snapshot(
            lambda snapshot: snapshot["algorithms"][0].update(
                {"nonce_bytes": [8, 12]}
            )
        )

    def test_ed25519_64_byte_input_is_rejected_by_policy(self) -> None:
        self.mutate_snapshot(
            lambda snapshot: next(
                algorithm
                for algorithm in snapshot["algorithms"]
                if algorithm["id"] == "ed25519@1"
            )["key_bytes"].update(
                {"allowed": [32, 64], "maximum": 64}
            )
        )

    def test_entropy_fallback_is_rejected(self) -> None:
        self.mutate_snapshot(
            lambda snapshot: snapshot["entropy_sources"][1].update(
                {"fallback": "dev_urandom"}
            )
        )

    def test_inspection_nested_shape_and_values_are_closed(self) -> None:
        self.mutate_fixture(
            "inspection-redaction.json",
            lambda fixture: fixture["report"]["input_lengths"].update(
                {"unexpected": 1}
            ),
        )
        self.mutate_fixture(
            "inspection-redaction.json",
            lambda fixture: fixture["report"].update(
                {"outcome": {"status": "success", "code": "provider_failure"}}
            ),
        )
        self.mutate_fixture(
            "inspection-redaction.json",
            lambda fixture: fixture["channels"]["logs"].update(
                {"message": "not-allowed"}
            ),
        )
        self.mutate_fixture(
            "inspection-redaction.json",
            lambda fixture: fixture["report"].update(
                {"key_identity": "opaque:secret-derived"}
            ),
        )
        self.mutate_fixture(
            "inspection-redaction.json",
            lambda fixture: fixture["representative_reports"][0].update(
                {"key_identity": "opaque_runtime_handle"}
            ),
        )
        self.mutate_fixture(
            "inspection-redaction.json",
            lambda fixture: fixture["representative_reports"][1].update(
                {"provider": "openssl-3.5-evp"}
            ),
        )

    def test_marker_secret_absence_across_all_channels(self) -> None:
        marker = "AXIOM_SECRET_MARKER_1481"
        markers = [
            marker,
            marker.lower(),
            marker.encode("utf-8").hex(),
            base64.b64encode(marker.encode("utf-8")).decode("ascii"),
        ]
        with checker.RepositoryReader(DATA_ROOT) as reader:
            fixture = reader.read_json(
                f"{checker.FIXTURES}/inspection-redaction.json"
            )
        for channel_name, channel_value in fixture["channels"].items():
            with self.subTest(channel=channel_name):
                checker.assert_markers_absent(
                    channel_value, markers, f"inspection.{channel_name}"
                )
                contaminated = {
                    "channel": copy.deepcopy(channel_value),
                    "probe": marker,
                }
                with self.assertRaises(checker.ContractError):
                    checker.assert_markers_absent(
                        contaminated,
                        markers,
                        f"inspection.{channel_name}",
                    )

    def test_successful_inspection_lengths_match_policy(self) -> None:
        def invalid_aead_lengths(fixture: dict) -> None:
            fixture["report"]["input_lengths"]["key"] = 1
            fixture["channels"]["serialized_inspection"] = copy.deepcopy(fixture["report"])

        self.mutate_fixture("inspection-redaction.json", invalid_aead_lengths)
        self.mutate_fixture(
            "inspection-redaction.json",
            lambda fixture: fixture["representative_reports"][1]["input_lengths"].update(
                {"requested": 65537}
            ),
        )

        def short_aead_open(fixture: dict) -> None:
            fixture["report"]["operation"] = "aead.open"
            fixture["report"]["input_lengths"] = {
                "aad": 7,
                "ciphertext": 15,
                "key": 32,
                "nonce": 12,
            }
            fixture["channels"]["serialized_inspection"] = copy.deepcopy(fixture["report"])

        self.mutate_fixture("inspection-redaction.json", short_aead_open)

    def test_vector_source_catalog_rejects_execution_claims_and_bad_sources(self) -> None:
        self.mutate_fixture(
            "algorithm-vectors.json",
            lambda fixture: fixture["vectors"][0].update(
                {"execution_status": "executed"}
            ),
        )
        self.mutate_fixture(
            "algorithm-vectors.json",
            lambda fixture: fixture["vectors"][0].update(
                {"source": "https://example.com/vector"}
            ),
        )
        self.mutate_fixture(
            "algorithm-vectors.json",
            lambda fixture: fixture["vectors"][0].update(
                {"source_case": {"section": "1"}}
            ),
        )

    def test_failure_catalog_rejects_execution_claim_and_incomplete_codes(self) -> None:
        self.mutate_fixture(
            "failure-matrix.json",
            lambda fixture: fixture["cases"][0].update(
                {"execution_status": "executed"}
            ),
        )
        self.mutate_fixture(
            "failure-matrix.json",
            lambda fixture: fixture["cases"].pop(),
        )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--root", type=Path, default=ROOT)
    args, unittest_args = parser.parse_known_args(argv)
    global DATA_ROOT
    DATA_ROOT = args.root
    try:
        checker.validate_contract(DATA_ROOT)
    except (checker.ContractError, KeyError, TypeError) as error:
        print(json.dumps({"error": str(error), "ok": False}, sort_keys=True))
        return 1
    program = unittest.main(
        argv=[sys.argv[0], *unittest_args],
        exit=False,
    )
    return 0 if program.result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
