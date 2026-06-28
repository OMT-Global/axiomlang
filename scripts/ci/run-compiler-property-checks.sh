#!/usr/bin/env bash
set -euo pipefail

script_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo_root="${AXIOM_CHECKOUT_PATH:-$script_repo_root}"
cd "$repo_root"

project_dir="stage1/examples/compiler_properties"
property_floor=100

property_count="$(
  grep -RhoE '^[[:space:]]*property[[:space:]]+fn[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\(' "$project_dir/src" \
    | wc -l \
    | tr -d '[:space:]'
)"

if (( property_count < property_floor )); then
  echo "compiler property corpus has ${property_count} property fn clauses; expected at least ${property_floor}" >&2
  exit 1
fi

echo "compiler property corpus has ${property_count} property fn clauses"

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
run_with_writable_outputs "$project_dir/dist" \
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project_dir" --properties --backend generated-rust --json
rm -rf "$project_dir/dist"
run_with_writable_outputs "$project_dir/dist" \
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project_dir" --properties --backend generated-rust
