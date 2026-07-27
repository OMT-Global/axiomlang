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
bash "$script_repo_root/scripts/ci/test-extended-validation-workflow.sh"
bash "$script_repo_root/scripts/ci/test-run-extended-stage1-checks.sh"
bash "$script_repo_root/scripts/ci/test-run-compiler-property-checks.sh"
python3 "$script_repo_root/scripts/ci/check-stage1-full-lib-triage.py" \
  --manifest "$repo_root/docs/rust-exit-full-lib-triage.json" \
  --json >/dev/null
bash "$script_repo_root/scripts/ci/test-check-stage1-full-lib-triage.sh"
python3 "$script_repo_root/scripts/ci/render-direct-native-runtime-abi-status.py" \
  --contract "$repo_root/stage1/runtime-abi/direct-native-v0.json" \
  --check-doc "$repo_root/docs/direct-native-runtime-abi-v0.md"
abi_matrix_report="$(mktemp)"
if ! python3 "$script_repo_root/scripts/ci/check-direct-native-runtime-abi.py" \
  --contract "$repo_root/stage1/runtime-abi/direct-native-v0.json" \
  --evidence-test-manifest "$repo_root/stage1/runtime-abi/direct-native-v0-evidence-tests.json" \
  --checkout-root "$repo_root" \
  --coverage-matrix \
  --json >"$abi_matrix_report"; then
  echo "direct-native runtime ABI coverage matrix failed:" >&2
  python3 -c 'import json,sys; [print(e, file=sys.stderr) for e in json.load(open(sys.argv[1])).get("errors", [])]' "$abi_matrix_report" || cat "$abi_matrix_report" >&2
  rm -f "$abi_matrix_report"
  exit 1
fi
rm -f "$abi_matrix_report"
monolith_report="$(mktemp)"
if ! python3 "$script_repo_root/scripts/ci/report-compiler-source-monoliths.py" \
  --checkout-root "$repo_root" \
  --json --check-plan --check-ratchet >"$monolith_report"; then
  echo "compiler source monolith ratchet failed (#1254); shrink the file or update the ceiling in docs/compiler-source-decomposition-plan.md in this PR:" >&2
  python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); [print(e, file=sys.stderr) for e in d.get("plan_check",{}).get("errors",[])+d.get("ratchet_check",{}).get("errors",[])]' "$monolith_report" || cat "$monolith_report" >&2
  rm -f "$monolith_report"
  exit 1
fi
rm -f "$monolith_report"
python3 "$script_repo_root/scripts/ci/test-report-compiler-source-monoliths.py"
bash "$script_repo_root/scripts/ci/test-check-rust-exit-command-surface.sh"
python3 "$script_repo_root/scripts/ci/test-pr-queue-remediation.py"
python3 "$script_repo_root/scripts/ci/test-report-delivery-signals.py"
python3 "$script_repo_root/scripts/ci/test-issue-pr-traceability.py"
# Checker self-tests must run in a CI lane so their harnesses cannot rot
# silently (#1364, #1369). test-pr-fast-ci-workflow.sh enforces that every
# scripts/ci/test-check-*.sh stays wired here.
bash "$script_repo_root/scripts/ci/test-check-python-exit-docs.sh"
bash "$script_repo_root/scripts/ci/test-check-python-exit-readiness.sh"
bash "$script_repo_root/scripts/ci/test-check-rust-exit-readiness.sh"
bash "$script_repo_root/scripts/ci/test-check-self-hosting-language-readiness.sh"
bash "$script_repo_root/scripts/ci/test-check-compatibility-v1.sh"
python3 "$script_repo_root/scripts/ci/check-capability-ledger.py" \
  --checkout-root "$repo_root" --check-docs --json >/dev/null
bash "$script_repo_root/scripts/ci/test-check-capability-ledger.sh"
python3 "$script_repo_root/scripts/ci/check-stdlib-catalog.py" --json >/dev/null
python3 "$script_repo_root/scripts/ci/test-check-stdlib-catalog.py"
cargo test --manifest-path "$repo_root/stage1/Cargo.toml" -p axiomc --test capability_ledger
python3 "$script_repo_root/scripts/ci/check-production-language-readiness.py" \
  --manifest "$repo_root/docs/production-language-readiness.json" \
  --doc "$repo_root/docs/production-language-roadmap.md" \
  --schema-file "$repo_root/stage1/schemas/axiom-production-language-readiness-v1.schema.json" \
  --json --validate-only >/dev/null
bash "$script_repo_root/scripts/ci/test-check-production-language-readiness.sh"
bash "$script_repo_root/scripts/ci/test-check-direct-native-runtime-abi.sh"
bash "$script_repo_root/scripts/ci/test-check-package-graph-boundary.sh"
bash "$script_repo_root/scripts/ci/test-check-diagnostics-syntax-boundary.sh"
bash "$script_repo_root/scripts/ci/test-check-command-lsp-boundary.sh"
bash "$script_repo_root/scripts/ci/test-check-hir-boundary.sh"
bash "$script_repo_root/scripts/ci/test-check-mir-backend-boundary.sh"
bash "$script_repo_root/scripts/ci/test-check-snapshot-bootstrap-readiness.sh"
python3 "$script_repo_root/scripts/ci/test-run-agent-autonomy-benchmark.py"
python3 "$script_repo_root/scripts/ci/run-agent-autonomy-benchmark.py" --subset ci --check >/dev/null
bash "$script_repo_root/scripts/ci/run-stdlib-property-checks.sh"
bash "$script_repo_root/scripts/ci/run-compiler-property-checks.sh"

cargo test --manifest-path stage1/Cargo.toml -p axiomc --lib render_rust_verifies_https_tls_certificates -- --nocapture
cargo test --manifest-path stage1/Cargo.toml -p axiomc --lib render_rust_uses_trusted_crypto_symbol_loading -- --nocapture

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

bash scripts/ci/run-stage1-proof-test.sh
