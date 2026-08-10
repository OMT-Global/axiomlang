#!/usr/bin/env bash
set -euo pipefail

script_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo_root="${AXIOM_CHECKOUT_PATH:-$script_repo_root}"
repo_root="$(cd "$repo_root" && pwd)"
cd "$repo_root"

python3 "$script_repo_root/scripts/ci/test-check-compatibility-v1.py"
python3 "$script_repo_root/scripts/ci/test-check-compatibility-corpus-v1.py"
python3 "$script_repo_root/scripts/ci/check-compatibility-corpus-v1.py" --json
cargo test --manifest-path "$repo_root/stage1/Cargo.toml" -p axiomc --test compatibility_v1 --test migration_plan_cli
