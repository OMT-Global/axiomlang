#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/run-direct-native-runtime-abi-evidence.sh"
makefile="$repo_root/Makefile"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

[[ -x "$script" ]] || {
  echo "missing executable direct native runtime ABI evidence runner: $script" >&2
  exit 1
}

grep -Fq 'check-direct-native-runtime-abi.py --json' "$script" || {
  echo "evidence runner must validate the direct native runtime ABI manifest" >&2
  exit 1
}

grep -Fq -- '--test cranelift_backend' "$script" || {
  echo "evidence runner must execute the Cranelift backend evidence suite" >&2
  exit 1
}

grep -Fq 'AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER' "$script" || {
  echo "evidence runner must expose a focused test filter for local repair loops" >&2
  exit 1
}

grep -Fq 'AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW' "$script" || {
  echo "evidence runner must expose a row-focused test filter for ABI evidence loops" >&2
  exit 1
}

grep -Fq 'AXIOM_DIRECT_NATIVE_RUNTIME_ABI_STATUS' "$script" || {
  echo "evidence runner must expose a row-status-focused test filter for ABI evidence loops" >&2
  exit 1
}

grep -Fq 'AXIOM_DIRECT_NATIVE_RUNTIME_ABI_DRY_RUN' "$script" || {
  echo "evidence runner must expose a dry-run mode for focused evidence planning" >&2
  exit 1
}

grep -Fq 'dry-run:' "$script" || {
  echo "evidence runner dry-run mode must print the resolved cargo commands" >&2
  exit 1
}

grep -Fq -- '--evidence-row "$AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW"' "$script" || {
  echo "evidence runner must resolve row-focused tests through the ABI checker" >&2
  exit 1
}

grep -Fq -- '--list-evidence-rows' "$script" || {
  echo "evidence runner must resolve row-status tests through the ABI row inventory" >&2
  exit 1
}

grep -Fq 'set only one of AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW, AXIOM_DIRECT_NATIVE_RUNTIME_ABI_STATUS, or AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER' "$script" || {
  echo "evidence runner must reject ambiguous row, status, and test filter combinations" >&2
  exit 1
}

grep -Fq 'direct native runtime ABI evidence row has no tests' "$script" || {
  echo "evidence runner must fail clearly for unknown ABI evidence rows" >&2
  exit 1
}

grep -Fq 'direct native runtime ABI status has no tests' "$script" || {
  echo "evidence runner must fail clearly for ABI statuses without focused tests" >&2
  exit 1
}

grep -Fq 'unknown direct native runtime ABI row status' "$script" || {
  echo "evidence runner must fail clearly for unknown ABI row statuses" >&2
  exit 1
}

if AXIOM_DIRECT_NATIVE_RUNTIME_ABI_STATUS=unknown "$script" >"$temp_dir/unknown-status.out" 2>"$temp_dir/unknown-status.err"; then
  echo "expected unknown ABI row statuses to fail" >&2
  exit 1
fi
grep -Fq 'unknown direct native runtime ABI row status: unknown' "$temp_dir/unknown-status.err"

if AXIOM_DIRECT_NATIVE_RUNTIME_ABI_STATUS=blocked "$script" >"$temp_dir/blocked-status.out" 2>"$temp_dir/blocked-status.err"; then
  echo "expected ABI row statuses without focused tests to fail" >&2
  exit 1
fi
grep -Fq 'direct native runtime ABI status has no tests: blocked' "$temp_dir/blocked-status.err"

AXIOM_DIRECT_NATIVE_RUNTIME_ABI_DRY_RUN=1 \
  AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW=fs.read \
  "$script" >"$temp_dir/row-dry-run.out"
grep -Fq 'dry-run: cargo test --manifest-path stage1/Cargo.toml -p axiomc --test cranelift_backend cranelift_backend_lowers_fs_read_to_runtime_exit_code -- --nocapture --test-threads=1' "$temp_dir/row-dry-run.out"
grep -Fq 'dry-run: cargo test --manifest-path stage1/Cargo.toml -p axiomc --lib cranelift_run_report_executes_without_generated_rust_artifact -- --nocapture' "$temp_dir/row-dry-run.out"

AXIOM_DIRECT_NATIVE_RUNTIME_ABI_DRY_RUN=1 \
  AXIOM_DIRECT_NATIVE_RUNTIME_ABI_STATUS=partial \
  "$script" >"$temp_dir/partial-status-dry-run.out"
grep -Fq 'cranelift_backend_lowers_option_int_match_to_runtime_exit_code' "$temp_dir/partial-status-dry-run.out"
grep -Fq 'cranelift_backend_lowers_array_literal_index_to_runtime_exit_code' "$temp_dir/partial-status-dry-run.out"

AXIOM_DIRECT_NATIVE_RUNTIME_ABI_DRY_RUN=1 \
  AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER=cranelift_backend_lowers_i64_main_to_runtime_exit_code \
  "$script" >"$temp_dir/test-filter-dry-run.out"
grep -Fq 'dry-run: cargo test --manifest-path stage1/Cargo.toml -p axiomc --test cranelift_backend cranelift_backend_lowers_i64_main_to_runtime_exit_code -- --nocapture --test-threads=1' "$temp_dir/test-filter-dry-run.out"

grep -Fq -- '--test-threads=1' "$script" || {
  echo "evidence runner must serialize localhost-backed Cranelift evidence tests" >&2
  exit 1
}

grep -Fq 'cranelift_run_report_executes_without_generated_rust_artifact' "$script" || {
  echo "evidence runner must execute the direct-native run command evidence" >&2
  exit 1
}

grep -Fq 'cranelift_test_case_executes_without_generated_rust_artifact' "$script" || {
  echo "evidence runner must execute the direct-native test command evidence" >&2
  exit 1
}

if grep -Fq 'which("openssl")' "$repo_root/stage1/crates/axiomc/tests/cranelift_backend.rs" ||
  grep -Fq 'Command::new("openssl")' "$repo_root/stage1/crates/axiomc/src/cranelift_backend.rs"; then
  echo "crypto signature evidence must not depend on the OpenSSL CLI" >&2
  exit 1
fi

grep -Fq 'stage1-direct-native-runtime-abi-evidence:' "$makefile" || {
  echo "Makefile must expose stage1-direct-native-runtime-abi-evidence" >&2
  exit 1
}

echo "direct native runtime ABI evidence runner regression cases passed"
