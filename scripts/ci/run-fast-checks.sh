#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

bash scripts/ci/test-pr-fast-ci-workflow.sh

for example in proof_cli proof_worker proof_http_service; do
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check "stage1/examples/${example}" --json
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build "stage1/examples/${example}" --json
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run "stage1/examples/${example}"
  cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test "stage1/examples/${example}" --json
done
