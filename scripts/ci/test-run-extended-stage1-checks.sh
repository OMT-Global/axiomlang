#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/run-extended-stage1-checks.sh"

grep -Fq -- 'test --conformance --backend cranelift --json' "$script" || {
  echo "extended stage1 checks must run conformance on the direct-native backend" >&2
  exit 1
}

grep -Fq 'direct-native conformance must not emit generated Rust' "$script" || {
  echo "extended stage1 checks must assert generated_rust stays null" >&2
  exit 1
}

if grep -Eq 'mktemp .*[.]XXXXXX[.]' "$script"; then
  echo "extended stage1 checks must use BSD-compatible mktemp templates" >&2
  exit 1
fi

grep -Fq 'conformance case' "$script" || {
  echo "extended stage1 checks must inspect per-case generated_rust artifacts" >&2
  exit 1
}

grep -Fq 'default targeted builds must not silently fall back to generated Rust' "$script" || {
  echo "extended stage1 checks must preserve targeted-build fail-closed coverage" >&2
  exit 1
}

echo "run-extended-stage1-checks regression cases passed"
