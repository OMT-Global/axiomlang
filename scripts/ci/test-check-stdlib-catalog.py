#!/usr/bin/env python3
"""Regression coverage for typed stdlib catalog parity and schema guards."""
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-stdlib-catalog.py"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/stdlib-catalog.json"


def run(root):
    return subprocess.run([sys.executable, str(root / "scripts/ci/check-stdlib-catalog.py")], cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def require_rejected(root, value, message):
    path = root / "stage1/compiler-contracts/snapshots/stdlib-catalog.json"
    path.write_text(json.dumps(value))
    if run(root).returncode == 0:
        raise SystemExit(message)


with tempfile.TemporaryDirectory() as directory:
    root = Path(directory) / "repo"
    shutil.copytree(ROOT / "stage1", root / "stage1")
    (root / "scripts/ci").mkdir(parents=True)
    shutil.copy2(CHECKER, root / "scripts/ci/check-stdlib-catalog.py")
    if run(root).returncode != 0:
        raise SystemExit("valid typed stdlib catalog was rejected")
    value = json.loads(SNAPSHOT.read_text())
    value["modules"][0]["symbols"][0]["signature"] = "fn malformed(): unknown"
    require_rejected(root, value, "stdlib catalog accepted a malformed symbol signature")
    value = json.loads(SNAPSHOT.read_text())
    value["modules"][0]["symbols"][0]["provider"]["id"] = "axiom://provider/other"
    require_rejected(root, value, "stdlib catalog accepted a mismatched provider declaration")
    value = json.loads(SNAPSHOT.read_text())
    value["modules"][0]["module_loading"]["source_digest"] = "0" * 64
    require_rejected(root, value, "stdlib catalog accepted a stale module loading digest")
    value = json.loads(SNAPSHOT.read_text())
    del value["acceptance_boundary"]
    require_rejected(root, value, "stdlib catalog accepted a missing acceptance boundary")

print("stdlib catalog checker tests passed")
