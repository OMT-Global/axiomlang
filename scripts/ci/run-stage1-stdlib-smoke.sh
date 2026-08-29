#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

temp_reports=()
cleanup() {
  rm -f "${temp_reports[@]}"
}
trap cleanup EXIT

capture_report() {
  local report="$1"
  shift

  if ! "$@" >"$report"; then
    cat "$report" >&2
    exit 1
  fi
}

capture_expected_failure_report() {
  local report="$1"
  shift

  if "$@" >"$report"; then
    echo "expected command to fail closed: $*" >&2
    exit 1
  fi
  [[ -s "$report" ]] || {
    echo "expected failing command to emit a JSON report: $*" >&2
    exit 1
  }
}

assert_ok_report() {
  local report="$1"
  local command_name="$2"
  local project="$3"

  python3 - "$report" "$command_name" "$project" <<'PY'
import json
import sys

path, command_name, project = sys.argv[1:4]
payload = json.load(open(path, encoding="utf-8"))
if payload.get("ok") is not True:
    raise SystemExit(f"{command_name} for {project} must pass")
PY
}

assert_cranelift_report() {
  local report="$1"
  local command_name="$2"
  local project="$3"
  local expectation="${4:-direct-native}"

  python3 scripts/ci/validate-stage1-smoke-report.py \
    --report "$report" \
    --command "$command_name" \
    --project "$project" \
    --expect "$expectation"
}

assert_runtime_lowering_required_report() {
  local report="$1"
  local command_name="$2"
  local project="$3"
  local validator_args=(
    --report "$report"
    --command "$command_name"
    --project "$project"
    --expect blocked
  )

  if [[ "$command_name" == "test" && "$project" == */stdlib_testing ]]; then
    validator_args+=(
      --expected-success-case src/json_bench
      --expected-success-case src/json_snapshot_test
      --expected-blocked-case src/collections_slices_property
      --expected-blocked-case src/encoding_url_property
      --expected-blocked-case src/json_object_property
      --expected-blocked-case src/json_roundtrip_property
      --expected-blocked-case src/json_schema_property
      --expected-blocked-case src/json_table_test
      --expected-blocked-case src/log_event_property
      --expected-blocked-case src/outcome_helpers_property
      --expected-blocked-case src/regex_match_property
      --expected-blocked-case src/regex_replace_property
      --expected-blocked-case src/string_builder_property
      --expected-blocked-case src/sync_state_property
      --expected-blocked-case src/testing_helpers_property
    )
  fi
  python3 scripts/ci/validate-stage1-smoke-report.py "${validator_args[@]}"
}

run_stdlib_project() {
  local example="$1"
  local expectation="${2:-direct-native}"
  local project="stage1/examples/${example}"
  local build_report check_report

  check_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-check.XXXXXX")"
  temp_reports+=("$check_report")
  capture_report "$check_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project" --json
  assert_ok_report "$check_report" "check" "$project"

  build_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-build-cranelift.XXXXXX")"
  temp_reports+=("$build_report")
  capture_report "$build_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "$project" --backend cranelift --json
  assert_cranelift_report "$build_report" "build" "$project" "$expectation"

  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run "$project" --backend cranelift
}

