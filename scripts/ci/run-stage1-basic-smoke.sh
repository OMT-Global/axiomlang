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

  python3 scripts/ci/validate-stage1-smoke-report.py \
    --report "$report" \
    --command "$command_name" \
    --project "$project" \
    --expect blocked
}

run_smoke_project() {
  local example="$1"
  local package="${2:-}"
  local expectation="${3:-direct-native}"
  local project="stage1/examples/${example}"
  local build_report check_report

  check_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-check.XXXXXX")"
  temp_reports+=("$check_report")
  capture_report "$check_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project" --json
  assert_ok_report "$check_report" "check" "$project"

  build_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-build-cranelift.XXXXXX")"
  temp_reports+=("$build_report")
  if [[ -n "$package" ]]; then
    capture_report "$build_report" \
      cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "$project" --package "$package" --backend cranelift --json
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run "$project" --package "$package" --backend cranelift
  else
    capture_report "$build_report" \
      cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "$project" --backend cranelift --json
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run "$project" --backend cranelift
  fi
  assert_cranelift_report "$build_report" "build" "$project" "$expectation"
}

run_test_project() {
  local example="$1"
  local project="stage1/examples/${example}"
  local test_report

  test_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-test-cranelift.XXXXXX")"
  temp_reports+=("$test_report")
  capture_report "$test_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project" --backend cranelift --json
  assert_cranelift_report "$test_report" "test" "$project"
}

run_fail_closed_project() {
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

run_smoke_project "hello"

for example in arrays slices borrowed_shapes tuples maps structs enums outcomes generic_aggregates; do
  run_smoke_project "$example" "" "bounded-static"
done

for example in modules packages workspace; do
  run_smoke_project "$example"
  run_test_project "$example"
done

run_fail_closed_project "capabilities"
run_smoke_project "workspace_only" "workspace-app"
run_test_project "workspace_only"

echo "stage1 basic smoke validated declared Cranelift execution modes or fail-closed lowering without generated Rust"
