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
assert report["capability_shim_count"] == 23
assert report["status_counts"]["value_features"]["implemented"] == 1
assert report["status_counts"]["value_features"]["partial"] == 11
assert report["status_counts"]["capability_shims"]["implemented"] == 23
assert report["status_counts"]["capability_shims"]["partial"] == 0
assert report["blocked_rows"] == []
assert len(report["incomplete_rows"]) == 11
assert report["evidence_test_manifest"]["present"] is True
assert report["evidence_test_manifest"]["value_feature_rows"] == 12
assert report["evidence_test_manifest"]["value_feature_test_count"] >= 40
assert report["evidence_test_manifest"]["capability_shim_rows"] == 23
assert report["evidence_test_manifest"]["capability_shim_test_count"] >= 70
assert "owned.move_state" not in report["incomplete_rows"]
assert "ffi.call" not in report["incomplete_rows"]
assert "json.serdes" not in report["incomplete_rows"]
assert "crypto.hash" not in report["incomplete_rows"]
assert "crypto.mac" not in report["incomplete_rows"]
assert "crypto.random" not in report["incomplete_rows"]
assert "crypto.signature" not in report["incomplete_rows"]
assert "crypto.aead" not in report["incomplete_rows"]
assert "clock.now_sleep" not in report["incomplete_rows"]
assert "env.read" not in report["incomplete_rows"]
assert "fs.read" not in report["incomplete_rows"]
assert "fs.write" not in report["incomplete_rows"]
assert "process.status" not in report["incomplete_rows"]
assert "cli.args" not in report["incomplete_rows"]
assert "sync.primitives" not in report["incomplete_rows"]
assert "regex.match_replace" not in report["incomplete_rows"]
assert "io.logging_stdio" not in report["incomplete_rows"]
assert "network.dns.resolve" not in report["incomplete_rows"]
assert "network.http.client" not in report["incomplete_rows"]
assert "network.http.server" not in report["incomplete_rows"]
assert "network.http.async_server" not in report["incomplete_rows"]
assert "network.tcp" not in report["incomplete_rows"]
assert "network.udp" not in report["incomplete_rows"]
assert "async.runtime" not in report["incomplete_rows"]
assert report["blocker_issues"] == [1124]
assert report["errors"] == []
PY

python3 "$script" --contract "$contract" --list-evidence-rows --json >"$temp_dir/row-list.json"
python3 - "$temp_dir/row-list.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert report["schema"] == "axiom.direct_native.runtime_abi.evidence_rows.v1"
assert report["ready"] is False
assert report["target_id"] == "axiom://target/stage1-direct-native"
assert report["contract_status"] == "partial"
assert report["value_feature_count"] == 12
assert report["capability_shim_count"] == 23
assert len(report["rows"]) == 35
assert report["status_counts"]["value_features"]["implemented"] == 1
assert report["status_counts"]["capability_shims"]["implemented"] == 23
assert report["blocker_issues"] == [1124]
assert report["errors"] == []

