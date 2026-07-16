#!/usr/bin/env python3
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-runtime-lifecycle-v1.py"


def run(root):
    return subprocess.run([sys.executable, str(root / "scripts/ci/check-runtime-lifecycle-v1.py")], cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def main():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory) / "repo"
        shutil.copytree(ROOT / "stage1", root / "stage1")
        (root / "scripts/ci").mkdir(parents=True)
        shutil.copy2(CHECKER, root / "scripts/ci/check-runtime-lifecycle-v1.py")
        if run(root).returncode != 0:
            raise SystemExit("valid Runtime Lifecycle ABI fixture was rejected")
        path = root / "stage1/compiler-contracts/snapshots/runtime-lifecycle-v1.json"
        value = json.loads(path.read_text())
        value["features"] = list(reversed(value["features"]))
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("unordered Runtime Lifecycle ABI features were accepted")
        value["features"] = sorted(value["features"])
        value["fixtures"] = value["fixtures"][:-1]
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("incomplete Runtime Lifecycle ABI fixtures were accepted")
        value = json.loads((ROOT / "stage1/compiler-contracts/snapshots/runtime-lifecycle-v1.json").read_text())
        value["unexpected_schema_violation"] = True
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("Runtime Lifecycle ABI fixture with an unexpected schema field was accepted")
    print("Runtime Lifecycle ABI v1 checker tests passed")


if __name__ == "__main__":
    main()
