#!/usr/bin/env python3
"""Regression coverage for Compatibility v1 contract validation and reporting."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-compatibility-v1.py"
OLD = ROOT / "stage1/examples/compatibility_v1/old.json"
CURRENT = ROOT / "stage1/examples/compatibility_v1/current.json"


def run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(CHECKER), *args, "--json"], cwd=ROOT, text=True, capture_output=True, check=False)


def main() -> int:
    result = run("--old", str(OLD), "--new", str(CURRENT))
    assert result.returncode == 0, result.stdout + result.stderr
    report = json.loads(result.stdout)
    assert report["schema_version"] == "axiom.compatibility_report.v1"
    assert report["edition"]["severity"] == "breaking"
    assert report["summary"] == {"additive": 3, "breaking": 2, "compatible": 0, "deprecated": 1}
    assert [(change["surface_kind"], change["severity"]) for change in report["changes"]] == [
        ("cli", "breaking"), ("compiler", "breaking"), ("stdlib", "deprecated"), ("language", "additive"), ("schema", "additive"), ("stdlib", "additive")
    ]
    with tempfile.TemporaryDirectory() as directory:
        bad = Path(directory) / "bad.json"
        payload = json.loads(CURRENT.read_text(encoding="utf-8"))
        payload["surfaces"][3].pop("migration")
        bad.write_text(json.dumps(payload), encoding="utf-8")
        result = run("--old", str(OLD), "--new", str(bad))
        assert result.returncode != 0
        assert "breaking public surface axiom://cli/check requires a migration note" in result.stdout
        removed = Path(directory) / "removed.json"
        payload = json.loads(CURRENT.read_text(encoding="utf-8"))
        payload["surfaces"] = [surface for surface in payload["surfaces"] if surface["id"] != "axiom://artifact/axc"]
        payload["migrations"] = {"axiom://artifact/axc": "Replace the legacy artifact with the versioned artifact envelope."}
        removed.write_text(json.dumps(payload), encoding="utf-8")
        result = run("--old", str(OLD), "--new", str(removed))
        assert result.returncode == 0, result.stdout + result.stderr
        assert any(change["surface_id"] == "axiom://artifact/axc" and change["change"] == "removed" for change in json.loads(result.stdout)["changes"])
        duplicate = Path(directory) / "duplicate.json"
        payload = json.loads(OLD.read_text(encoding="utf-8"))
        payload["surfaces"].append(payload["surfaces"][0])
        duplicate.write_text(json.dumps(payload), encoding="utf-8")
        result = run("--old", str(duplicate), "--new", str(CURRENT))
        assert result.returncode != 0
        assert "duplicates public surface axiom://language/loop" in result.stdout
    print("compatibility v1 regression cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