rows = {row["row_id"]: row for row in report["rows"]}
assert rows["option"]["group"] == "value_features"
assert rows["option"]["status"] == "partial"
assert rows["option"]["blockers"] == [1124]
assert rows["option"]["test_count"] >= 1
assert "cranelift_backend_lowers_option_int_match_to_runtime_exit_code" in rows["option"]["tests"]
assert "stage1/crates/axiomc/tests/cranelift_backend.rs" in rows["option"]["evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in rows["option"]["runtime_evidence"]
assert "general Option ABI" in rows["option"]["notes"]

assert rows["fs.read"]["group"] == "capability_shims"
assert rows["fs.read"]["status"] == "implemented"
assert rows["fs.read"]["blockers"] == []
assert rows["fs.read"]["test_count"] >= 2
assert "cranelift_backend_lowers_fs_read_to_runtime_exit_code" in rows["fs.read"]["tests"]
assert "stage1/crates/axiomc/tests/cranelift_backend.rs" in rows["fs.read"]["denial_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in rows["fs.read"]["runtime_evidence"]

assert rows["cli.args"]["group"] == "capability_shims"
assert rows["cli.args"]["status"] == "implemented"
assert rows["cli.args"]["blockers"] == []
assert rows["cli.args"]["test_count"] >= 2
assert "cranelift_backend_builds_std_cli_no_args_binary" in rows["cli.args"]["tests"]
assert "cranelift_backend_builds_std_cli_forwarded_args_binary" in rows["cli.args"]["tests"]
assert "stage1/crates/axiomc/tests/cranelift_backend.rs" in rows["cli.args"]["evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in rows["cli.args"]["runtime_evidence"]
assert "stage1/crates/axiomc-backend-cranelift/src/lib.rs" in rows["cli.args"]["runtime_evidence"]
PY

python3 "$script" --contract "$contract" --list-evidence-rows >"$temp_dir/row-list.txt"
grep -Fq "value_features option partial" "$temp_dir/row-list.txt"
grep -Fq "capability_shims fs.read implemented" "$temp_dir/row-list.txt"
grep -Fq "capability_shims cli.args implemented" "$temp_dir/row-list.txt"
grep -Fq "blockers=#1124" "$temp_dir/row-list.txt"
grep -Fq "blockers=-" "$temp_dir/row-list.txt"

if python3 "$script" --contract "$contract" --list-evidence-rows --evidence-row fs.read >"$temp_dir/row-list-conflict.txt" 2>"$temp_dir/row-list-conflict.err"; then
  echo "expected conflicting row inspection modes to fail" >&2
  exit 1
fi
grep -Fq -- "--list-evidence-rows and --evidence-row cannot be combined" "$temp_dir/row-list-conflict.err"

python3 "$script" --contract "$contract" --evidence-row fs.read >"$temp_dir/fs-read-row.txt"
grep -Fxq "cranelift_backend_lowers_fs_read_to_runtime_exit_code" "$temp_dir/fs-read-row.txt"
grep -Fxq "cranelift_backend_denies_fs_read_symlink_escape_at_runtime" "$temp_dir/fs-read-row.txt"

python3 "$script" --contract "$contract" --evidence-row option --json >"$temp_dir/option-row.json"
python3 - "$temp_dir/option-row.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert report["schema"] == "axiom.direct_native.runtime_abi.evidence_row.v1"
assert report["row_id"] == "option"
assert report["group"] == "value_features"
assert report["status"] == "partial"
assert report["blockers"] == [1124]
assert "stage1/crates/axiomc/tests/cranelift_backend.rs" in report["evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in report["runtime_evidence"]
assert "stage1/crates/axiomc-backend-cranelift/src/lib.rs" in report["runtime_evidence"]
assert "general Option ABI" in report["notes"]
assert "cranelift_backend_lowers_option_int_match_to_runtime_exit_code" in report["tests"]
assert report["errors"] == []
PY

python3 "$script" --contract "$contract" --evidence-row fs.read --json >"$temp_dir/fs-read-row.json"
python3 - "$temp_dir/fs-read-row.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert report["row_id"] == "fs.read"
assert report["group"] == "capability_shims"
assert report["status"] == "implemented"
assert report["blockers"] == []
assert "stage1/crates/axiomc/tests/cranelift_backend.rs" in report["evidence"]
assert "stage1/crates/axiomc/tests/cranelift_backend.rs" in report["denial_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in report["runtime_evidence"]
assert "cranelift_backend_lowers_fs_read_to_runtime_exit_code" in report["tests"]
assert report["errors"] == []
PY

if python3 "$script" --contract "$contract" --evidence-row missing.row >"$temp_dir/missing-row.txt" 2>"$temp_dir/missing-row.err"; then
  echo "expected unknown evidence rows to fail" >&2
  exit 1
fi
grep -Fq "unknown direct native runtime ABI evidence row: missing.row" "$temp_dir/missing-row.err"
grep -Fq "unknown direct native runtime ABI contract row: missing.row" "$temp_dir/missing-row.err"

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

value_rows = {row["id"]: row for row in contract["value_features"]}
capability_rows = {row["id"]: row for row in contract["capability_shims"]}
assert "stage1/crates/axiomc/src/hir.rs" in value_rows["owned.move_state"]["runtime_evidence"]
for row_id in (
    "boolean",
    "enum.payload",
    "numeric.scalars",
    "option",
    "result",
):
    runtime_evidence = value_rows[row_id]["runtime_evidence"]
    assert "stage1/crates/axiomc/src/cranelift_backend.rs" in runtime_evidence
    assert "stage1/crates/axiomc-backend-cranelift/src/lib.rs" in runtime_evidence

for row_id in (
    "array.fixed",
    "map.lookup",
    "slice.borrowed",
    "string",
    "struct.field",
    "tuple",
):
    assert "stage1/crates/axiomc/src/cranelift_backend.rs" in value_rows[row_id]["runtime_evidence"]

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
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["network.http.async_server"]["runtime_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["cli.args"]["runtime_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["crypto.signature"]["runtime_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["crypto.aead"]["runtime_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["ffi.call"]["runtime_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["async.runtime"]["runtime_evidence"]
assert "stage1/crates/axiomc/src/cranelift_backend.rs" in capability_rows["json.serdes"]["runtime_evidence"]
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

cp "$repo_root/stage1/runtime-abi/direct-native-v0-evidence-tests.json" "$temp_dir/stale-evidence-tests.json"
python3 - "$temp_dir/stale-evidence-tests.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    manifest = json.load(handle)

manifest["value_features"]["numeric.scalars"][0] = "missing_cranelift_runtime_abi_test"
manifest["capability_shims"]["fs.read"][0] = "missing_cranelift_capability_abi_test"

with open(sys.argv[1], "w", encoding="utf-8") as handle:
    json.dump(manifest, handle)
PY

if python3 "$script" \
  --contract "$contract" \
  --evidence-test-manifest "$temp_dir/stale-evidence-tests.json" \
  --json >"$temp_dir/stale-evidence-tests-report.json"; then
  echo "expected stale focused evidence test names to fail" >&2
  exit 1
fi
python3 - "$temp_dir/stale-evidence-tests-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert any("names missing test" in error for error in report["errors"])
assert any(
    "capability_shims" in error and "names missing test" in error
    for error in report["errors"]
)
PY

python3 - "$contract" "$temp_dir/implemented-with-blocker.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle)

contract["value_features"][0]["status"] = "implemented"
contract["value_features"][0]["blockers"] = [1124]
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
