#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-diagnostics-syntax-boundary.py"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

python3 "$script" --json >"$temp_dir/result.json"

python3 - "$temp_dir/result.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["schema"] == "axiom.compiler.diagnostics_syntax.v1"
assert payload["ok"] is True
assert payload["fixtures"] == 5
assert payload["stable_parse_codes"] >= 4
PY

python3 - "$repo_root/stage1/compiler-contracts/snapshots/diagnostics-syntax.json" "$temp_dir/unexpected-field.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["diagnostics"]["unexpected"] = "drift"

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/unexpected-field.json" >"$temp_dir/unexpected.out" 2>"$temp_dir/unexpected.err"; then
  echo "expected schema-invalid diagnostics/syntax snapshot to fail" >&2
  exit 1
fi

grep -q "unexpected fields" "$temp_dir/unexpected.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/diagnostics-syntax.json" "$temp_dir/missing-parse-code.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["diagnostics"]["stable_parse_codes"].remove("parse.missing_token")
payload["diagnostics"]["stable_parse_codes"].append("parse.future_placeholder")

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/missing-parse-code.json" >"$temp_dir/code.out" 2>"$temp_dir/code.err"; then
  echo "expected missing stable parse code to fail" >&2
  exit 1
fi

grep -q "stable parse code list" "$temp_dir/code.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/diagnostics-syntax.json" "$temp_dir/non-parse-code.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["diagnostics"]["stable_parse_codes"].append("ownership.use_after_move")

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/non-parse-code.json" >"$temp_dir/nonparse.out" 2>"$temp_dir/nonparse.err"; then
  echo "expected non-parse stable code to fail" >&2
  exit 1
fi

grep -q "parse.* namespace" "$temp_dir/nonparse.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/diagnostics-syntax.json" "$temp_dir/rust-capture.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["syntax"]["public_terms"].append("Rust enum")

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/rust-capture.json" >"$temp_dir/rust.out" 2>"$temp_dir/rust.err"; then
  echo "expected Rust-captured public term to fail" >&2
  exit 1
fi

grep -q "captures Rust term" "$temp_dir/rust.err"

python3 - "$repo_root" "$temp_dir" <<'PY'
import copy
import json
import subprocess
import sys
from pathlib import Path

root = Path(sys.argv[1])
temporary = Path(sys.argv[2])
script = root / "scripts/ci/check-diagnostics-syntax-boundary.py"
snapshot = json.loads((root / "stage1/compiler-contracts/snapshots/diagnostics-syntax.json").read_text())
fixture = json.loads((root / snapshot["fixtures"]["check_success"]["path"]).read_text())
accepted_invalid = []

for name, value in [("true", True), ("false", False), ("null", None),
                    ("fraction", 1.5), ("string", "1"), ("missing", None)]:
    payload = copy.deepcopy(fixture)
    if name == "missing":
        payload.pop("statement_count", None)
    else:
        payload["statement_count"] = value
    fixture_path = temporary / f"count-{name}.json"
    fixture_path.write_text(json.dumps(payload))
    candidate = copy.deepcopy(snapshot)
    candidate["fixtures"]["check_success"]["path"] = str(fixture_path)
    snapshot_path = temporary / f"snapshot-{name}.json"
    snapshot_path.write_text(json.dumps(candidate))
    result = subprocess.run([sys.executable, str(script), "--snapshot", str(snapshot_path)],
                            cwd=root, capture_output=True, text=True, timeout=10)
    if result.returncode == 0:
        accepted_invalid.append(name)
    else:
        assert result.returncode == 1, (name, result.stderr)
        assert "statement_count" in result.stderr, (name, result.stderr)

assert not accepted_invalid, f"accepted invalid statement counts: {accepted_invalid}"

for count in (0, 1, 37):
    payload = copy.deepcopy(fixture)
    payload["statement_count"] = count
    fixture_path = temporary / f"count-{count}.json"
    fixture_path.write_text(json.dumps(payload))
    candidate = copy.deepcopy(snapshot)
    candidate["fixtures"]["check_success"]["path"] = str(fixture_path)
    snapshot_path = temporary / f"snapshot-{count}.json"
    snapshot_path.write_text(json.dumps(candidate))
    result = subprocess.run([sys.executable, str(script), "--snapshot", str(snapshot_path), "--json"],
                            cwd=root, capture_output=True, text=True, timeout=10)
    assert result.returncode == 0, (count, result.stderr)
    assert json.loads(result.stdout)["ok"] is True

print("statement_count CLI controls: six invalid rejected, three integers preserved")
PY

echo "check-diagnostics-syntax-boundary regression cases passed"
