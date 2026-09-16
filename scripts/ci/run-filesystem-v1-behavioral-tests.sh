#!/usr/bin/env bash
set -euo pipefail

script_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo_root="${1:-${AXIOM_CHECKOUT_PATH:-$script_repo_root}}"
manifest="$repo_root/stage1/Cargo.toml"

run_required_tests() {
  local label="$1"
  shift
  local output
  if ! output="$("$@" 2>&1)"; then
    printf '%s\n' "$output" >&2
    return 1
  fi
  printf '%s\n' "$output"
  if ! grep -Eq 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored;' <<<"$output"; then
    printf 'filesystem-v1: required evidence filter executed no non-ignored tests: %s\n' "$label" >&2
    return 1
  fi
}

run_required_tests filesystem-schema \
  cargo test --manifest-path "$manifest" -p axiomc --locked --test schema_metadata filesystem_v1_schema_enforces_promotion_boundaries
run_required_tests build-project-scopes \
  cargo test --manifest-path "$manifest" -p axiomc --locked --lib build_project_scopes_fs_ -- --test-threads=1
run_required_tests stdlib-write-helpers \
  cargo test --manifest-path "$manifest" -p axiomc --locked --lib stage1_project_imports_synthetic_stdlib_fs_write_helpers -- --test-threads=1
run_required_tests cranelift-lowers \
  cargo test --manifest-path "$manifest" -p axiomc --locked --test cranelift_backend cranelift_backend_lowers_fs_ -- --test-threads=1
run_required_tests cranelift-denials \
  cargo test --manifest-path "$manifest" -p axiomc --locked --test cranelift_backend cranelift_backend_denies_fs_ -- --test-threads=1
run_required_tests cranelift-rejections \
  cargo test --manifest-path "$manifest" -p axiomc --locked --test cranelift_backend cranelift_backend_rejects_fs_ -- --test-threads=1
run_required_tests native-replace \
  cargo test --manifest-path "$manifest" -p axiomc-backend-cranelift --locked --lib links_i64_exit_program_with_replace_file -- --test-threads=1
