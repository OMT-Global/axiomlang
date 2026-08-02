#!/usr/bin/env python3
"""Hermetic regressions for the Compatibility v1 corpus boundary."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-compatibility-corpus-v1.py"
FIXTURES = ROOT / "stage1/compatibility/fixtures"


def run(fixtures: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(CHECKER), "--fixtures-root", str(fixtures), "--json"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


def main() -> int:
    result = run(FIXTURES)
    assert result.returncode == 0, result.stdout + result.stderr
    with tempfile.TemporaryDirectory() as temporary:
        fixtures = Path(temporary) / "fixtures"
        shutil.copytree(FIXTURES, fixtures)
        (fixtures / "current/src/main.ax").unlink()
        result = run(fixtures)
        assert result.returncode != 0
        assert "current corpus missing required files" in result.stdout
    with tempfile.TemporaryDirectory() as temporary:
        fixtures = Path(temporary) / "fixtures"
        shutil.copytree(FIXTURES, fixtures)
        metadata = fixtures / "previous-contract-fixture/metadata.json"
        metadata.write_text(
            metadata.read_text(encoding="utf-8").replace(
                '"status": "no_compiler_association"',
                '"status": "workspace_source"',
            ),
            encoding="utf-8",
        )
        result = run(fixtures)
        assert result.returncode != 0
        assert "compiler status must be no_compiler_association" in result.stdout
    mutations = [
        (("unexpected",), True, "metadata fields drifted"),
        (("compiler", "version"), "9.9.9", "compiler metadata contradicts"),
        (("compiler", "released"), True, "must not claim a released"),
        (("compiler", "qualified_previous"), True, "must not claim a released"),
        (("edition", "id"), "9999", "edition metadata contradicts"),
        (("edition", "policy_status"), "supported", "edition metadata contradicts"),
        (("edition", "manifest_selection"), "selectable", "edition metadata contradicts"),
        (("contract",), "elsewhere.json", "metadata contract must be contract.json"),
        (("qualification",), "Published compiler evidence.", "qualification disclaimer drifted"),
    ]
    for path, replacement, message in mutations:
        with tempfile.TemporaryDirectory() as temporary:
            fixtures = Path(temporary) / "fixtures"
            shutil.copytree(FIXTURES, fixtures)
            metadata_path = fixtures / "current/metadata.json"
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            target = metadata
            for key in path[:-1]:
                target = target[key]
            target[path[-1]] = replacement
            metadata_path.write_text(
                json.dumps(metadata, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            result = run(fixtures)
            assert result.returncode != 0, result.stdout
            assert message in result.stdout, result.stdout
    print("compatibility corpus regression cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
