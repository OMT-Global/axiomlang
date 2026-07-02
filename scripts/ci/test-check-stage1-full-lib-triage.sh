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
assert report["ready"] is True
assert report["summary"]["failure_count"] == 0
assert report["summary"]["categories"] == {}
assert report["summary"]["resolutions"] == {}
PY

python3 - "$manifest" "$temp_dir/env-only.json" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
payload["status"] = "blocked"
payload["failures"] = [{
    "name": "tests::synthetic_environment_gated_case",
    "category": "environment_gated",
    "resolution": "environment_gate",
    "blockerIssue": 1255,
    "rationale": "Synthetic row proves environment-gated manifests stay valid while failures remain separated.",
    "resolutionStatus": "open",
}]
payload["expectedFailureCount"] = 1
payload["resolutionTracking"]["statuses"]["open"] = 1
json.dump(payload, open(sys.argv[2], "w", encoding="utf-8"))
PY
python3 "$script" --manifest "$temp_dir/env-only.json" --json >"$temp_dir/env-only-report.json"
python3 - "$temp_dir/env-only-report.json" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
assert report["triaged"] is True
assert report["summary"]["failure_count"] == 1
assert report["summary"]["categories"] == {"environment_gated": 1}
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
payload["status"] = "blocked"
payload["expectedFailureCount"] = 1
payload["failures"] = [{
    "name": "tests::synthetic_environment_gated_case",
    "category": "environment_gated",
    "resolution": "update_direct_native_contract",
    "blockerIssue": 1255,
    "rationale": "Synthetic row intentionally mismatches an environment-gated resolution.",
    "resolutionStatus": "open",
}]
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
payload["status"] = "blocked"
failure = {
    "name": "tests::synthetic_duplicate_case",
    "category": "direct_native_contract",
    "resolution": "update_direct_native_contract",
    "blockerIssue": 1255,
    "rationale": "Synthetic row proves duplicate names are rejected.",
    "resolutionStatus": "open",
}
payload["failures"] = [failure, dict(failure)]
payload["expectedFailureCount"] = 2
json.dump(payload, open(sys.argv[2], "w", encoding="utf-8"))
PY
if python3 "$script" --manifest "$temp_dir/duplicate.json" >/tmp/axiom-duplicate.out 2>&1; then
  echo "expected duplicate failure rows to fail validation" >&2
  exit 1
fi

echo "stage1 full lib triage regression cases passed"
