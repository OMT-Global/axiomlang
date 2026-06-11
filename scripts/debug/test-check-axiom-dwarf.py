#!/usr/bin/env python3
"""Regression tests for scripts/debug/check-axiom-dwarf.py."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
TOOL = REPO_ROOT / "scripts" / "debug" / "check-axiom-dwarf.py"


def write_manifest(path: Path, binary: Path, axiom_dwarf: bool) -> None:
    path.write_text(
        json.dumps(
            {
                "schema_version": "axiom.stage1.direct_native.debug_manifest.v1",
                "backend": "cranelift",
                "binary": str(binary),
                "native_debug": {
                    "producer": "cranelift",
                    "debuginfo": 2,
                    "opt_level": 0,
                    "native_debug_info": "native Axiom DWARF line tables",
                    "axiom_dwarf": axiom_dwarf,
                },
            }
        ),
        encoding="utf-8",
    )


class CheckAxiomDwarfTests(unittest.TestCase):
    def test_false_manifest_claim_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            binary.write_bytes(b".debug_line .debug_info /workspace/src/main.ax")
            write_manifest(manifest, binary, False)

            result = subprocess.run(
                [
                    sys.executable,
                    str(TOOL),
                    "verify",
                    "--manifest",
                    str(manifest),
                ],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

        self.assertEqual(result.returncode, 1)
        self.assertIn("native_debug.axiom_dwarf is false", result.stderr)

    def test_true_manifest_claim_requires_binary_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            binary.write_bytes(b"native binary without debug sections")
            write_manifest(manifest, binary, True)

            result = subprocess.run(
                [
                    sys.executable,
                    str(TOOL),
                    "verify",
                    "--manifest",
                    str(manifest),
                ],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

        self.assertEqual(result.returncode, 1)
        self.assertIn("missing evidence", result.stderr)
        self.assertIn("DWARF line section marker", result.stderr)
        self.assertIn(".ax source path marker", result.stderr)

    def test_true_manifest_claim_with_binary_evidence_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            binary.write_bytes(b"\0.debug_line\0.debug_info\0/workspace/src/main.ax\0")
            write_manifest(manifest, binary, True)

            result = subprocess.run(
                [
                    sys.executable,
                    str(TOOL),
                    "verify",
                    "--manifest",
                    str(manifest),
                    "--json",
                ],
                check=True,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

        payload = json.loads(result.stdout)
        self.assertTrue(payload["ok"])
        self.assertEqual(payload["binary"], str(binary))


if __name__ == "__main__":
    unittest.main()
