#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/run-stage1-basic-smoke.sh"

python3 - "$script" <<'PY'
import pathlib
import sys

source = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
required = (
    "capture_expected_failure_report",
    "validate-stage1-smoke-report.py",
    '--expect "$expectation"',
    '"bounded-static"',
    "--expect blocked",
    'run_smoke_project "$example" "" "bounded-static"',
    'run_fail_closed_project "capabilities"',
)
missing = [fragment for fragment in required if fragment not in source]
if missing:
    raise SystemExit(
        "basic smoke fail-closed contract is incomplete: " + ", ".join(missing)
    )
if 'run_smoke_project "capabilities"' in source:
    raise SystemExit(
        "capabilities cannot be advertised as direct-native while runtime lowering is blocked"
    )
PY

echo "stage1 basic smoke fail-closed contract passed"
