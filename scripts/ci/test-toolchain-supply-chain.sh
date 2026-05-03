#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workflow="$repo_root/.github/workflows/toolchain-supply-chain.yml"
script="$repo_root/scripts/ci/run-toolchain-supply-chain.sh"

[[ -f "$workflow" ]] || { echo "missing workflow: $workflow" >&2; exit 1; }
[[ -x "$script" ]] || { echo "missing executable script: $script" >&2; exit 1; }

grep -Fq 'cargo install cargo-vet --locked' "$workflow" || {
  echo "workflow must install cargo-vet with a locked install" >&2
  exit 1
}

grep -Fq 'actions/setup-node@' "$workflow" || {
  echo "workflow must install Node.js for signed package verification" >&2
  exit 1
}

grep -Fq 'node-version: 20' "$workflow" || {
  echo "workflow must pin the expected Node.js major version" >&2
  exit 1
}

grep -Fq 'bash scripts/ci/run-toolchain-supply-chain.sh' "$workflow" || {
  echo "workflow must run the supply-chain validation script" >&2
  exit 1
}

grep -Fq 'stage1/target/sbom/stage1.spdx.json' "$workflow" || {
  echo "workflow must upload the generated SBOM artifact" >&2
  exit 1
}

grep -Fq 'cargo vet --manifest-path "$manifest_path" --locked --frozen' "$script" || {
  echo "supply-chain script must run cargo-vet in locked frozen mode" >&2
  exit 1
}

grep -Fq 'npm ci --prefix "$repo_root" --ignore-scripts --no-audit --no-fund' "$script" || {
  echo "supply-chain script must install Node.js packages without lifecycle scripts" >&2
  exit 1
}

grep -Fq 'npm audit signatures --prefix "$repo_root"' "$script" || {
  echo "supply-chain script must verify signed Node.js packages" >&2
  exit 1
}

grep -Fq 'cargo metadata --manifest-path "$manifest_path" --format-version 1 --locked --offline' "$script" || {
  echo "supply-chain script must verify offline metadata resolution" >&2
  exit 1
}

grep -Fq 'cargo build --manifest-path "$manifest_path" -p axiomc --locked --release' "$script" || {
  echo "supply-chain script must perform a locked release build" >&2
  exit 1
}

grep -Fq 'scripts/ci/emit-stage1-sbom.py' "$script" || {
  echo "supply-chain script must emit an SBOM" >&2
  exit 1
}

echo "toolchain supply-chain workflow validation passed"
