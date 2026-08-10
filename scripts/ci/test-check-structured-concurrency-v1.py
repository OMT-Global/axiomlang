#!/usr/bin/env python3

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-structured-concurrency-v1.py"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/runtime-concurrency-v1.json"


def run(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, root / "scripts/ci/check-structured-concurrency-v1.py"],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def write(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value), encoding="utf-8")


def expect_failure(root: Path, value: dict, message: str) -> None:
    write(root / "stage1/compiler-contracts/snapshots/runtime-concurrency-v1.json", value)
    if run(root).returncode == 0:
        raise SystemExit(message)


def main() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory) / "repo"
        shutil.copytree(ROOT / "stage1", root / "stage1")
        (root / "scripts/ci").mkdir(parents=True)
        shutil.copy2(CHECKER, root / "scripts/ci/check-structured-concurrency-v1.py")
        if run(root).returncode != 0:
            raise SystemExit("valid Structured Concurrency v1 fixture was rejected")

        value = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
        value["features"] = list(reversed(value["features"][:-1])) + [value["features"][-1]]
        expect_failure(root, value, "unordered concurrency features were accepted")

        value = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
        value["fixtures"] = value["fixtures"][:-1]
        expect_failure(root, value, "incomplete concurrency fixtures were accepted")

        value = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
        value["task_model"]["hierarchy"] = "detached_children"
        expect_failure(root, value, "detached task hierarchy was accepted")

        value = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
        value["operations"][0]["id"] = "rust://scheduler/spawn"
        expect_failure(root, value, "host-captured operation id was accepted")

        value = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
        value["unexpected_schema_violation"] = True
        expect_failure(root, value, "unexpected schema field was accepted")

    print("Structured Concurrency v1 checker tests passed")


if __name__ == "__main__":
    main()
