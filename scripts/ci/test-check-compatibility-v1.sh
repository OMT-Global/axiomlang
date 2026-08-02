#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

python3 scripts/ci/test-check-compatibility-v1.py
python3 scripts/ci/test-check-compatibility-corpus-v1.py
python3 scripts/ci/check-compatibility-corpus-v1.py --json
cargo test --manifest-path stage1/Cargo.toml -p axiomc --test compatibility_v1 --test migration_plan_cli
