#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

bash scripts/ci/test-pr-fast-ci-workflow.sh

if [[ "${AXIOM_FAST_CI_PROOF_WORKLOADS:-1}" != "1" ]]; then
  echo "Skipping proof workload execution because no Rust C linker is available."
  exit 0
fi

for example in proof_cli proof_worker proof_http_service; do
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "stage1/examples/${example}" --json
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "stage1/examples/${example}" --json
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run "stage1/examples/${example}"
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "stage1/examples/${example}" --json
done
