#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/run-stage1-proof-test.sh"

if grep -Fq -- '--offline' "$script"; then
  echo "shared proof smoke must not require a pre-populated offline Cargo cache" >&2
  exit 1
fi

# shellcheck source=run-stage1-proof-test.sh
source "$script"

temporary="$(mktemp -d "${TMPDIR:-/tmp}/axiom-proof-capture-test.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT

emit_success() {
  printf '{"ok":true}\n'
}

emit_failure() {
  printf '{"ok":false}\n'
  return 7
}

success_report="$temporary/success.json"
failure_report="$temporary/failure.json"
capture_report "$success_report" emit_success
capture_expected_failure_report "$failure_report" emit_failure

if capture_report "$temporary/nonzero-success.json" emit_failure 2>/dev/null; then
  echo "success capture accepted a nonzero command" >&2
  exit 1
fi

if capture_expected_failure_report "$temporary/zero-failure.json" emit_success 2>/dev/null; then
  echo "expected-failure capture accepted a zero command" >&2
  exit 1
fi

if capture_expected_failure_report "$temporary/empty-failure.json" false 2>/dev/null; then
  echo "expected-failure capture accepted an empty report" >&2
  exit 1
fi

echo "stage1 proof smoke exit-status contract passed"
