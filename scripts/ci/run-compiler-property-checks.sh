#!/usr/bin/env bash
set -euo pipefail

script_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo_root="${AXIOM_CHECKOUT_PATH:-$script_repo_root}"
cd "$repo_root"

project_dir="stage1/examples/compiler_properties"
property_floor=100

property_count="$(
  grep -RhoE '^[[:space:]]*property[[:space:]]+fn[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\(' "$project_dir/src" \
    | wc -l \
    | tr -d '[:space:]'
)"

if (( property_count < property_floor )); then
  echo "compiler property corpus has ${property_count} property fn clauses; expected at least ${property_floor}" >&2
  exit 1
fi

echo "compiler property corpus has ${property_count} property fn clauses"

keep_outputs_writable() {
  trap - EXIT HUP INT TERM
  local dir="$1"
  while true; do
    chmod -R u+rwX "$dir" 2>/dev/null || true
    if [[ -d "$dir/tests" ]]; then
      find "$dir/tests" -maxdepth 1 -type f ! -name '*.o' ! -name '*.toml' -exec chmod u+x {} + 2>/dev/null || true
    fi
    # Keep generated test artifacts writable while rustc is creating sidecar outputs.
    sleep 0.01
  done
}

run_with_writable_outputs() {
  local dir="$1"
  shift
  mkdir -p "$dir"
  keep_outputs_writable "$dir" &
  local fixer_pid=$!
  local status=0
  if "$@"; then
    status=0
  else
    status=$?
  fi
  kill "$fixer_pid" 2>/dev/null || true
  wait "$fixer_pid" 2>/dev/null || true
  return "$status"
}

rm -rf "$project_dir/dist"
run_with_writable_outputs "$project_dir/dist" \
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project_dir" --properties --json || true

test_report_dir=""
test_report=""
cleanup_test_report() {
  if [[ -n "$test_report_dir" ]]; then
    rm -rf "$test_report_dir"
  fi
}
trap cleanup_test_report EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

# Keep the report directory under the checked-out workspace in Actions. The
# runner fleet may reclaim nested RUNNER_TEMP paths while a long compiler
# subprocess is still producing its JSON report. Keep TMPDIR as the
# local/test-harness default so regression cases can inspect the cleanup
# boundary.
if [[ "${GITHUB_ACTIONS:-}" == "true" && -n "${RUNNER_TEMP:-}" ]]; then
  report_parent="$repo_root"
else
  report_parent="${TMPDIR:-/tmp}"
fi
mkdir -p "$report_parent"
test_report_dir="$(mktemp -d "${report_parent%/}/axiom-compiler-property-cranelift.XXXXXX")"
test_report="$test_report_dir/report.json"
rm -rf "$project_dir/dist"
run_with_writable_outputs "$project_dir/dist" \
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project_dir" --properties --backend cranelift --json >"$test_report" || true

python3 - "$test_report" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
if payload.get("backend") != "cranelift":
    raise SystemExit(f"compiler property tests must run on cranelift, got {payload.get('backend')!r}")
cases = payload.get("cases")
if not isinstance(cases, list) or not cases:
    raise SystemExit("compiler property test run produced no cases")
for case in cases:
    if case.get("kind") != "property":
        raise SystemExit(f"non-property case in --properties output: {case.get('kind')!r}")
    if not isinstance(case.get("name"), str) or not case["name"]:
        raise SystemExit("property case has no stable name")
    if not isinstance(case.get("duration_ms"), int) or case["duration_ms"] < 0:
        raise SystemExit(f"property case {case.get('name')} has invalid duration")
    error = case.get("error") or {}
    tolerated_unexecuted = (
        case.get("ok") is False
        and error.get("code") == "backend.runtime_lowering_required"
    )
    if case.get("binary") is None:
        lowering = case.get("lowering") or {}
        if not tolerated_unexecuted:
            raise SystemExit(f"property case {case.get('name')} was not executed")
        if lowering.get("schema_version") != "axiom.build-lowering-evidence.v1":
            raise SystemExit(f"unexecuted property case {case.get('name')} lacks bounded lowering evidence schema")
        if lowering.get("lowering_mode") != "runtime_lowering_required" or lowering.get("execution_mode") != "not_produced":
            raise SystemExit(f"unexecuted property case {case.get('name')} has inconsistent lowering evidence")
    elif case.get("ok") is True and case.get("exit_code") != 0:
        raise SystemExit(f"executed property case {case.get('name')} reported success without exit_code 0")
for case in cases:
    if case.get("generated_rust") is not None:
        raise SystemExit(f"compiler property case {case.get('name')} used generated Rust")
    if case.get("ok") is False and case.get("error", {}).get("code") != "backend.runtime_lowering_required":
        raise SystemExit(f"compiler property case {case.get('name')} failed unexpectedly")
PY
