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
        stability_downgrade = Path(directory) / "stability-downgrade.json"
        payload = json.loads(OLD.read_text(encoding="utf-8"))
        loop = next(surface for surface in payload["surfaces"] if surface["id"] == "axiom://language/loop")
        loop["stability"] = "experimental"
        stability_downgrade.write_text(json.dumps(payload), encoding="utf-8")
        result = run("--old", str(OLD), "--new", str(stability_downgrade))
        assert result.returncode != 0
        assert "breaking public surface axiom://language/loop requires a migration note" in result.stdout
        stability_promotion_old = Path(directory) / "stability-promotion-old.json"
        payload = json.loads(OLD.read_text(encoding="utf-8"))
        loop = next(surface for surface in payload["surfaces"] if surface["id"] == "axiom://language/loop")
        loop["stability"] = "experimental"
        stability_promotion_old.write_text(json.dumps(payload), encoding="utf-8")
        result = run("--old", str(stability_promotion_old), "--new", str(OLD))
        assert result.returncode == 0, result.stdout + result.stderr
        promotion = next(change for change in json.loads(result.stdout)["changes"] if change["surface_id"] == "axiom://language/loop")
        assert promotion["severity"] == "additive"
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
        top_level_unknown = Path(directory) / "top-level-unknown.json"
        payload = json.loads(CURRENT.read_text(encoding="utf-8"))
        payload["unexpected"] = True
        top_level_unknown.write_text(json.dumps(payload), encoding="utf-8")
        result = run("--old", str(OLD), "--new", str(top_level_unknown))
        assert result.returncode != 0
        assert "new contains unknown properties: unexpected" in result.stdout
        edition_unknown = Path(directory) / "edition-unknown.json"
        payload = json.loads(CURRENT.read_text(encoding="utf-8"))
        payload["edition"]["unexpected"] = True
        edition_unknown.write_text(json.dumps(payload), encoding="utf-8")
        result = run("--old", str(OLD), "--new", str(edition_unknown))
        assert result.returncode != 0
        assert "new.edition contains unknown properties: unexpected" in result.stdout
    print("compatibility v1 regression cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
