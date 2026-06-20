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
assert report["contract_status"] == "partial"
assert report["value_feature_count"] == 12
assert report["capability_shim_count"] == 22
assert report["status_counts"]["value_features"]["partial"] == 12
assert report["status_counts"]["capability_shims"]["implemented"] == 16
assert report["status_counts"]["capability_shims"]["partial"] == 6
assert report["blocked_rows"] == []
assert len(report["incomplete_rows"]) == 18
assert "ffi.call" in report["incomplete_rows"]
assert "json.serdes" in report["incomplete_rows"]
assert "crypto.hash" not in report["incomplete_rows"]
assert "crypto.mac" not in report["incomplete_rows"]
assert "crypto.random" not in report["incomplete_rows"]
assert "clock.now_sleep" not in report["incomplete_rows"]
assert "env.read" not in report["incomplete_rows"]
assert "fs.read" not in report["incomplete_rows"]
assert "fs.write" not in report["incomplete_rows"]
assert "process.status" not in report["incomplete_rows"]
assert "sync.primitives" not in report["incomplete_rows"]
assert "regex.match_replace" not in report["incomplete_rows"]
assert "io.logging_stdio" not in report["incomplete_rows"]
assert "network.dns.resolve" not in report["incomplete_rows"]
assert "network.http.client" not in report["incomplete_rows"]
assert "network.http.server" not in report["incomplete_rows"]
assert "network.tcp" not in report["incomplete_rows"]
assert "network.udp" not in report["incomplete_rows"]
assert report["blocker_issues"] == [1001]
assert report["errors"] == []
PY

python3 - "$contract" "$temp_dir/ready-contract.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle)

contract["status"] = "implemented"
for row in contract["value_features"] + contract["capability_shims"]:
    row["status"] = "implemented"
    row.pop("blockers", None)
    row.setdefault("runtime_evidence", ["stage1/crates/axiomc/tests/cranelift_backend.rs"])

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY

python3 - "$contract" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle)

capability_rows = {row["id"]: row for row in contract["capability_shims"]}
for row_id in (
    "clock.now_sleep",
    "crypto.hash",
    "crypto.mac",
    "crypto.random",
    "env.read",
    "fs.read",
    "fs.write",
    "network.dns.resolve",
    "process.status",
    "regex.match_replace",
    "io.logging_stdio",
):
    runtime_evidence = capability_rows[row_id]["runtime_evidence"]
    assert "stage1/crates/axiomc/src/cranelift_backend.rs" in runtime_evidence
    assert "stage1/crates/axiomc-backend-cranelift/src/lib.rs" in runtime_evidence

assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["network.tcp"]["runtime_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["network.udp"]["runtime_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["network.http.client"]["runtime_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["network.http.server"]["runtime_evidence"]
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
  echo "expected implemented rows without evidence to fail" >&2
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
contract["value_features"][0]["blockers"] = [1001]
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
  echo "expected --enforce-ready to fail while direct native runtime ABI rows are partial" >&2
  exit 1
fi

python3 "$script" --contract "$temp_dir/ready-contract.json" --enforce-ready >/dev/null

echo "direct native runtime ABI contract regression cases passed"
