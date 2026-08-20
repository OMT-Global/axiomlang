#!/usr/bin/env python3
from __future__ import annotations

import copy
import contextlib
import importlib.util
import io
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts/ci/run-target-support-evidence-v1.py"
SPEC = importlib.util.spec_from_file_location("run_target_support_evidence_v1", RUNNER)
assert SPEC and SPEC.loader
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


def passing_evidence() -> dict:
    checks = [
        {"id": identifier, "status": "passed", "claim": runner.CHECK_CLAIMS[identifier]}
        for identifier in sorted(runner.CHECK_CLAIMS)
    ]
    return {
        "schema_version": "axiom.target_support_evidence.v1",
        "evidence_status": "passed",
        "head_sha": "a" * 40,
        "trigger": "test",
        "expected_target": "aarch64-apple-darwin",
        "observed_target": "aarch64-apple-darwin",
        "platform": "macos-arm64",
        "backend": "cranelift",
        "target_selection": "exact-host-only",
        "offline_replay": True,
        "network_policy": "cargo_offline_registry_network_disabled",
        "toolchain": {
            "rustc_version": "rustc 1.90.0 (test)",
            "rustc_host": "aarch64-apple-darwin",
            "rustc_verbose_sha256": "1" * 64,
            "cargo_version": "cargo 1.90.0 (test)",
            "cargo_verbose_sha256": "2" * 64,
            "cargo_lock_sha256": "3" * 64,
            "source_date_epoch": 1_700_000_000,
        },
        "runner_labels": ["ARM64", "macOS", "private", "self-hosted", "xcode"],
        "profiles": ["debug", "release"],
        "binary_metadata": [
            {
                "name": "axiomc",
                "profile": profile,
                "target": "aarch64-apple-darwin",
                "object_format": "mach-o",
                "architecture": "arm64",
                "bytes": 1024,
                "sha256": "4" * 64,
            }
            for profile in ("debug", "release")
        ],
        "checks": checks,
        "qualification": {
            "status": "partial",
            "host_evidence": True,
            "cross_compilation": False,
            "proof_workloads": "executed_or_fail_closed",
            "release_qualification": False,
        },
    }


class TargetSupportEvidenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.schema = runner.load_json(runner.SCHEMA)

    def test_passing_evidence_is_valid(self) -> None:
        runner.validate_evidence(passing_evidence(), self.schema)

    def test_target_mismatch_is_rejected(self) -> None:
        evidence = passing_evidence()
        evidence["observed_target"] = "x86_64-unknown-linux-gnu"
        with self.assertRaises(runner.EvidenceError):
            runner.validate_evidence(evidence, self.schema)

    def test_wrong_binary_target_or_architecture_is_rejected(self) -> None:
        for field, value in (
            ("target", "x86_64-unknown-linux-gnu"),
            ("architecture", "x86_64"),
        ):
            evidence = passing_evidence()
            evidence["binary_metadata"][0][field] = value
            with self.assertRaises(runner.EvidenceError):
                runner.validate_evidence(evidence, self.schema)

    def test_invalid_binary_digest_is_rejected(self) -> None:
        evidence = passing_evidence()
        evidence["binary_metadata"][0]["sha256"] = "not-a-digest"
        with self.assertRaises(runner.EvidenceError):
            runner.validate_evidence(evidence, self.schema)

    def test_binary_header_rejects_wrong_architecture(self) -> None:
        mach_o = bytearray(8)
        mach_o[:4] = bytes.fromhex("cffaedfe")
        mach_o[4:8] = (0x01000007).to_bytes(4, "little")
        with self.assertRaises(runner.EvidenceError):
            runner.binary_architecture(bytes(mach_o), runner.TARGETS["aarch64-apple-darwin"])

    def test_compiler_identity_is_rechecked_after_execution(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            compiler = root / "axiomc"
            mach_o = bytearray(8)
            mach_o[:4] = bytes.fromhex("cffaedfe")
            mach_o[4:8] = (0x0100000C).to_bytes(4, "little")
            compiler.write_bytes(mach_o)
            target = runner.TARGETS["aarch64-apple-darwin"]
            metadata = runner.binary_metadata(
                compiler,
                "debug",
                "aarch64-apple-darwin",
                target,
            )

            def mutate_compiler(*_args, **_kwargs):
                compiler.write_bytes(bytes(mach_o) + b"changed")
                return subprocess.CompletedProcess([str(compiler)], 0, "", "")

            with mock.patch.object(runner, "run_command", side_effect=mutate_compiler):
                with self.assertRaisesRegex(runner.EvidenceError, "binary identity drift"):
                    runner.run_verified_compiler(
                        [str(compiler), "doctor"],
                        compiler,
                        metadata,
                        "aarch64-apple-darwin",
                        target,
                        root,
                        dict(os.environ),
                    )

    def test_checkout_identity_rejects_wrong_head_and_tracked_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.name", "AxiOM CI"], check=True)
            subprocess.run(
                ["git", "-C", str(root), "config", "user.email", "ci@invalid.example"],
                check=True,
            )
            tracked = root / "tracked.txt"
            ignore = root / ".gitignore"
            tracked.write_text("clean\n", encoding="utf-8")
            ignore.write_text("ignored.txt\n", encoding="utf-8")
            subprocess.run(
                ["git", "-C", str(root), "add", "tracked.txt", ".gitignore"],
                check=True,
            )
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(root),
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-q",
                    "-m",
                    "fixture",
                ],
                check=True,
            )
            head = subprocess.run(
                ["git", "-C", str(root), "rev-parse", "HEAD"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            self.assertGreater(
                runner.checkout_source_date_epoch(root, head, dict(os.environ)),
                0,
            )
            with self.assertRaises(runner.EvidenceError):
                runner.checkout_source_date_epoch(root, "0" * 40, dict(os.environ))
            untracked = root / "untracked.txt"
            untracked.write_text("input\n", encoding="utf-8")
            with self.assertRaises(runner.EvidenceError):
                runner.checkout_source_date_epoch(root, head, dict(os.environ))
            untracked.unlink()
            ignored = root / "ignored.txt"
            ignored.write_text("input\n", encoding="utf-8")
            with self.assertRaises(runner.EvidenceError):
                runner.checkout_source_date_epoch(root, head, dict(os.environ))
            ignored.unlink()
            tracked.write_text("dirty\n", encoding="utf-8")
            with self.assertRaises(runner.EvidenceError):
                runner.checkout_source_date_epoch(root, head, dict(os.environ))

    def test_failed_evidence_accepts_bounded_unsupported_observed_host(self) -> None:
        evidence = passing_evidence()
        evidence["evidence_status"] = "failed"
        evidence["observed_target"] = "x86_64-unknown-linux-musl"
        evidence["toolchain"]["rustc_host"] = evidence["observed_target"]
        evidence["checks"][0]["status"] = "failed"
        evidence["checks"][1]["status"] = "skipped"
        evidence["binary_metadata"] = []
        evidence["qualification"]["host_evidence"] = False
        runner.validate_evidence(evidence, self.schema)

    def test_native_smoke_artifact_must_be_anchored_and_not_a_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            project = root / "stage1/examples/stdlib_collection_lookup"
            output = project / "dist/stdlib-collection-lookup"
            output.parent.mkdir(parents=True)
            output.write_bytes(b"binary")
            self.assertEqual(
                runner.anchored_project_artifact(
                    root,
                    "stage1/examples/stdlib_collection_lookup",
                    str(output),
                    "stdlib-collection-lookup",
                ),
                output.resolve(),
            )
            unrelated = root / "unrelated"
            unrelated.write_bytes(b"binary")
            with self.assertRaises(runner.EvidenceError):
                runner.anchored_project_artifact(
                    root,
                    "stage1/examples/stdlib_collection_lookup",
                    str(unrelated),
                    "stdlib-collection-lookup",
                )
            output.unlink()
            output.symlink_to(unrelated)
            with self.assertRaises(runner.EvidenceError):
                runner.anchored_project_artifact(
                    root,
                    "stage1/examples/stdlib_collection_lookup",
                    str(output),
                    "stdlib-collection-lookup",
                )

    def test_native_smoke_artifact_rejects_symlinked_ancestor(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            real_stage1 = root / "real-stage1"
            output = real_stage1 / "examples/stdlib_collection_lookup/dist/stdlib-collection-lookup"
            output.parent.mkdir(parents=True)
            output.write_bytes(b"binary")
            (root / "stage1").symlink_to(real_stage1, target_is_directory=True)
            reported = root / "stage1/examples/stdlib_collection_lookup/dist/stdlib-collection-lookup"
            with self.assertRaisesRegex(runner.EvidenceError, "contains a symlink"):
                runner.anchored_project_artifact(
                    root,
                    "stage1/examples/stdlib_collection_lookup",
                    str(reported),
                    "stdlib-collection-lookup",
                )

    def test_host_mismatch_fails_fast_without_building(self) -> None:
        calls: list[list[str]] = []

        def fake_run(argv: list[str], _root: Path, environment: dict[str, str]):
            self.assertEqual(environment.get("CARGO_NET_OFFLINE"), "true")
            self.assertEqual(environment.get("AXIOM_REGISTRY_NETWORK_DISABLED"), "1")
            calls.append(argv)
            if argv == ["rustc", "-vV"]:
                stdout = "rustc 1.90.0 (test)\nhost: x86_64-unknown-linux-gnu\n"
            elif argv == ["cargo", "--version", "--verbose"]:
                stdout = "cargo 1.90.0 (test)\n"
            elif argv == ["uname", "-s"]:
                stdout = "Linux\n"
            elif argv == ["uname", "-m"]:
                stdout = "x86_64\n"
            else:
                self.fail(f"host mismatch must not execute {argv}")
            return subprocess.CompletedProcess(argv, 0, stdout, "")

        with mock.patch.object(runner, "checkout_source_date_epoch", return_value=1_700_000_000):
            with mock.patch.object(runner, "run_command", side_effect=fake_run):
                evidence = runner.produce_evidence(
                    root=runner.ROOT,
                    expected_target="aarch64-apple-darwin",
                    head_sha="a" * 40,
                    trigger="test",
                    runner_labels=["local"],
                )
        self.assertEqual(evidence["evidence_status"], "failed")
        self.assertEqual(
            next(check for check in evidence["checks"] if check["id"] == "host-identity")["status"],
            "failed",
        )
        self.assertTrue(
            all(call[0] not in {"cargo", "bash"} or call[:2] == ["cargo", "--version"] for call in calls)
        )

    def test_missing_check_is_rejected(self) -> None:
        evidence = passing_evidence()
        evidence["checks"].pop()
        with self.assertRaises(runner.EvidenceError):
            runner.validate_evidence(evidence, self.schema)

    def test_passed_evidence_cannot_hide_failed_check(self) -> None:
        evidence = passing_evidence()
        evidence["checks"][0]["status"] = "failed"
        with self.assertRaises(runner.EvidenceError):
            runner.validate_evidence(evidence, self.schema)

    def test_partial_qualification_cannot_claim_release_or_cross_target_support(self) -> None:
        for field in ("release_qualification", "cross_compilation"):
            evidence = passing_evidence()
            evidence["qualification"][field] = True
            with self.assertRaises(runner.EvidenceError):
                runner.validate_evidence(evidence, self.schema)

    def test_failed_evidence_is_structurally_valid_and_honest(self) -> None:
        evidence = passing_evidence()
        evidence["evidence_status"] = "failed"
        evidence["checks"][0]["status"] = "failed"
        evidence["checks"][1]["status"] = "skipped"
        evidence["binary_metadata"] = []
        evidence["qualification"]["host_evidence"] = False
        runner.validate_evidence(evidence, self.schema)

    def test_validate_cli_accepts_canonical_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "evidence.json"
            path.write_text(json.dumps(passing_evidence()), encoding="utf-8")
            self.assertEqual(runner.main(["validate", "--evidence", str(path)]), 0)

    def test_run_cli_rejects_malformed_runner_labels_before_execution(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(
                    runner.main(
                        [
                            "run",
                            "--expected-target",
                            "aarch64-apple-darwin",
                            "--head-sha",
                            "a" * 40,
                            "--trigger",
                            "test",
                            "--runner-labels-json",
                            '{"not": "an array"}',
                            "--output",
                            str(Path(temporary) / "evidence.json"),
                        ]
                    ),
                    1,
                )

    def test_schema_rejects_open_top_level_contract(self) -> None:
        schema = copy.deepcopy(self.schema)
        schema["additionalProperties"] = True
        with self.assertRaises(runner.EvidenceError):
            runner.validate_evidence(passing_evidence(), schema)

    def test_actual_json_schema_is_applied(self) -> None:
        schema = copy.deepcopy(self.schema)
        schema["properties"]["trigger"]["const"] = "only-this-trigger"
        with self.assertRaises(runner.EvidenceError):
            runner.validate_evidence(passing_evidence(), schema)


if __name__ == "__main__":
    unittest.main()
