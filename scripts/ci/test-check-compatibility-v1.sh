#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# Keep checker self-tests on the trusted script checkout, but compile the same
# PR checkout as the rest of run-fast-checks.sh. Compiling base and head into
# its shared Cargo target can otherwise reuse a stale path-dependency artifact.
checkout_root="${AXIOM_CHECKOUT_PATH:-$repo_root}"
cd "$repo_root"

AXIOM_CHECKOUT_PATH="$repo_root" python3 scripts/ci/test-check-compatibility-v1.py
AXIOM_CHECKOUT_PATH="$repo_root" python3 scripts/ci/test-check-compatibility-corpus-v1.py
python3 scripts/ci/check-compatibility-corpus-v1.py --json
cargo test --manifest-path "$checkout_root/stage1/Cargo.toml" -p axiomc --test compatibility_v1 --test migration_plan_cli
