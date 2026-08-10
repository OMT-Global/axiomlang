#!/usr/bin/env python3
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("check-stage1-benchmarks.py")
SPEC = importlib.util.spec_from_file_location("check_stage1_benchmarks", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class Stage1ArtifactEquivalenceTests(unittest.TestCase):
    def test_equal_artifacts_pass(self):
        with tempfile.TemporaryDirectory() as temp_name:
            root = Path(temp_name)
            (root / ".axiom").mkdir()
            (root / ".axiom" / "provenance.json").write_text(
                '{"path":"' + str(root) + '/source.ax"}\n', encoding="utf-8"
            )
            (root / "program.bin").write_bytes(b"native")
            snapshot = MODULE.snapshot_artifacts(root)
            comparison = MODULE.compare_artifact_snapshots(snapshot, snapshot)
        self.assertEqual(comparison["status"], "pass")

    def test_changed_artifact_fails_with_relative_path(self):
        with tempfile.TemporaryDirectory() as temp_name:
            root = Path(temp_name)
            (root / "program.bin").write_bytes(b"cold")
            cold = MODULE.snapshot_artifacts(root)
            (root / "program.bin").write_bytes(b"warm")
            warm = MODULE.snapshot_artifacts(root)
        comparison = MODULE.compare_artifact_snapshots(cold, warm)
        self.assertEqual(comparison["status"], "fail")
        self.assertEqual(comparison["changed"], ["program.bin"])

    def test_missing_artifact_fails(self):
        with tempfile.TemporaryDirectory() as temp_name:
            root = Path(temp_name)
            (root / "program.bin").write_bytes(b"native")
            cold = MODULE.snapshot_artifacts(root)
            (root / "program.bin").unlink()
            warm = MODULE.snapshot_artifacts(root)
        comparison = MODULE.compare_artifact_snapshots(cold, warm)
        self.assertEqual(comparison["status"], "fail")
        self.assertEqual(comparison["removed"], ["program.bin"])

    def test_metadata_paths_are_normalized(self):
        with tempfile.TemporaryDirectory() as first_name, tempfile.TemporaryDirectory() as second_name:
            first = Path(first_name)
            second = Path(second_name)
            for root in (first, second):
                (root / "cache.toml").write_text(
                    f'module = "{root}/src/main.ax"\n', encoding="utf-8"
                )
            self.assertEqual(
                MODULE.snapshot_artifacts(first), MODULE.snapshot_artifacts(second)
            )

    def test_macho_uuid_and_code_signature_are_normalized(self):
        def macho(uuid: bytes, signature: bytes) -> bytes:
            header = b"\xcf\xfa\xed\xfe" + b"\0" * 12 + (2).to_bytes(4, "little") + b"\0" * 12
            uuid_command = (0x1B).to_bytes(4, "little") + (24).to_bytes(4, "little") + uuid
            signature_command = (
                (0x1D).to_bytes(4, "little")
                + (16).to_bytes(4, "little")
                + (88).to_bytes(4, "little")
                + len(signature).to_bytes(4, "little")
            )
            return header + uuid_command + signature_command + b"\0" * 16 + signature

        with tempfile.TemporaryDirectory() as first_name, tempfile.TemporaryDirectory() as second_name:
            first = Path(first_name)
            second = Path(second_name)
            (first / "program").write_bytes(macho(b"a" * 16, b"signature-a"))
            (second / "program").write_bytes(macho(b"b" * 16, b"signature-b"))
            self.assertEqual(
                MODULE.snapshot_artifacts(first), MODULE.snapshot_artifacts(second)
            )

    def test_timing_and_cache_fields_do_not_change_build_evidence(self):
        project = Path("/repo/project")
        base = {
            "binary": "/repo/project/dist/program",
            "duration_ms": 12,
            "cache_hits": 0,
            "cache_misses": 1,
            "packages": [
                {
                    "package_root": "/repo/project",
                    "binary": "/repo/project/dist/program",
                    "cache_status": "miss",
                    "compile_ms": 10,
                    "lowering": {"lowering_mode": "direct_native_runtime"},
                }
            ],
            "lowering": {"lowering_mode": "direct_native_runtime"},
        }
        warm = {
            **base,
            "duration_ms": 1,
            "cache_hits": 1,
            "cache_misses": 0,
            "packages": [{**base["packages"][0], "cache_status": "hit", "compile_ms": 0}],
        }
        self.assertEqual(
            MODULE.normalized_build_evidence(base, project),
            MODULE.normalized_build_evidence(warm, project),
        )


if __name__ == "__main__":
    unittest.main()
