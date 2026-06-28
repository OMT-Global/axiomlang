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

run_smoke_project() {
  local example="$1"
  local package="${2:-}"
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
  assert_cranelift_report "$build_report" "build" "$project"
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

for example in hello arrays slices borrowed_shapes tuples maps structs enums outcomes generic_aggregates; do
  run_smoke_project "$example"
done

for example in modules packages workspace; do
  run_smoke_project "$example"
  run_test_project "$example"
done

run_smoke_project "capabilities"
run_smoke_project "workspace_only" "workspace-app"
run_test_project "workspace_only"

echo "stage1 basic smoke passed on cranelift without generated Rust"
