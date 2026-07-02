#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-stage1-full-lib-triage.py"
manifest="$repo_root/docs/rust-exit-full-lib-triage.json"
temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/axiom-full-lib-triage-test.XXXXXX")"
trap 'rm -rf "$temp_dir"' EXIT

python3 "$script" --manifest "$manifest" --json >"$temp_dir/report.json"
python3 - "$temp_dir/report.json" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
assert report["schema"] == "axiom.stage1.full_lib_triage.v1"
assert report["triaged"] is True
assert report["ready"] is False
assert report["summary"]["failure_count"] == 15
assert report["summary"]["categories"]["stale_generated_rust_expectation"] >= 1
assert report["summary"]["categories"]["direct_native_contract"] >= 1
assert report["summary"]["categories"]["environment_gated"] >= 1
PY

python3 - "$manifest" "$temp_dir/bad-count.json" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
payload["expectedFailureCount"] = payload["expectedFailureCount"] + 1
json.dump(payload, open(sys.argv[2], "w", encoding="utf-8"))
PY
if python3 "$script" --manifest "$temp_dir/bad-count.json" >/tmp/axiom-bad-count.out 2>&1; then
  echo "expected mismatched failure count to fail validation" >&2
  exit 1
fi

python3 - "$manifest" "$temp_dir/bad-env.json" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
for failure in payload["failures"]:
    if failure["category"] == "environment_gated":
        failure["resolution"] = "update_direct_native_contract"
        break
json.dump(payload, open(sys.argv[2], "w", encoding="utf-8"))
PY
if python3 "$script" --manifest "$temp_dir/bad-env.json" >/tmp/axiom-bad-env.out 2>&1; then
  echo "expected environment-gated resolution mismatch to fail validation" >&2
  exit 1
fi

python3 - "$manifest" "$temp_dir/duplicate.json" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
payload["failures"][1]["name"] = payload["failures"][0]["name"]
json.dump(payload, open(sys.argv[2], "w", encoding="utf-8"))
PY
if python3 "$script" --manifest "$temp_dir/duplicate.json" >/tmp/axiom-duplicate.out 2>&1; then
  echo "expected duplicate failure rows to fail validation" >&2
  exit 1
fi

echo "stage1 full lib triage regression cases passed"
