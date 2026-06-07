#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-direct-native-runtime-abi.py"
contract="$repo_root/stage1/runtime-abi/direct-native-v0.json"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

python3 "$script" --contract "$contract" --json >"$temp_dir/report.json"
python3 - "$temp_dir/report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert report["schema"] == "axiom.direct_native.runtime_abi.check.v1"
assert report["ready"] is False
assert report["target_id"] == "axiom://target/stage1-direct-native"
assert report["value_feature_count"] == 12
assert report["capability_shim_count"] == 22
assert "fs.read" in report["blocked_rows"]
assert "owned.move_state" in report["blocked_rows"]
assert report["errors"] == []
PY

if python3 "$script" --contract "$contract" --enforce-ready >/dev/null; then
  echo "expected --enforce-ready to fail while direct native runtime ABI rows are blocked" >&2
  exit 1
fi

echo "direct native runtime ABI contract regression cases passed"
