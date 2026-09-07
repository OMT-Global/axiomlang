#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workflow="$repo_root/.github/workflows/toolchain-supply-chain.yml"
script="$repo_root/scripts/ci/run-toolchain-supply-chain.sh"

[[ -f "$workflow" ]] || { echo "missing workflow: $workflow" >&2; exit 1; }
[[ -x "$script" ]] || { echo "missing executable script: $script" >&2; exit 1; }
[[ -x "$repo_root/scripts/ci/check-cargo-audit-policy.py" ]] || {
  echo "missing executable cargo-audit policy checker" >&2
  exit 1
}

grep -Fq 'cargo-audit 0.22.2 is required' "$script" || {
  echo "supply-chain script must fail clearly when cargo-audit is unavailable" >&2
  exit 1
}

grep -Fq 'test-check-cargo-audit-policy.sh' "$script" || {
  echo "supply-chain script must run the cargo-audit policy regression suite" >&2
  exit 1
}

grep -Fq 'cargo-audit-policy.json' "$script" || {
  echo "supply-chain script must enforce the versioned cargo-audit policy" >&2
  exit 1
}

grep -Fq 'check-cargo-audit-policy.py' "$script" || {
  echo "supply-chain script must run the cargo-audit policy checker" >&2
  exit 1
}

grep -Fq 'stage1/supply-chain/config.toml' "$workflow" || {
  echo "workflow must read the required cargo-vet version from the supply-chain config" >&2
  exit 1
}

grep -Fq 'install_version="0.10.2"' "$workflow" || {
  echo "workflow must install cargo-vet 0.10.2 for trusted-publisher imports" >&2
  exit 1
}

grep -Fq 'group: toolchain-supply-chain-${{ github.event.pull_request.number || github.ref }}' "$workflow" || {
  echo "workflow must group supply-chain runs by PR or ref for cancellation" >&2
  exit 1
}

grep -Fq 'cancel-in-progress: true' "$workflow" || {
  echo "workflow must cancel superseded supply-chain runs" >&2
  exit 1
}

grep -Fq 'cargo-vet --version' "$workflow" || {
  echo "workflow must validate an existing cargo-vet binary before reusing it" >&2
  exit 1
}

grep -Fq 'Ensure cargo-audit' "$workflow" || {
  echo "workflow must install and validate the pinned RustSec cargo-audit scanner" >&2
  exit 1
}

grep -Fq 'required_version="0.22.2"' "$workflow" || {
  echo "workflow must pin cargo-audit 0.22.2" >&2
  exit 1
}

grep -Fq 'cargo install cargo-audit --version "$required_version" --locked --force' "$workflow" || {
  echo "workflow must force-install cargo-audit at the configured version" >&2
  exit 1
}

grep -Fq '[[ "$installed_version" != "cargo-vet ${install_version}" ]]' "$workflow" || {
  echo "workflow must reject older cargo-vet patch releases" >&2
  exit 1
}

grep -Fq 'cargo install cargo-vet --version "$install_version" --locked --force' "$workflow" || {
  echo "workflow must force-install the configured cargo-vet version on mismatch" >&2
  exit 1
}

if grep -Fq 'install_version="${install_version}.0"' "$workflow"; then
  echo "workflow must not force a .0 cargo-vet patch release for major/minor config versions" >&2
  exit 1
fi

grep -Fq 'install_version="^${install_version}"' "$workflow" || {
  echo "workflow must install a compatible cargo-vet patch range for major/minor config versions" >&2
  exit 1
}

grep -Fq 'required_is_minor=1' "$workflow" || {
  echo "workflow must distinguish major/minor cargo-vet requirements from exact patch requirements" >&2
  exit 1
}

