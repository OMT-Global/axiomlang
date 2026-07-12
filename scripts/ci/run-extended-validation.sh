#!/usr/bin/env bash
set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
output_dir="${AXIOM_QUALIFICATION_OUTPUT_DIR:-$repo_root/artifacts/toolchain-qualification}"

mkdir -p "$output_dir"
exec python3 "$repo_root/scripts/ci/run-toolchain-qualification.py" \
  --repo-root "$repo_root" \
  --output-dir "$output_dir" \
  "$@"
