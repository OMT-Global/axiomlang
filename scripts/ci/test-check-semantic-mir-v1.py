#!/usr/bin/env python3
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-semantic-mir-v1.py"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/semantic-mir-v1.json"

def run(root):
    return subprocess.run([sys.executable, str(root / "scripts/ci/check-semantic-mir-v1.py")], cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

def main():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory) / "repo"
        shutil.copytree(ROOT / "stage1", root / "stage1")
        (root / "scripts/ci").mkdir(parents=True)
        shutil.copy2(CHECKER, root / "scripts/ci/check-semantic-mir-v1.py")
        if run(root).returncode != 0:
            raise SystemExit("valid Semantic MIR fixture was rejected")
        path = root / "stage1/compiler-contracts/snapshots/semantic-mir-v1.json"
        value = json.loads(path.read_text())
        value["features"] = list(reversed(value["features"]))
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("unordered Semantic MIR features were accepted")
        value["features"] = sorted(value["features"])
        value["functions"][0]["blocks"][0]["semantic_nodes"] = ["axiom://rust/leak"]
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("Rust capture leak was accepted")
        value = json.loads(SNAPSHOT.read_text())
        value["functions"][0]["blocks"][0]["semantic_nodes"] = []
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("Semantic MIR block without provenance was accepted")
        value = json.loads(SNAPSHOT.read_text())
        del value["migration"]
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("Semantic MIR fixture without required migration was accepted")
        value = json.loads(SNAPSHOT.read_text())
        value["package_id"] = "not-an-axiom-id"
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("Semantic MIR fixture with invalid package id was accepted")
        value = json.loads(SNAPSHOT.read_text())
        value["functions"][0]["span"]["line"] = 0
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("Semantic MIR fixture with invalid source span was accepted")
        value = json.loads(SNAPSHOT.read_text())
        value["unexpected_schema_violation"] = True
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("Semantic MIR fixture with unexpected schema field was accepted")
    print("Semantic MIR v1 checker tests passed")

if __name__ == "__main__":
    main()