if ! awk '
  /elif \(\( required_is_minor \)\); then/ { in_minor = 1; next }
  in_minor && /install_cargo_vet=1/ { found = 1 }
  in_minor && /elif \[\[ "\$installed_version"/ { in_minor = 0 }
  END { exit found ? 0 : 1 }
' "$workflow"; then
  echo "workflow must force-install a patched cargo-vet binary for major/minor config versions" >&2
  exit 1
fi

grep -Fq 'Ensure Rust linker availability' "$workflow" || {
  echo "workflow must provision a Rust linker before installing cargo-vet" >&2
  exit 1
}

grep -Fq 'timeout-minutes: 90' "$workflow" || {
  echo "workflow must leave enough budget for cold release builds and SBOM emission" >&2
  exit 1
}

grep -Fq 'gcc libc6-dev' "$workflow" || {
  echo "workflow must install gcc/libc headers when runner images lack cc" >&2
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

grep -Fq "hashFiles('package-lock.json') != ''" "$workflow" || {
  echo "workflow must skip Node.js setup when no package-lock.json is present" >&2
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

grep -Fq 'cargo audit --file "$repo_root/stage1/Cargo.lock" --json' "$script" || {
  echo "supply-chain script must emit a RustSec JSON report" >&2
  exit 1
}

grep -Fq 'stage1.cargo-audit.json' "$script" || {
  echo "supply-chain script must retain the RustSec audit report" >&2
  exit 1
}

grep -Fq 'stage1.cargo-audit.json' "$workflow" || {
  echo "workflow must upload the RustSec audit report with the SBOM" >&2
  exit 1
}

grep -Fq 'cargo test --manifest-path "$manifest_path" -p axiomc --locked --lib package_trust::tests' "$script" || {
  echo "supply-chain script must run the locked Package Trust core test suite" >&2
  exit 1
}

grep -Fq 'cargo test --manifest-path "$manifest_path" -p axiomc --locked --lib registry::tests' "$script" || {
  echo "supply-chain script must run the locked registry publish/verify/serve test suite" >&2
  exit 1
}

grep -Fq 'cargo test --manifest-path "$manifest_path" -p axiomc --locked --test package_trust_cli' "$script" || {
  echo "supply-chain script must run the locked pkg verify CLI integration test suite" >&2
  exit 1
}

for resolver_test in \
  'package_version::tests' \
  'package_resolver::tests' \
  'registry_client::tests' \
  'package_archive::tests' \
  'package_store::tests' \
  'package_manager::tests' \
  '--test package_resolver_cli'; do
  grep -Fq -- "$resolver_test" "$script" || {
    echo "supply-chain script must run the Package Resolver v1 test: $resolver_test" >&2
    exit 1
  }
done

grep -Fq 'check-package-graph-boundary.py" --json' "$script" || {
  echo "supply-chain script must run the Package Graph boundary checker" >&2
  exit 1
}

grep -Fq 'test-check-package-graph-boundary.sh' "$script" || {
  echo "supply-chain script must run the Package Graph boundary regression suite" >&2
  exit 1
}

if ! awk '
  /test-check-package-trust-contract\.sh/ { contract = NR }
  /--locked --lib package_trust::tests/ { core = NR }
  /--locked --lib registry::tests/ { registry = NR }
  /--locked --test package_trust_cli/ { cli = NR }
  /package_version::tests/ { resolver = NR }
  /test-check-package-graph-boundary\.sh/ { graph = NR }
  /cargo audit --file/ { audit = NR }
  /check-cargo-audit-policy\.py/ { policy = NR }
  /cargo vet --manifest-path/ { vet = NR }
  /cargo build --manifest-path/ { build = NR }
  END {
    exit !(contract < core && core < registry && registry < cli &&
           cli < resolver && resolver < graph && graph < audit && audit < policy && policy < vet && vet < build)
  }
' "$script"; then
  echo "Package Trust, Package Resolver, and RustSec suites must run after contract checks and before vet/build" >&2
  exit 1
fi

grep -Fq 'cargo build --manifest-path "$manifest_path" -p axiomc --locked --release' "$script" || {
  echo "supply-chain script must perform a locked release build" >&2
  exit 1
}

grep -Fq 'scripts/ci/emit-stage1-sbom.py' "$script" || {
  echo "supply-chain script must emit an SBOM" >&2
  exit 1
}

echo "toolchain supply-chain workflow validation passed"
