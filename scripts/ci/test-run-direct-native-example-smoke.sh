#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/run-direct-native-example-smoke.sh"
makefile="$repo_root/Makefile"

[[ -f "$script" ]] || {
  echo "missing direct native example smoke runner: $script" >&2
  exit 1
}

grep -Fq 'stage1/examples/stdlib_crypto_random' "$script" || {
  echo "direct native example smoke must cover stdlib_crypto_random" >&2
  exit 1
}

grep -Fq 'stage1/examples/stdlib_crypto_signature' "$script" || {
  echo "direct native example smoke must cover stdlib_crypto_signature" >&2
  exit 1
}

grep -Fq 'stage1/examples/stdlib_crypto_aead' "$script" || {
  echo "direct native example smoke must cover stdlib_crypto_aead" >&2
  exit 1
}

grep -Fq -- '--backend cranelift --json' "$script" || {
  echo "direct native example smoke must build/test with --backend cranelift --json" >&2
  exit 1
}

grep -Fq 'generated_rust: null' "$script" || {
  echo "direct native example smoke must assert generated_rust: null" >&2
  exit 1
}

grep -Fq 'stage1-direct-native-example-smoke:' "$makefile" || {
  echo "Makefile must expose stage1-direct-native-example-smoke" >&2
  exit 1
}

echo "direct native example smoke runner regression cases passed"
