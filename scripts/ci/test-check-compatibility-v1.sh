#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

python3 scripts/ci/test-check-compatibility-v1.py
cargo test --manifest-path stage1/Cargo.toml -p axiomc --test compatibility_v1
