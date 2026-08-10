#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/run-stage1-stdlib-smoke.sh"

python3 - "$script" <<'PY'
import pathlib
import re
import sys

source = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
required = (
    "capture_expected_failure_report",
    "validate-stage1-smoke-report.py",
    '--expect "$expectation"',
    "run_stdlib_env_project",
    '__AXIOM_STAGE1_MISSING__=first-runtime-value',
    '__AXIOM_STAGE1_MISSING__=second-runtime-value',
    '"execution_mode") != "direct_native_runtime"',
    '"direct_native_runtime") is not True',
    '"known_value_static_folds") is not False',
    '"generated_rust") is not None',
    "stdlib_env did not produce runtime-sensitive output",
    '"bounded-static"',
    "--expect blocked",
    "run_fail_closed_stdlib_project",
    'run_fail_closed_stdlib_test "stdlib_async"',
    'run_fail_closed_stdlib_test "stdlib_testing" --include-benchmarks',
    'run_stdlib_project "$example" "bounded-static"',
    'run_stdlib_test "$example" "bounded-static"',
    "--expected-success-case src/json_bench",
    "--expected-success-case src/json_snapshot_test",
    "--expected-blocked-case src/collections_slices_property",
    "--expected-blocked-case src/testing_helpers_property",
)
missing = [fragment for fragment in required if fragment not in source]
if missing:
    raise SystemExit(
        "stdlib smoke fail-closed contract is incomplete: " + ", ".join(missing)
    )

fail_closed = {
    "stdlib_fs",
    "stdlib_net",
    "stdlib_process",
    "stdlib_crypto_hash",
    "stdlib_io",
    "stdlib_log",
    "stdlib_async",
    "stdlib_http",
}
marker = '  run_fail_closed_stdlib_project "$example"'
if marker not in source:
    raise SystemExit("stdlib smoke fail-closed project loop is missing")
before_marker = source.split(marker, 1)[0]
loop = before_marker.rsplit("for example in \\", 1)[-1]
actual = set(re.findall(r"\bstdlib_[a-z_]+\b", loop))
if actual != fail_closed:
    raise SystemExit(
        f"stdlib fail-closed projects drifted: expected {sorted(fail_closed)}, "
        f"got {sorted(actual)}"
    )
PY

echo "stage1 stdlib smoke fail-closed contract passed"
