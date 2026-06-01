#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

project_dir="stage1/examples/compiler_properties"
property_floor=100

keep_outputs_writable() {
  local dir="$1"
  while true; do
    chmod -R u+rwX "$dir" 2>/dev/null || true
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
actual_properties="$(rg -c '^property fn ' "$project_dir/src/property_clause_surface_property.ax")"
if [[ "$actual_properties" -lt "$property_floor" ]]; then
  echo "expected at least $property_floor property fn clauses in $project_dir/src/property_clause_surface_property.ax, found $actual_properties" >&2
  exit 1
fi
run_with_writable_outputs "$project_dir/dist" \
  env CARGO_TARGET_DIR="$project_dir/dist/target" cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project_dir" --properties --json
rm -rf "$project_dir/dist"
run_with_writable_outputs "$project_dir/dist" \
  env CARGO_TARGET_DIR="$project_dir/dist/target" cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project_dir" --properties
