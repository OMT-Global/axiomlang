#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/run-direct-native-runtime-abi-evidence.sh"
makefile="$repo_root/Makefile"

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
