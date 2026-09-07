#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
manifest_path="$repo_root/stage1/Cargo.toml"
sbom_output_dir="${SBOM_OUTPUT_DIR:-$repo_root/stage1/target/sbom}"

if ! command -v cargo-vet >/dev/null 2>&1; then
  echo "cargo-vet is required for supply-chain checks" >&2
  exit 1
fi
if ! command -v cargo-audit >/dev/null 2>&1; then
  echo "cargo-audit 0.22.2 is required for supply-chain checks" >&2
  exit 1
fi

mkdir -p "$sbom_output_dir"

python3 "$repo_root/scripts/ci/check-package-trust-contract.py" --json
bash "$repo_root/scripts/ci/test-check-package-trust-contract.sh"
bash "$repo_root/scripts/ci/test-check-cargo-audit-policy.sh"

cargo test --manifest-path "$manifest_path" -p axiomc --locked --lib package_trust::tests
cargo test --manifest-path "$manifest_path" -p axiomc --locked --lib registry::tests
cargo test --manifest-path "$manifest_path" -p axiomc --locked --test package_trust_cli
cargo test --manifest-path "$manifest_path" -p axiomc --locked --lib package_version::tests
cargo test --manifest-path "$manifest_path" -p axiomc --locked --lib package_resolver::tests
cargo test --manifest-path "$manifest_path" -p axiomc --locked --lib registry_client::tests
cargo test --manifest-path "$manifest_path" -p axiomc --locked --lib package_archive::tests
cargo test --manifest-path "$manifest_path" -p axiomc --locked --lib package_store::tests
cargo test --manifest-path "$manifest_path" -p axiomc --locked --lib package_manager::tests
cargo test --manifest-path "$manifest_path" -p axiomc --locked --test package_resolver_cli
python3 "$repo_root/scripts/ci/check-package-graph-boundary.py" --json
bash "$repo_root/scripts/ci/test-check-package-graph-boundary.sh"

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
cargo_audit_status=0
cargo audit --file "$repo_root/stage1/Cargo.lock" --json >"$sbom_output_dir/stage1.cargo-audit.json" || cargo_audit_status=$?
python3 "$repo_root/scripts/ci/check-cargo-audit-policy.py" \
  --report "$sbom_output_dir/stage1.cargo-audit.json" \
  --policy "$repo_root/stage1/supply-chain/cargo-audit-policy.json"
if (( cargo_audit_status != 0 )); then
  echo "cargo-audit reported findings accepted by the explicit policy" >&2
fi
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
