#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
manifest_path="$repo_root/stage1/Cargo.toml"
sbom_output_dir="${SBOM_OUTPUT_DIR:-$repo_root/stage1/target/sbom}"

if ! command -v cargo-vet >/dev/null 2>&1; then
  echo "cargo-vet is required for supply-chain checks" >&2
  exit 1
fi

mkdir -p "$sbom_output_dir"

if [[ -f "$repo_root/package-lock.json" ]]; then
  if ! command -v npm >/dev/null 2>&1; then
    echo "npm is required to verify signed packages in package-lock.json" >&2
    exit 1
  fi

  npm ci --prefix "$repo_root" --ignore-scripts --no-audit --no-fund
  npm audit signatures --prefix "$repo_root"
fi

cargo fetch --manifest-path "$manifest_path" --locked
cargo metadata --manifest-path "$manifest_path" --format-version 1 --locked --offline >/dev/null
cargo vet --manifest-path "$manifest_path" --locked --frozen

export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1704067200}"
if [[ -n "${RUSTFLAGS:-}" ]]; then
  export RUSTFLAGS="${RUSTFLAGS} --remap-path-prefix=$repo_root=."
else
  export RUSTFLAGS="--remap-path-prefix=$repo_root=."
fi
cargo build --manifest-path "$manifest_path" -p axiomc --locked --release

python3 "$repo_root/scripts/ci/emit-stage1-sbom.py" \
  --manifest-path "$manifest_path" \
  --output "$sbom_output_dir/stage1.spdx.json"
