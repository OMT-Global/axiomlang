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
for case in payload.get("cases", []):
    if case.get("generated_rust") is not None:
        raise SystemExit(
            f"{command_name} case {case.get('name')} for {project} emitted generated Rust"
        )
PY
}

property_report="$(mktemp)"
temp_reports+=("$property_report")
capture_report "$property_report" \
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_testing \
    --properties --backend cranelift --json
assert_cranelift_report "$property_report" "property test" "stage1/examples/stdlib_testing"

property_ratio="$(
  python3 - "$property_report" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
properties = payload.get("properties", {})
print(f"{properties.get('passed', 0)}/{properties.get('total', 0)}")
PY
)"

echo "properties passed: $property_ratio"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    echo "### Stdlib property checks"
    echo
    echo "- properties passed: $property_ratio"
  } >>"$GITHUB_STEP_SUMMARY"
fi

cranelift_stdlib_projects=(
  stage1/examples/stdlib_async
  stage1/examples/stdlib_cli
  stage1/examples/stdlib_collection_lookup
  stage1/examples/stdlib_collections
  stage1/examples/stdlib_crypto_hash
  stage1/examples/stdlib_crypto_mac
  stage1/examples/stdlib_crypto_random
  stage1/examples/stdlib_crypto_signature
  stage1/examples/stdlib_crypto_aead
  stage1/examples/stdlib_encoding
  stage1/examples/stdlib_env
  stage1/examples/stdlib_fs
  stage1/examples/stdlib_fs_write
  stage1/examples/stdlib_http
  stage1/examples/stdlib_io
  stage1/examples/stdlib_json
  stage1/examples/stdlib_json_value
  stage1/examples/stdlib_lsp
  stage1/examples/stdlib_log
  stage1/examples/stdlib_net
  stage1/examples/stdlib_outcome
  stage1/examples/stdlib_process
  stage1/examples/stdlib_regex
  stage1/examples/stdlib_string_builder
  stage1/examples/stdlib_sync
  stage1/examples/stdlib_testing
)

for project in "${cranelift_stdlib_projects[@]}"; do
  report="$(mktemp)"
  temp_reports+=("$report")
  capture_report "$report" \
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project" --backend cranelift --json
  assert_cranelift_report "$report" "test" "$project"
done
