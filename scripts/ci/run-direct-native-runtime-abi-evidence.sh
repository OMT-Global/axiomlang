#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

target_dir="${CARGO_TARGET_DIR:-${RUNNER_TEMP:-/tmp}/axiom-direct-native-runtime-abi-target}"
mkdir -p "$target_dir"
export CARGO_TARGET_DIR="$target_dir"
row_test_file=""
trap '[[ -z "${row_test_file:-}" ]] || rm -f "$row_test_file"' EXIT

python3 scripts/ci/check-direct-native-runtime-abi.py --json

if [[ -n "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW:-}" && -n "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER:-}" ]]; then
  echo "set either AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW or AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER, not both" >&2
  exit 1
fi

cranelift_cargo_base=(
  cargo
  test
  --manifest-path stage1/Cargo.toml
  -p axiomc
  --test cranelift_backend
)

if [[ -n "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW:-}" ]]; then
  row_test_file="$target_dir/abi-row-tests-$$.txt"
  row_tests=()
  python3 - "$AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW" >"$row_test_file" <<'PY'
import json
import sys
from pathlib import Path

row_id = sys.argv[1]
manifest_path = Path("stage1/runtime-abi/direct-native-v0-evidence-tests.json")
with manifest_path.open(encoding="utf-8") as handle:
    manifest = json.load(handle)

tests = (
    manifest.get("value_features", {}).get(row_id)
    or manifest.get("capability_shims", {}).get(row_id)
)
if not tests:
    raise SystemExit(f"unknown direct native runtime ABI evidence row: {row_id}")

for test in tests:
    print(test)
PY

  while IFS= read -r test_name; do
    row_tests+=("$test_name")
  done < "$row_test_file"
  rm -f "$row_test_file"

  if ((${#row_tests[@]} == 0)); then
    echo "direct native runtime ABI evidence row has no tests: $AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW" >&2
    exit 1
  fi

  for row_test in "${row_tests[@]}"; do
    "${cranelift_cargo_base[@]}" "$row_test" -- --nocapture --test-threads=1
  done
else
  cargo_args=("${cranelift_cargo_base[@]}")

  if [[ -n "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER:-}" ]]; then
    cargo_args+=("$AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER")
  fi

  cargo_args+=(-- --nocapture --test-threads=1)

  "${cargo_args[@]}"
fi

for command_evidence in \
  cranelift_run_report_executes_without_generated_rust_artifact \
  cranelift_test_case_executes_without_generated_rust_artifact
do
  cargo test \
    --manifest-path stage1/Cargo.toml \
    -p axiomc \
    --lib \
    "$command_evidence" \
    -- --nocapture
done
