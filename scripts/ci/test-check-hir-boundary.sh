#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-hir-boundary.py"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

python3 "$script" --json >"$temp_dir/result.json"

python3 - "$temp_dir/result.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["schema"] == "axiom.compiler.hir_ownership_capability.v1"
assert payload["ok"] is True
assert payload["apis"] == 9
assert payload["contracts"] == 6
assert payload["fixtures"] == 5
PY

python3 - "$repo_root/stage1/compiler-contracts/snapshots/hir-ownership-capability.json" "$temp_dir/unexpected-field.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["apis"][0]["rust_module"] = "hir.rs"

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/unexpected-field.json" >"$temp_dir/unexpected.out" 2>"$temp_dir/unexpected.err"; then
  echo "expected schema-invalid HIR boundary output to fail" >&2
  exit 1
fi

grep -q "unexpected fields" "$temp_dir/unexpected.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/hir-ownership-capability.json" "$temp_dir/missing-api.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

for api in payload["apis"]:
    if api["name"] == "compiler.hir.evaluate_borrow_state":
        api["name"] = "compiler.hir.evaluate_alias_state"

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/missing-api.json" >"$temp_dir/missing-api.out" 2>"$temp_dir/missing-api.err"; then
  echo "expected missing HIR API to fail" >&2
  exit 1
fi

grep -q "compiler.hir API set mismatch" "$temp_dir/missing-api.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/hir-ownership-capability.json" "$temp_dir/rust-capture.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["apis"][0]["outputs"].append("rust_lifetime_table")

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/rust-capture.json" >"$temp_dir/rust-capture.out" 2>"$temp_dir/rust-capture.err"; then
  echo "expected Rust-captured HIR output to fail" >&2
  exit 1
fi

grep -q "Rust capture term" "$temp_dir/rust-capture.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/hir-ownership-capability.json" "$temp_dir/missing-forbidden.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["analysis_input"]["forbidden_required_fields"] = [
    "generated_output" if field == "generated_source" else field
    for field in payload["analysis_input"]["forbidden_required_fields"]
]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/missing-forbidden.json" >"$temp_dir/missing-forbidden.out" 2>"$temp_dir/missing-forbidden.err"; then
  echo "expected missing forbidden HIR input to fail" >&2
  exit 1
fi

grep -q "HIR analysis input must forbid host/backend required fields" "$temp_dir/missing-forbidden.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/hir-ownership-capability.json" "$temp_dir/misaligned-capability-output.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

for api in payload["apis"]:
    if api["name"] == "compiler.hir.infer_capability_use":
        api["outputs"] = [
            "capability_use_records" if output == "inferred_capability_use_records" else output
            for output in api["outputs"]
        ]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/misaligned-capability-output.json" >"$temp_dir/misaligned-capability-output.out" 2>"$temp_dir/misaligned-capability-output.err"; then
  echo "expected misaligned inferred capability output to fail" >&2
  exit 1
fi

grep -q "infer_capability_use must expose inferred_capability_use_records" "$temp_dir/misaligned-capability-output.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/hir-ownership-capability.json" "$temp_dir/uncorrelated.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

for contract in payload["contracts"]:
    if contract["name"] == "ownership_state_contract":
        contract["source_correlated"] = False

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/uncorrelated.json" >"$temp_dir/uncorrelated.out" 2>"$temp_dir/uncorrelated.err"; then
  echo "expected non-source-correlated HIR contract to fail" >&2
  exit 1
fi

grep -q "ownership_state_contract must be source-correlated" "$temp_dir/uncorrelated.err"

echo "check-hir-boundary regression cases passed"
