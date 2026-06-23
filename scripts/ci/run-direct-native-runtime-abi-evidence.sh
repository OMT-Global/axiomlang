#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

target_dir="${CARGO_TARGET_DIR:-$repo_root/target/direct-native-runtime-abi}"
mkdir -p "$target_dir"
export CARGO_TARGET_DIR="$target_dir"

if ! command -v cargo >/dev/null 2>&1; then
  for cargo_bin_dir in "${CARGO_HOME:-$HOME/.cargo}/bin" /usr/local/cargo/bin; do
    if [[ -x "$cargo_bin_dir/cargo" ]]; then
      export PATH="$cargo_bin_dir:$PATH"
      break
    fi
  done
fi

command -v cargo >/dev/null 2>&1 || {
  echo "cargo is required but was not found in PATH" >&2
  exit 127
}

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
