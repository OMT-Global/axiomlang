#!/usr/bin/env python3
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-runtime-associative-collections-v1.py"


def run(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(root / "scripts/ci/check-runtime-associative-collections-v1.py")],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def main() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory) / "repo"
        shutil.copytree(ROOT / "stage1", root / "stage1")
        (root / "scripts/ci").mkdir(parents=True)
        shutil.copy2(CHECKER, root / "scripts/ci/check-runtime-associative-collections-v1.py")
        if run(root).returncode != 0:
            raise SystemExit("valid associative collections contract was rejected")
        path = root / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json"

        value = json.loads(path.read_text())
        value["keys"]["accepted"] = ["primitive"]
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("incomplete key-shape coverage was accepted")

        value = json.loads((ROOT / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json").read_text())
        value["resources"]["collision"] = "unbounded"
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("unbounded collision handling was accepted")

        value = json.loads((ROOT / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json").read_text())
        value["fixtures"][9]["kind"] = "positive"
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("missing adversarial fixture classification was accepted")

        value = json.loads((ROOT / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json").read_text())
        del value["keys"]["equality"]["tuple"]
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("missing nested schema constraint was accepted")

        value = json.loads((ROOT / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json").read_text())
        value["hashing"]["host_seed"] = "Rust HashMap detail"
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("host-specific associative collection terms were accepted")
    print("Runtime Associative Collections v1 checker tests passed")


if __name__ == "__main__":
    main()
