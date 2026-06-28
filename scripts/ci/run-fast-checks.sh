#!/usr/bin/env bash
set -euo pipefail

script_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo_root="${AXIOM_CHECKOUT_PATH:-$script_repo_root}"
cd "$repo_root"

target_dir="${CARGO_TARGET_DIR:-${RUNNER_TEMP:-/tmp}/axiom-fast-ci-target}"
mkdir -p "$target_dir"
export CARGO_TARGET_DIR="$target_dir"

bash "$script_repo_root/scripts/ci/check-python-exit-docs.sh"
bash "$script_repo_root/scripts/ci/validate-capability-manifests.sh"
bash "$script_repo_root/scripts/ci/test-validate-capability-manifests.sh"
bash "$script_repo_root/scripts/ci/test-pr-fast-ci-workflow.sh"
python3 "$script_repo_root/scripts/ci/render-direct-native-runtime-abi-status.py" \
  --contract "$repo_root/stage1/runtime-abi/direct-native-v0.json" \
  --check-doc "$repo_root/docs/direct-native-runtime-abi-v0.md"
python3 "$script_repo_root/scripts/ci/test-pr-queue-remediation.py"
python3 "$script_repo_root/scripts/ci/test-report-delivery-signals.py"
python3 "$script_repo_root/scripts/ci/test-issue-pr-traceability.py"
bash "$script_repo_root/scripts/ci/run-stdlib-property-checks.sh"
bash "$script_repo_root/scripts/ci/run-compiler-property-checks.sh"

cargo test --manifest-path stage1/Cargo.toml -p axiomc render_rust_verifies_https_tls_certificates -- --nocapture
cargo test --manifest-path stage1/Cargo.toml -p axiomc render_rust_uses_trusted_crypto_symbol_loading -- --nocapture

if [[ "${AXIOM_FAST_CI_PROOF_WORKLOADS:-1}" != "1" ]]; then
  echo "error: proof workload execution is required for PR fast checks." >&2
  exit 1
fi

rust_linker="${AXIOM_FAST_CI_RUST_LINKER:-}"
smoke_linker() {
  local linker="$1"
  local smoke_rs smoke_bin
  smoke_rs="${RUNNER_TEMP:-/tmp}/axiom-fast-ci-linker-smoke.rs"
  smoke_bin="${RUNNER_TEMP:-/tmp}/axiom-fast-ci-linker-smoke"
  printf '%s\n' 'fn main() {}' > "$smoke_rs"
  rustc -C "linker=$linker" "$smoke_rs" -o "$smoke_bin" >/dev/null 2>&1
}

if [[ -n "$rust_linker" ]]; then
  if [[ ! -x "$rust_linker" ]] || ! smoke_linker "$rust_linker"; then
    echo "error: AXIOM_FAST_CI_RUST_LINKER must point to a usable Rust linker for PR fast checks." >&2
    exit 1
  fi
else
  for candidate in cc gcc clang; do
    if rust_linker="$(command -v "$candidate" 2>/dev/null)" && [[ -n "$rust_linker" ]] && smoke_linker "$rust_linker"; then
      export AXIOM_FAST_CI_RUST_LINKER="$rust_linker"
      break
    fi
  done
fi

if [[ -z "$rust_linker" ]]; then
  echo "error: no usable cc, gcc, clang, or AXIOM_FAST_CI_RUST_LINKER was found for PR fast checks." >&2
  exit 1
fi

for example in proof_cli proof_worker proof_http_service; do
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "stage1/examples/${example}" --json
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "stage1/examples/${example}" --json
  # The HTTP service proof workload is covered by its generated test suite and
  # does not terminate cleanly under `run`, so keep fast checks bounded.
  if [[ "$example" != "proof_http_service" ]]; then
    cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run "stage1/examples/${example}"
  fi
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "stage1/examples/${example}" --json
done
