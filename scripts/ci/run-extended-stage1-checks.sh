#!/usr/bin/env bash
set -euo pipefail

make stage1-conformance
rustup target add wasm32-wasip1
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --target wasm32 --json
