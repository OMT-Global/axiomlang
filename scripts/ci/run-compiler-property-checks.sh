#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/compiler_properties --properties --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/compiler_properties --properties
