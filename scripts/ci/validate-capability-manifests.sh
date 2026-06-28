#!/usr/bin/env bash
set -euo pipefail

script_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo_root="${AXIOM_CHECKOUT_PATH:-$script_repo_root}"
python3 "$script_repo_root/scripts/ci/validate-capability-manifests.py" --root "$repo_root"
