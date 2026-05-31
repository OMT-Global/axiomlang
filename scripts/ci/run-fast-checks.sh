#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

bash scripts/ci/check-python-exit-docs.sh
bash scripts/ci/test-pr-fast-ci-workflow.sh
bash scripts/ci/test-validate-capability-manifests.sh
python3 scripts/ci/test-issue-pr-traceability.py
bash scripts/ci/validate-capability-manifests.sh

if [[ "${AXIOM_FAST_CI_PROOF_WORKLOADS:-1}" != "1" ]]; then
  echo "error: proof workload execution is required for PR fast checks." >&2
  exit 1
fi

if ! command -v cc >/dev/null 2>&1; then
  rust_linker="${AXIOM_FAST_CI_RUST_LINKER:-}"
  if [[ -z "$rust_linker" || ! -x "$rust_linker" ]]; then
    echo "error: cc or AXIOM_FAST_CI_RUST_LINKER is required to run proof workloads in PR fast checks." >&2
    exit 1
  fi
fi

for example in proof_cli proof_worker proof_http_service; do
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "stage1/examples/${example}" --json
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "stage1/examples/${example}" --json
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run "stage1/examples/${example}"
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "stage1/examples/${example}" --json
done
