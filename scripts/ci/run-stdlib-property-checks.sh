#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

property_log="$(mktemp)"
trap 'rm -f "$property_log"' EXIT

cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_testing --properties 2>&1 | tee "$property_log"
property_ratio="$(sed -nE 's/^([0-9]+\/[0-9]+) properties passed$/\1/p' "$property_log" | tail -n 1)"
if [[ -z "$property_ratio" ]]; then
  echo "error: missing stdlib property summary from axiomc test --properties" >&2
  exit 1
fi
echo "properties passed: $property_ratio"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    echo "### Stdlib property checks"
    echo
    echo "- properties passed: $property_ratio"
  } >>"$GITHUB_STEP_SUMMARY"
fi

stdlib_projects=(
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

for project in "${stdlib_projects[@]}"; do
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project" --json
done
