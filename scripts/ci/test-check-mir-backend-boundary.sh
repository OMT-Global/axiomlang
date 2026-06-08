#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-mir-backend-boundary.py"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

python3 "$script" --json >"$temp_dir/result.json"

python3 - "$temp_dir/result.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["schema"] == "axiom.compiler.mir_backend.v1"
assert payload["ok"] is True
assert payload["packages"] == 4
assert payload["targets"] == 2
PY

python3 - "$repo_root/stage1/compiler-contracts/snapshots/mir-backend.json" "$temp_dir/unexpected-field.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["targets"][0]["unexpected"] = "drift"

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/unexpected-field.json" >"$temp_dir/unexpected.out" 2>"$temp_dir/unexpected.err"; then
  echo "expected schema-invalid MIR/backend boundary output to fail" >&2
  exit 1
fi

grep -q "unexpected fields" "$temp_dir/unexpected.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/mir-backend.json" "$temp_dir/native-rust-source.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

for target in payload["targets"]:
    if target["id"] == "axiom://target/stage1-direct-native":
        target["primary_artifacts"].append("rust_source")

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/native-rust-source.json" >"$temp_dir/native-rust-source.out" 2>"$temp_dir/native-rust-source.err"; then
  echo "expected direct-native rust_source artifact to fail" >&2
  exit 1
fi

grep -q "direct native primary artifacts must not include rust_source" "$temp_dir/native-rust-source.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/mir-backend.json" "$temp_dir/missing-forbidden.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["backend_input"]["forbidden_required_fields"] = [
    field for field in payload["backend_input"]["forbidden_required_fields"]
    if field != "rustc_command"
]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/missing-forbidden.json" >"$temp_dir/missing-forbidden.out" 2>"$temp_dir/missing-forbidden.err"; then
  echo "expected missing Rust-derived forbidden field to fail" >&2
  exit 1
fi

grep -q "backend input must forbid Rust-derived required fields" "$temp_dir/missing-forbidden.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/mir-backend.json" "$temp_dir/missing-cranelift-forbidden.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["backend_input"]["forbidden_required_fields"] = [
    field for field in payload["backend_input"]["forbidden_required_fields"]
    if field != "cranelift_module_path"
]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/missing-cranelift-forbidden.json" >"$temp_dir/missing-cranelift-forbidden.out" 2>"$temp_dir/missing-cranelift-forbidden.err"; then
  echo "expected missing Cranelift forbidden field to fail" >&2
  exit 1
fi

grep -q "backend input must forbid Rust-derived required fields" "$temp_dir/missing-cranelift-forbidden.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/mir-backend.json" "$temp_dir/rust-capture.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["packages"][0]["apis"].append("mir.rs::lower_package")

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/rust-capture.json" >"$temp_dir/rust-capture.out" 2>"$temp_dir/rust-capture.err"; then
  echo "expected Rust-captured package API to fail" >&2
  exit 1
fi

grep -q "Rust capture term" "$temp_dir/rust-capture.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/mir-backend.json" "$temp_dir/cranelift-capture.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

for package in payload["packages"]:
    if package["name"] == "compiler.backend.native":
        package["apis"].append("compiler.backend.native.cranelift_backend")

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/cranelift-capture.json" >"$temp_dir/cranelift-capture.out" 2>"$temp_dir/cranelift-capture.err"; then
  echo "expected Cranelift-captured package API to fail" >&2
  exit 1
fi

grep -q "Rust capture term 'cranelift'" "$temp_dir/cranelift-capture.err"

echo "check-mir-backend-boundary regression cases passed"
