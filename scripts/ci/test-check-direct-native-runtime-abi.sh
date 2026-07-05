#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-direct-native-runtime-abi.py"
contract="$repo_root/stage1/runtime-abi/direct-native-v0.json"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

python3 "$script" --contract "$contract" --json >"$temp_dir/report.json"
python3 "$script" --contract "$contract" --coverage-matrix --json >"$temp_dir/coverage-matrix.json"
python3 - "$temp_dir/report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert report["schema"] == "axiom.direct_native.runtime_abi.check.v1"
assert report["ready"] is True
assert report["target_id"] == "axiom://target/stage1-direct-native"
assert report["contract_status"] == "implemented"
assert report["value_feature_count"] == 12
assert report["capability_shim_count"] == 22
assert report["status_counts"]["value_features"]["implemented"] == 12
assert report["status_counts"]["capability_shims"]["implemented"] == 22
assert report["blocked_rows"] == []
assert report["incomplete_rows"] == []
assert report["blocker_issues"] == []
assert report["errors"] == []
PY

python3 - "$temp_dir/coverage-matrix.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    matrix = json.load(handle)

assert matrix["schema"] == "axiom.direct_native.runtime_abi.coverage_matrix.v1"
assert matrix["ready"] is True
assert matrix["summary"]["value_feature_rows"] == 12
assert matrix["summary"]["capability_shim_rows"] == 22
assert matrix["errors"] == []
rows = {(row["group"], row["row_id"]): row for row in matrix["rows"]}
numeric = rows[("value_features", "numeric.scalars")]
assert numeric["coverage"]["positive_runtime_evidence"]
assert numeric["coverage"]["backend_artifact_evidence"]["generated_rust_absent"] is True
assert numeric["coverage"]["backend_artifact_evidence"]["artifact_assertion_tests"]
assert numeric["validation_command"] == (
    "AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW=numeric.scalars "
    "make stage1-direct-native-runtime-abi-evidence"
)
fs_read = rows[("capability_shims", "fs.read")]
assert fs_read["coverage"]["negative_or_diagnostic_evidence"]
assert fs_read["coverage"]["backend_artifact_evidence"]["focused_tests"]
assert fs_read["coverage"]["backend_artifact_evidence"]["artifact_assertion_tests"]
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
for row_id in ("regex.match_replace", "io.logging_stdio"):
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
contract["value_features"][0]["blockers"] = [1124]

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

if python3 "$script" --contract "$temp_dir/implemented-without-runtime-evidence.json" --coverage-matrix --json >"$temp_dir/implemented-without-runtime-evidence-matrix.json"; then
  echo "expected coverage matrix to fail when runtime evidence is missing" >&2
  exit 1
fi
python3 - "$temp_dir/implemented-without-runtime-evidence-matrix.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    matrix = json.load(handle)

assert matrix["ready"] is False
assert any("runtime evidence" in error for error in matrix["errors"])
PY

python3 - "$repo_root/stage1/runtime-abi/direct-native-v0-evidence-tests.json" "$temp_dir/missing-artifact-assertion-tests.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    manifest = json.load(handle)

manifest["value_features"]["numeric.scalars"] = [
    "cranelift_backend_rejects_process_denial_before_backend_lowering"
]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(manifest, handle)
PY

python3 "$script" \
  --contract "$contract" \
  --evidence-test-manifest "$temp_dir/missing-artifact-assertion-tests.json" \
  --json >"$temp_dir/missing-artifact-assertion-report.json"

if python3 "$script" \
  --contract "$contract" \
  --evidence-test-manifest "$temp_dir/missing-artifact-assertion-tests.json" \
  --coverage-matrix \
  --json >"$temp_dir/missing-artifact-assertion-matrix.json"; then
  echo "expected coverage matrix to fail when focused tests lack generated_rust assertions" >&2
  exit 1
fi
python3 - "$temp_dir/missing-artifact-assertion-report.json" "$temp_dir/missing-artifact-assertion-matrix.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)
with open(sys.argv[2], encoding="utf-8") as handle:
    matrix = json.load(handle)

assert report["ready"] is True
assert report["errors"] == []
assert matrix["ready"] is False
assert any("generated_rust artifact assertions" in error for error in matrix["errors"])
rows = {(row["group"], row["row_id"]): row for row in matrix["rows"]}
numeric = rows[("value_features", "numeric.scalars")]
assert numeric["coverage"]["backend_artifact_evidence"]["focused_tests"]
assert numeric["coverage"]["backend_artifact_evidence"]["artifact_assertion_tests"] == []
assert numeric["coverage"]["backend_artifact_evidence"]["generated_rust_absent"] is False
PY

python3 "$script" --contract "$contract" --enforce-ready >/dev/null

python3 "$script" --contract "$temp_dir/ready-contract.json" --enforce-ready >/dev/null

# Split-checkout regression: when the script runs from a trusted checkout that
# lacks a new evidence file present in the data checkout, evidence paths must
# resolve against --checkout-root instead of the script's own repository.
split_root="$temp_dir/split-checkout"
mkdir -p "$split_root/stage1/runtime-abi" "$split_root/new-evidence"
ln -s "$repo_root/stage1/crates" "$split_root/stage1/crates"
ln -s "$repo_root/stage1/conformance" "$split_root/stage1/conformance"
ln -s "$repo_root/scripts" "$split_root/scripts"
printf '{}\n' >"$split_root/new-evidence/expected-error.json"
python3 - "$contract" "$split_root/stage1/runtime-abi/direct-native-v0.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle)

contract["value_features"][0]["denial_evidence"] = [
    "new-evidence/expected-error.json"
]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY
if python3 "$script" \
  --contract "$split_root/stage1/runtime-abi/direct-native-v0.json" \
  --no-evidence-test-manifest \
  --json >"$temp_dir/split-default-root.json"; then
  echo "expected data-checkout-only evidence to fail without --checkout-root" >&2
  exit 1
fi
python3 - "$temp_dir/split-default-root.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert any("does not exist" in error for error in report["errors"])
PY
python3 "$script" \
  --contract "$split_root/stage1/runtime-abi/direct-native-v0.json" \
  --no-evidence-test-manifest \
  --checkout-root "$split_root" \
  --json >/dev/null

echo "direct native runtime ABI contract regression cases passed"
