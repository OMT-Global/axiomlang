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
assert report["status_counts"]["value_features"]["partial"] == 12
assert report["status_counts"]["capability_shims"]["blocked"] == 0
assert report["blocked_rows"] == []
assert "fs.read" in report["incomplete_rows"]
assert "owned.move_state" in report["incomplete_rows"]
assert "crypto.aead" in report["incomplete_rows"]
assert report["blocker_issues"] == [928]
assert report["errors"] == []
PY

python3 - "$contract" "$temp_dir/missing-evidence.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle)

contract["value_features"][0].pop("evidence", None)

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY

if python3 "$script" --contract "$temp_dir/missing-evidence.json" --json >"$temp_dir/missing-evidence-report.json"; then
  echo "expected partial rows without evidence to fail" >&2
  exit 1
fi
python3 - "$temp_dir/missing-evidence-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert any("must name evidence" in error for error in report["errors"])
PY

python3 - "$contract" "$temp_dir/stale-evidence-path.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle)

contract["value_features"][0]["evidence"] = ["stage1/runtime-abi/missing-evidence.rs"]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY

if python3 "$script" --contract "$temp_dir/stale-evidence-path.json" --json >"$temp_dir/stale-evidence-path-report.json"; then
  echo "expected stale evidence paths to fail" >&2
  exit 1
fi
python3 - "$temp_dir/stale-evidence-path-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert any("does not exist" in error for error in report["errors"])
PY

python3 - "$contract" "$temp_dir/stale-runtime-evidence-path.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle)

contract["value_features"][0]["runtime_evidence"] = [
    "stage1/runtime-abi/missing-runtime-evidence.rs"
]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY

if python3 "$script" --contract "$temp_dir/stale-runtime-evidence-path.json" --json >"$temp_dir/stale-runtime-evidence-path-report.json"; then
  echo "expected stale runtime evidence paths to fail" >&2
  exit 1
fi
python3 - "$temp_dir/stale-runtime-evidence-path-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert any("runtime_evidence" in error and "does not exist" in error for error in report["errors"])
PY

python3 - "$contract" "$temp_dir/implemented-with-blocker.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle)

contract["value_features"][0]["status"] = "implemented"
contract["value_features"][0]["runtime_evidence"] = [
    "stage1/crates/axiomc/tests/cranelift_backend.rs"
]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY

if python3 "$script" --contract "$temp_dir/implemented-with-blocker.json" --json >"$temp_dir/implemented-with-blocker-report.json"; then
  echo "expected implemented rows with blockers to fail" >&2
  exit 1
fi
python3 - "$temp_dir/implemented-with-blocker-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert any("must not name blockers" in error for error in report["errors"])
PY

python3 - "$contract" "$temp_dir/implemented-without-runtime-evidence.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle)

contract["value_features"][0]["status"] = "implemented"
contract["value_features"][0].pop("blockers", None)
contract["value_features"][0].pop("runtime_evidence", None)

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY

if python3 "$script" --contract "$temp_dir/implemented-without-runtime-evidence.json" --json >"$temp_dir/implemented-without-runtime-evidence-report.json"; then
  echo "expected implemented rows without runtime evidence to fail" >&2
  exit 1
fi
python3 - "$temp_dir/implemented-without-runtime-evidence-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert any("must name runtime_evidence" in error for error in report["errors"])
PY

if python3 "$script" --contract "$contract" --enforce-ready >/dev/null; then
  echo "expected --enforce-ready to fail while direct native runtime ABI rows are incomplete" >&2
  exit 1
fi

echo "direct native runtime ABI contract regression cases passed"
