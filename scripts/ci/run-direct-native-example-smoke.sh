#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

target_dir="${CARGO_TARGET_DIR:-${RUNNER_TEMP:-/tmp}/axiom-direct-native-example-smoke-target}"
mkdir -p "$target_dir"
export CARGO_TARGET_DIR="$target_dir"

temp_reports=()
cleanup() {
  rm -f "${temp_reports[@]}"
}
trap cleanup EXIT

examples=(
  stage1/examples/stdlib_collection_lookup
  stage1/examples/stdlib_collections
  stage1/examples/stdlib_crypto_hash
  stage1/examples/stdlib_crypto_mac
  stage1/examples/stdlib_crypto_random
  stage1/examples/stdlib_crypto_signature
  stage1/examples/stdlib_crypto_aead
  stage1/examples/stdlib_json
  stage1/examples/stdlib_outcome
  stage1/examples/stdlib_regex
  stage1/examples/stdlib_string_builder
)

assert_generated_rust_null() {
  local report_path="$1"
  local command_name="$2"
  local project="$3"

  python3 - "$report_path" "$command_name" "$project" <<'PY'
import json
import sys

report_path, command_name, project = sys.argv[1:]
with open(report_path, encoding="utf-8") as handle:
    payload = json.load(handle)

if payload.get("backend") != "cranelift":
    raise SystemExit(f"{command_name} for {project} did not use cranelift backend")
if payload.get("generated_rust") is not None:
    raise SystemExit(
        f"{command_name} for {project} must report generated_rust: null"
    )
PY
}

for project in "${examples[@]}"; do
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project" --json

  build_report="$(mktemp)"
  temp_reports+=("$build_report")
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "$project" --backend cranelift --json >"$build_report"
  assert_generated_rust_null "$build_report" "build" "$project"

  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run "$project" --backend cranelift

  test_report="$(mktemp)"
  temp_reports+=("$test_report")
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project" --backend cranelift --json >"$test_report"
  assert_generated_rust_null "$test_report" "test" "$project"
done

echo "direct native example smoke passed"
