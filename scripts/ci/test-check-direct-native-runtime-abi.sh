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
assert report["status_counts"]["capability_shims"]["partial"] == 22
assert report["blocked_rows"] == []
assert len(report["incomplete_rows"]) == 34
assert len(report["incomplete_rows_by_group"]["value_features"]) == 12
assert len(report["incomplete_rows_by_group"]["capability_shims"]) == 22
assert report["blocked_rows_by_group"] == {
    "value_features": [],
    "capability_shims": [],
}
assert "env.read" in report["incomplete_rows"]
assert "ffi.call" in report["incomplete_rows"]
assert "json.serdes" in report["incomplete_rows"]
assert "crypto.random" in report["incomplete_rows"]
assert "network.dns.resolve" in report["incomplete_rows"]
assert "numeric.scalars" in report["incomplete_rows_by_group"]["value_features"]
assert "process.status" in report["incomplete_rows_by_group"]["capability_shims"]
assert report["evidence_summary"]["value_features"] == {
    "with_evidence": 12,
    "without_evidence": 0,
    "with_runtime_evidence": 5,
    "without_runtime_evidence": 7,
    "with_denial_evidence": 0,
    "without_denial_evidence": 12,
}
assert report["evidence_summary"]["capability_shims"] == {
    "with_evidence": 22,
    "without_evidence": 0,
    "with_runtime_evidence": 9,
    "without_runtime_evidence": 13,
    "with_denial_evidence": 18,
    "without_denial_evidence": 4,
}
assert report["blocker_issues"] == [1001]
assert report["errors"] == []
PY

printf '{' >"$temp_dir/invalid-contract.json"
if python3 "$script" --contract "$temp_dir/invalid-contract.json" --json >"$temp_dir/invalid-report.json"; then
  echo "expected invalid contracts to fail" >&2
  exit 1
fi
python3 - "$temp_dir/invalid-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert report["schema"] == "axiom.direct_native.runtime_abi.check.v1"
assert report["ready"] is False
assert report["incomplete_rows"] == []
assert report["incomplete_rows_by_group"] == {
    "value_features": [],
    "capability_shims": [],
}
assert report["blocked_rows"] == []
assert report["blocked_rows_by_group"] == {
    "value_features": [],
    "capability_shims": [],
}
assert report["evidence_summary"] == {
    "value_features": {
        "with_evidence": 0,
        "without_evidence": 0,
        "with_runtime_evidence": 0,
        "without_runtime_evidence": 0,
        "with_denial_evidence": 0,
        "without_denial_evidence": 0,
    },
    "capability_shims": {
        "with_evidence": 0,
        "without_evidence": 0,
        "with_runtime_evidence": 0,
        "without_runtime_evidence": 0,
        "with_denial_evidence": 0,
        "without_denial_evidence": 0,
    },
}
assert report["errors"]
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
    "fs.read",
    "fs.write",
    "process.status",
    "env.read",
    "clock.now_sleep",
    "ffi.call",
    "regex.match_replace",
    "io.logging_stdio",
):
    runtime_evidence = capability_rows[row_id]["runtime_evidence"]
    assert "stage1/crates/axiomc/src/cranelift_backend.rs" in runtime_evidence
    assert "stage1/crates/axiomc-backend-cranelift/src/lib.rs" in runtime_evidence
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
