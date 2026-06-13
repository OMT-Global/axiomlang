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
FNV_OFFSET_BASIS = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3


def hash_bytes(value: bytes) -> str:
    hash_value = FNV_OFFSET_BASIS
    for byte in value:
        hash_value ^= byte
        hash_value = (hash_value * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
    return f"{hash_value:016x}"


def write_manifest(
    path: Path,
    binary: Path,
    axiom_dwarf: bool,
    binary_hash: str | None = None,
    include_binary_hash: bool = True,
) -> None:
    payload = {
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
    if include_binary_hash:
        payload["binary_hash"] = (
            binary_hash if binary_hash is not None else hash_bytes(binary.read_bytes())
        )
    path.write_text(
        json.dumps(payload),
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
                "if '--show-section-sizes' in sys.argv or '--debug-line' in sys.argv:",
                "    print(section_output, end='')",
                "elif '--show-sources' in sys.argv or '--debug-info' in sys.argv:",
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

    def test_missing_binary_hash_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            binary.write_bytes(b"native binary")
            write_manifest(manifest, binary, True, include_binary_hash=False)

            result = run_tool(manifest)

        self.assertEqual(result.returncode, 1)
        self.assertIn("manifest binary_hash is missing or invalid", result.stderr)

    def test_mismatched_binary_hash_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            fake_dwarfdump = Path(tmp) / "dwarfdump"
            binary.write_bytes(b"native binary")
            write_manifest(manifest, binary, True, binary_hash="0000000000000000")
            write_fake_dwarfdump(
                fake_dwarfdump,
                ".debug_info  128\n.debug_line  64\n",
                "/workspace/src/main.ax\n",
            )

            result = run_tool(manifest, fake_dwarfdump)

        self.assertEqual(result.returncode, 1)
        self.assertIn("manifest binary_hash mismatch", result.stderr)

    def test_true_manifest_claim_tolerates_punctuation_around_ax_source_path(self) -> None:
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
                '/workspace/src/main.ax,\n',
            )

            result = run_tool(manifest, fake_dwarfdump)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_true_manifest_claim_accepts_quoted_ax_source_path_with_spaces(self) -> None:
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
                '"/workspace/src/native module/main.ax"\n',
            )

            result = run_tool(manifest, fake_dwarfdump)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_true_manifest_claim_uses_dwarfdump_fallback(self) -> None:
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

            env = os.environ.copy()
            env.pop("AXIOM_DWARFDUMP", None)
            env["PATH"] = f"{tmp}:{env['PATH']}"
            command = [
                sys.executable,
                str(TOOL),
                "verify",
                "--manifest",
                str(manifest),
            ]
            result = subprocess.run(
                command,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=env,
            )

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_true_manifest_claim_uses_versioned_llvm_dwarfdump_flags(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            fake_dwarfdump = Path(tmp) / "llvm-dwarfdump-17"
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

            env = os.environ.copy()
            env["AXIOM_DWARFDUMP"] = str(fake_dwarfdump)
            command = [
                sys.executable,
                str(TOOL),
                "verify",
                "--manifest",
                str(manifest),
            ]
            result = subprocess.run(
                command,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=env,
            )

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_true_manifest_claim_uses_llvm_dwarfdump_flags(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            fake_dwarfdump = Path(tmp) / "llvm-dwarfdump"
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

            env = os.environ.copy()
            env["AXIOM_DWARFDUMP"] = str(fake_dwarfdump)
            command = [
                sys.executable,
                str(TOOL),
                "verify",
                "--manifest",
                str(manifest),
            ]
            result = subprocess.run(
                command,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=env,
            )

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_true_manifest_claim_uses_generic_dwarfdump_flags(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "app"
            manifest = Path(tmp) / "app.debug-manifest.json"
            fake_dwarfdump = Path(tmp) / "dwarfdump"
            binary.write_bytes(b"native binary")
            write_manifest(manifest, binary, True)
            write_fake_dwarfdump(
                fake_dwarfdump,
                """.debug_info[0x00000000]
  Compilation Unit @ offset 0x0:
   Source File: /workspace/src/main.ax
.debug_line contents:
  file_names[ 1]: /workspace/src/main.ax
""",
                "",
            )

            env = os.environ.copy()
            env["AXIOM_DWARFDUMP"] = str(fake_dwarfdump)
            command = [
                sys.executable,
                str(TOOL),
                "verify",
                "--manifest",
                str(manifest),
            ]
            result = subprocess.run(
                command,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=env,
            )

        self.assertEqual(result.returncode, 0, result.stderr)

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