run_stdlib_env_project() {
  local project="stage1/examples/stdlib_env"
  local build_report check_report runtime_binary first_output second_output

  check_report="$(mktemp "${TMPDIR:-/tmp}/axiom-stdlib-env-check.XXXXXX")"
  temp_reports+=("$check_report")
  capture_report "$check_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project" --json
  assert_ok_report "$check_report" "check" "$project"

  build_report="$(mktemp "${TMPDIR:-/tmp}/axiom-stdlib-env-build-cranelift.XXXXXX")"
  temp_reports+=("$build_report")
  capture_report "$build_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "$project" --backend cranelift --json
  assert_cranelift_report "$build_report" "build" "$project" "direct-native"

  runtime_binary="$(python3 - "$build_report" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
if payload.get("generated_rust") is not None:
    raise SystemExit("stdlib_env emitted generated Rust")
lowering = payload.get("lowering")
if not isinstance(lowering, dict):
    raise SystemExit("stdlib_env omitted lowering evidence")
if lowering.get("execution_mode") != "direct_native_runtime":
    raise SystemExit(f"stdlib_env did not use direct-native runtime: {lowering}")
if lowering.get("direct_native_runtime") is not True:
    raise SystemExit(f"stdlib_env did not prove direct-native runtime: {lowering}")
if lowering.get("known_value_static_folds") is not False:
    raise SystemExit(f"stdlib_env used static-value folding: {lowering}")
if lowering.get("legacy_fallback_attempted") is not False:
    raise SystemExit(f"stdlib_env attempted a legacy fallback: {lowering}")
binary = payload.get("binary")
if not isinstance(binary, str) or not binary:
    raise SystemExit(f"stdlib_env omitted runtime binary: {payload}")
print(binary)
PY
)"

  first_output="$(mktemp "${TMPDIR:-/tmp}/axiom-stdlib-env-first-run.XXXXXX")"
  temp_reports+=("$first_output")
  capture_report "$first_output" \
    env "__AXIOM_STAGE1_MISSING__=first-runtime-value" "$runtime_binary"

  second_output="$(mktemp "${TMPDIR:-/tmp}/axiom-stdlib-env-second-run.XXXXXX")"
  temp_reports+=("$second_output")
  capture_report "$second_output" \
    env "__AXIOM_STAGE1_MISSING__=second-runtime-value" "$runtime_binary"

  python3 - "$first_output" "$second_output" <<'PY'
from pathlib import Path
import sys

first, second = (Path(path).read_text(encoding="utf-8") for path in sys.argv[1:])
if first != "first-runtime-value\n" or second != "second-runtime-value\n":
    raise SystemExit(
        "stdlib_env did not produce runtime-sensitive output: "
        f"first={first!r}, second={second!r}"
    )
if first == second:
    raise SystemExit("stdlib_env output did not change with the runtime environment")
PY
}

run_stdlib_test() {
  local example="$1"
  shift
  local expectation="${1:-direct-native}"
  if [[ $# -gt 0 ]]; then
    shift
  fi
  local project="stage1/examples/${example}"
  local test_report

  test_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-test-cranelift.XXXXXX")"
  temp_reports+=("$test_report")
  capture_report "$test_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project" --backend cranelift "$@" --json
  assert_cranelift_report "$test_report" "test" "$project" "$expectation"
}

run_fail_closed_stdlib_project() {
  local example="$1"
  local project="stage1/examples/${example}"
  local build_report check_report

  check_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-check.XXXXXX")"
  temp_reports+=("$check_report")
  capture_report "$check_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project" --json
  assert_ok_report "$check_report" "check" "$project"

  build_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-build-cranelift.XXXXXX")"
  temp_reports+=("$build_report")
  capture_expected_failure_report "$build_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "$project" --backend cranelift --json
  assert_runtime_lowering_required_report "$build_report" "build" "$project"
}

run_fail_closed_stdlib_test() {
  local example="$1"
  shift
  local project="stage1/examples/${example}"
  local test_report

  test_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-test-cranelift.XXXXXX")"
  temp_reports+=("$test_report")
  capture_expected_failure_report "$test_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project" --backend cranelift "$@" --json
  assert_runtime_lowering_required_report "$test_report" "test" "$project"
}

for example in stdlib_time stdlib_testing; do
  run_stdlib_project "$example"
done

for example in \
  stdlib_json \
  stdlib_regex \
  stdlib_collections \
  stdlib_string_builder \
  stdlib_sync; do
  run_stdlib_project "$example" "bounded-static"
done

run_stdlib_env_project

for example in \
  stdlib_fs \
  stdlib_net \
  stdlib_process \
  stdlib_crypto_hash \
  stdlib_io \
  stdlib_log \
  stdlib_async \
  stdlib_http; do
  run_fail_closed_stdlib_project "$example"
done

for example in \
  stdlib_regex \
  stdlib_collections \
  stdlib_string_builder \
  stdlib_log \
  stdlib_sync; do
  run_stdlib_test "$example" "bounded-static"
done

run_fail_closed_stdlib_test "stdlib_async"
run_fail_closed_stdlib_test "stdlib_testing" --include-benchmarks

echo "stage1 stdlib smoke validated declared Cranelift execution modes or fail-closed lowering without generated Rust"
