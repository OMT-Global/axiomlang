#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cleanup_generated_outputs() {
  rm -rf stage1/examples/proof_worker/scratch
}

trap cleanup_generated_outputs EXIT

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

  python3 - "$report" "$command_name" "$project" <<'PY'
import json
import sys

path, command_name, project = sys.argv[1:4]
payload = json.load(open(path, encoding="utf-8"))
if payload.get("backend") != "cranelift":
    raise SystemExit(
        f"{command_name} for {project} must run on cranelift, got {payload.get('backend')!r}"
    )
if payload.get("ok") is not True:
    raise SystemExit(f"{command_name} for {project} must pass on cranelift")
if payload.get("generated_rust") is not None:
    raise SystemExit(f"{command_name} for {project} emitted generated Rust")
for package in payload.get("packages", []):
    if isinstance(package, dict) and package.get("generated_rust") is not None:
        raise SystemExit(
            f"{command_name} package {package.get('package_root')} for {project} emitted generated Rust"
        )
for case in payload.get("cases", []):
    if case.get("generated_rust") is not None:
        raise SystemExit(
            f"{command_name} case {case.get('name')} for {project} emitted generated Rust"
        )
PY
}

assert_generated_rust_compatibility_report() {
  local report="$1"
  local command_name="$2"
  local project="$3"

  python3 - "$report" "$command_name" "$project" <<'PY'
import json
import sys

path, command_name, project = sys.argv[1:4]
payload = json.load(open(path, encoding="utf-8"))
if payload.get("backend") != "generated-rust":
    raise SystemExit(
        f"{command_name} compatibility check for {project} must run on generated-rust, got {payload.get('backend')!r}"
    )
if payload.get("ok") is not True:
    raise SystemExit(f"{command_name} compatibility check for {project} must pass")
cases = payload.get("cases", [])
if not cases:
    raise SystemExit(f"{command_name} compatibility check for {project} did not report any cases")
for case in cases:
    if case.get("generated_rust") is None:
        raise SystemExit(
            f"{command_name} compatibility case {case.get('name')} for {project} did not report generated Rust"
        )
PY
}

capture_report() {
  local report="$1"
  shift

  if ! "$@" >"$report"; then
    cat "$report" >&2
    exit 1
  fi
}

run_cranelift_workload() {
  local example="$1"
  local test_filter="${2:-}"
  local project="stage1/examples/${example}"
  local build_report check_report test_report

  check_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-check.XXXXXX")"
  capture_report "$check_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project" --json
  assert_ok_report "$check_report" "check" "$example"

  build_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-build-cranelift.XXXXXX")"
  capture_report "$build_report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "$project" --backend cranelift --json
  assert_cranelift_report "$build_report" "build" "$example"

  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run "$project" --backend cranelift

  test_report="$(mktemp "${TMPDIR:-/tmp}/axiom-${example}-test-cranelift.XXXXXX")"
  if [[ -n "$test_filter" ]]; then
    capture_report "$test_report" \
      cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project" --backend cranelift --filter "$test_filter" --json
  else
    capture_report "$test_report" \
      cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project" --backend cranelift --json
  fi
  assert_cranelift_report "$test_report" "test" "$example"
}

cleanup_generated_outputs

run_cranelift_workload "proof_cli"
run_cranelift_workload "proof_worker"
run_cranelift_workload "proof_http_service" "src/main_test"

# The live HTTP server fixture still depends on the generated-Rust server runtime.
# Keep the exception filtered so Cranelift remains mandatory for every other proof check above.
http_compat_report="$(mktemp "${TMPDIR:-/tmp}/axiom-proof-http-service-test-generated-rust.XXXXXX")"
capture_report "$http_compat_report" \
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/proof_http_service \
    --backend generated-rust --filter http-service-health --json
assert_generated_rust_compatibility_report "$http_compat_report" "test" "proof_http_service"

echo "stage1 proof workloads passed on cranelift; proof_http_service live-service fixture remains generated-rust compatibility coverage"
