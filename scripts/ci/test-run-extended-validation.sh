#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
runner="$repo_root/scripts/ci/run-toolchain-qualification.py"

for required in full_crate_integration conformance build_purity proof_smoke schemas_protocol lsp_protocol_smoke direct_native_abi runtime_sensitivity benchmark_comparison supply_chain readiness_self_tests; do
  grep -Fq "\"id\": \"$required\"" "$runner" || {
    echo "missing extended qualification lane: $required" >&2
    exit 1
  }
done

python3 "$repo_root/scripts/ci/test-run-toolchain-qualification.py"
