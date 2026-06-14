#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

target_dir="${CARGO_TARGET_DIR:-${RUNNER_TEMP:-/tmp}/axiom-direct-native-runtime-abi-target}"
mkdir -p "$target_dir"
export CARGO_TARGET_DIR="$target_dir"

python3 scripts/ci/check-direct-native-runtime-abi.py --json

cargo_args=(
  test
  --manifest-path stage1/Cargo.toml
  -p axiomc
  --test cranelift_backend
)

if [[ -n "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER:-}" ]]; then
  cargo_args+=("$AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER")
fi

cargo_args+=(-- --nocapture --test-threads=1)

cargo "${cargo_args[@]}"

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
