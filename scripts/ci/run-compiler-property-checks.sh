#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
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
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "$project_dir" --properties --json

test_report="$(mktemp "${TMPDIR:-/tmp}/axiom-compiler-property-cranelift.XXXXXX.json")"
rm -rf "$project_dir/dist"
if ! run_with_writable_outputs "$project_dir/dist" \
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "$project_dir" --properties --backend cranelift --json >"$test_report"; then
  cat "$test_report" >&2
  exit 1
fi

python3 - "$test_report" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
if payload.get("backend") != "cranelift":
    raise SystemExit(f"compiler property tests must run on cranelift, got {payload.get('backend')!r}")
if payload.get("ok") is not True:
    raise SystemExit("compiler property tests must pass on cranelift")
for case in payload.get("cases", []):
    if case.get("generated_rust") is not None:
        raise SystemExit(f"compiler property case {case.get('name')} used generated Rust")
PY
