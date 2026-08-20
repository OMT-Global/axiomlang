#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

temp_reports=()
cleanup() {
  local report
  for report in "${temp_reports[@]-}"; do
    [[ -n "$report" ]] && rm -f "$report"
  done
  rm -rf stage1/examples/proof_worker/scratch
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
  shift 3

  python3 scripts/ci/validate-stage1-smoke-report.py \
    --report "$report" \
    --command "$command_name" \
    --project "$project" \
    --expect blocked \
    "$@"
}

capture_report() {
  local report="$1"
  shift

  if ! "$@" >"$report"; then
    cat "$report" >&2
    return 1
  fi
  [[ -s "$report" ]] || { echo "missing JSON report" >&2; return 1; }
}

capture_expected_failure_report() {
  local report="$1"
  shift

  if "$@" >"$report"; then
    echo "expected command to fail closed: $*" >&2
    return 1
  fi
  [[ -s "$report" ]] || {
    echo "expected failing command to emit a JSON report: $*" >&2
    return 1
  }
}

run_cranelift_workload() {
  local example="$1"
  local test_filter="${2:-}"
  local project="stage1/examples/${example}"
  local build_report check_report test_report

  check_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-check.XXXXXX")"
  temp_reports+=("$check_report")
  capture_report "$check_report" \
    cargo run --locked --manifest-path stage1/Cargo.toml -p axiomc -- check "$project" --json
  assert_ok_report "$check_report" "check" "$example"

  build_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-build-cranelift.XXXXXX")"
  temp_reports+=("$build_report")
  capture_expected_failure_report "$build_report" \
    cargo run --locked --manifest-path stage1/Cargo.toml -p axiomc -- build "$project" --backend cranelift --json
  assert_cranelift_report "$build_report" "build" "$example"

  test_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-test-cranelift.XXXXXX")"
  temp_reports+=("$test_report")
  if [[ -n "$test_filter" ]]; then
    capture_expected_failure_report "$test_report" \
      cargo run --locked --manifest-path stage1/Cargo.toml -p axiomc -- test "$project" --backend cranelift --filter "$test_filter" --json
  else
    capture_expected_failure_report "$test_report" \
      cargo run --locked --manifest-path stage1/Cargo.toml -p axiomc -- test "$project" --backend cranelift --json
  fi
  if [[ "$example" == "proof_cli" ]]; then
    assert_cranelift_report "$test_report" "test" "$example" \
      --expected-bounded-static-case src/main_test \
      --expected-blocked-case src/main_test
  else
    assert_cranelift_report "$test_report" "test" "$example" \
      --expected-blocked-case src/main_test
  fi
}

main() {
  cd "$repo_root"
  trap cleanup EXIT

  cleanup

  run_cranelift_workload "proof_cli"
  run_cranelift_workload "proof_worker"
  run_cranelift_workload "proof_http_service" "src/main_test"

  echo "stage1 proof workloads validated direct-native execution or fail-closed lowering without generated Rust"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
