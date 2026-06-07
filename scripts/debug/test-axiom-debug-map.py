#!/usr/bin/env python3
"""Regression tests for scripts/debug/axiom-debug-map.py."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
TOOL = REPO_ROOT / "scripts" / "debug" / "axiom-debug-map.py"


class AxiomDebugMapTests(unittest.TestCase):
    def test_resolves_generated_line_from_debug_map(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            debug_map = Path(tmp) / "app.debug-map.json"
            debug_map.write_text(
                json.dumps(
                    {
                        "schema_version": "axiom.stage1.debug_map.v1",
                        "generated_rust": "/tmp/generated.rs",
                        "mappings": [
                            {
                                "generated_line": 17,
                                "source": "/workspace/src/main.ax",
                                "line": 2,
                                "column": 1,
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(TOOL),
                    "resolve",
                    "--debug-map",
                    str(debug_map),
                    "--generated-line",
                    "17",
                ],
                check=True,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

        self.assertEqual(result.stdout.strip(), "/workspace/src/main.ax:2:1")

    def test_resolves_generated_line_from_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            debug_map = Path(tmp) / "app.debug-map.json"
            manifest = Path(tmp) / "app.debug-manifest.json"
            debug_map.write_text(
                json.dumps(
                    {
                        "schema_version": "axiom.stage1.debug_map.v1",
                        "generated_rust": "/tmp/generated.rs",
                        "mappings": [
                            {
                                "generated_line": 31,
                                "source": "/workspace/src/helper.ax",
                                "line": 4,
                                "column": 3,
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            manifest.write_text(
                json.dumps(
                    {
                        "schema_version": "axiom.stage1.debug_manifest.v1",
                        "debug_map": str(debug_map),
                    }
                ),
                encoding="utf-8",
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(TOOL),
                    "resolve",
                    "--manifest",
                    str(manifest),
                    "--generated-line",
                    "31",
                    "--json",
                ],
                check=True,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

        self.assertEqual(
            json.loads(result.stdout),
            {"column": 3, "line": 4, "source": "/workspace/src/helper.ax"},
        )

    def test_missing_mapping_exits_nonzero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            debug_map = Path(tmp) / "app.debug-map.json"
            debug_map.write_text(
                json.dumps(
                    {
                        "schema_version": "axiom.stage1.debug_map.v1",
                        "generated_rust": "/tmp/generated.rs",
                        "mappings": [],
                    }
                ),
                encoding="utf-8",
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(TOOL),
                    "resolve",
                    "--debug-map",
                    str(debug_map),
                    "--generated-line",
                    "99",
                ],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

        self.assertEqual(result.returncode, 1)
        self.assertIn("no Axiom span for generated line 99", result.stderr)


if __name__ == "__main__":
    unittest.main()
