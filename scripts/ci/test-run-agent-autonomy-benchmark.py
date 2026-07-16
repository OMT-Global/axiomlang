#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
RUNNER = REPO_ROOT / "scripts/ci/run-agent-autonomy-benchmark.py"
FIXTURE = REPO_ROOT / "stage1/agent-autonomy/benchmark-v0.json"
BASELINE = REPO_ROOT / "stage1/agent-autonomy/readiness-baseline-v0.json"


def invoke(fixture: Path, baseline: Path = BASELINE, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(RUNNER), "--fixture", str(fixture), "--baseline", str(baseline), *args],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def main() -> int:
    valid = invoke(FIXTURE, BASELINE, "--validate-only")
    if valid.returncode != 0:
        raise SystemExit(f"valid autonomy fixture was rejected:\n{valid.stderr}")

    with tempfile.TemporaryDirectory() as temp_dir:
        invalid_path = Path(temp_dir) / "duplicate-id.json"
        invalid = json.loads(FIXTURE.read_text(encoding="utf-8"))
        invalid["tasks"][1]["id"] = invalid["tasks"][0]["id"]
        invalid_path.write_text(json.dumps(invalid), encoding="utf-8")
        rejected = invoke(invalid_path, BASELINE, "--validate-only")
        if rejected.returncode == 0 or "unique non-empty id" not in rejected.stderr:
            raise SystemExit(f"duplicate benchmark task id was accepted:\n{rejected.stdout}\n{rejected.stderr}")

        false_green_fixture = json.loads(FIXTURE.read_text(encoding="utf-8"))
        false_green_fixture["tasks"] = [false_green_fixture["tasks"][0]]
        false_green_fixture["tasks"][0]["command"] = ["true"]
        false_green_path = Path(temp_dir) / "false-green.json"
        false_green_path.write_text(json.dumps(false_green_fixture), encoding="utf-8")
        false_green_baseline = json.loads(BASELINE.read_text(encoding="utf-8"))
        false_green_baseline["minimum_tasks"] = 1
        false_green_baseline["minimum_ci_tasks"] = 1
        false_green_baseline["minimum_stop_tasks"] = 0
        false_green_baseline_path = Path(temp_dir) / "false-green-baseline.json"
        false_green_baseline_path.write_text(json.dumps(false_green_baseline), encoding="utf-8")
        false_green = invoke(false_green_path, false_green_baseline_path, "--subset", "ci", "--check")
        if false_green.returncode == 0 or '"false_green_count": 1' not in false_green.stdout:
            raise SystemExit(f"no-op benchmark task was accepted:\n{false_green.stdout}\n{false_green.stderr}")

    print("agent autonomy benchmark runner tests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
