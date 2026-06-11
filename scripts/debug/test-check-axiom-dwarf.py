#!/usr/bin/env python3
"""Regression tests for scripts/debug/check-axiom-dwarf.py."""

from __future__ import annotations

import json
import os
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


def write_fake_dwarfdump(path: Path, section_output: str, source_output: str) -> None:
    path.write_text(
        "\n".join(
            [
                "#!/usr/bin/env python3",
                "import sys",
                f"section_output = {section_output!r}",
                f"source_output = {source_output!r}",
                "if '--show-section-sizes' in sys.argv:",
                "    print(section_output, end='')",
                "elif '--show-sources' in sys.argv:",
                "    print(source_output, end='')",
                "else:",
                "    sys.exit(2)",
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    path.chmod(0o755)


def run_tool(
    manifest: Path,
    fake_dwarfdump: Path | None = None,
    json_output: bool = False,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    if fake_dwarfdump is not None:
        env["AXIOM_DWARFDUMP"] = str(fake_dwarfdump)
    command = [
        sys.executable,
        str(TOOL),
        "verify",
        "--manifest",
        str(manifest),
    ]
    if json_output:
        command.append("--json")
    return subprocess.run(
        command,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )


class CheckAxiomDwarfTests(unittest.TestCase):
    def test_false_manifest_claim_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            binary.write_bytes(b".debug_line .debug_info /workspace/src/main.ax")
            write_manifest(manifest, binary, False)

            result = run_tool(manifest)

        self.assertEqual(result.returncode, 1)
        self.assertIn("native_debug.axiom_dwarf is false", result.stderr)

    def test_true_manifest_claim_requires_dwarf_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            fake_dwarfdump = Path(tmp) / "dwarfdump"
            binary.write_bytes(b".debug_line .debug_info /workspace/src/main.ax")
            write_manifest(manifest, binary, True)
            write_fake_dwarfdump(
                fake_dwarfdump,
                """----------------------------------------------------
file: app
----------------------------------------------------
SECTION  SIZE (b)
-------  --------

 Total Size: 0  (0.00%)
 Total File Size: 42
""",
                "",
            )

            result = run_tool(manifest, fake_dwarfdump)

        self.assertEqual(result.returncode, 1)
        self.assertIn("missing evidence", result.stderr)
        self.assertIn("non-empty DWARF line section", result.stderr)
        self.assertIn(".ax source path in DWARF sources", result.stderr)

    def test_true_manifest_claim_with_dwarf_metadata_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            fake_dwarfdump = Path(tmp) / "dwarfdump"
            binary.write_bytes(b"native binary")
            write_manifest(manifest, binary, True)
            write_fake_dwarfdump(
                fake_dwarfdump,
                """----------------------------------------------------
file: app
----------------------------------------------------
SECTION  SIZE (b)
-------  --------
.debug_info  128
.debug_line  64

 Total Size: 192
 Total File Size: 512
""",
                "/workspace/src/main.ax\n",
            )

            result = run_tool(manifest, fake_dwarfdump, json_output=True)

        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertTrue(payload["ok"])
        self.assertEqual(payload["binary"], str(binary))


if __name__ == "__main__":
    unittest.main()
