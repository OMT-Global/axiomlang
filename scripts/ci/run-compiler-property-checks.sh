#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

project_dir="stage1/examples/compiler_properties"

keep_outputs_writable() {
  local dir="$1"
  while true; do
    chmod -R u+w "$dir" 2>/dev/null || true
    sleep 0.1
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
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project_dir" --properties --json
rm -rf "$project_dir/dist"
run_with_writable_outputs "$project_dir/dist" \
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project_dir" --properties
