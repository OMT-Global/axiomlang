#!/usr/bin/env bash
set -euo pipefail
export RUST_MIN_STACK="${RUST_MIN_STACK:-8388608}"
bash scripts/ci/check-python-exit-docs.sh
bash scripts/ci/test-pr-fast-ci-workflow.sh
bash scripts/ci/test-toolchain-supply-chain.sh
bash scripts/ci/test-validate-pr-description.sh
make stage1-test
make stage1-conformance
rustup target add wasm32-wasip1
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --target wasm32 --json
make stage1-smoke
