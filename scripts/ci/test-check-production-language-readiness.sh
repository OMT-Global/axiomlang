#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

manifest="$tmpdir/readiness.json"
doc="$tmpdir/readiness.md"
evidence="$tmpdir/evidence.ax"
issues_open="$tmpdir/issues-open.txt"
issues_closed="$tmpdir/issues-closed.txt"
issues_governing_open="$tmpdir/issues-governing-open.txt"
checker="scripts/ci/check-production-language-readiness.py"
schema="stage1/schemas/axiom-production-language-readiness-v1.schema.json"

printf '# Production readiness fixture\n' > "$doc"
printf 'fn main() {}\n' > "$evidence"
printf '1124 CLOSED\n1125 CLOSED\n1434 OPEN\n' > "$issues_open"
printf '1124 CLOSED\n1125 CLOSED\n1434 CLOSED\n' > "$issues_closed"
printf '1124 OPEN\n1125 CLOSED\n1434 CLOSED\n' > "$issues_governing_open"

write_blocked_manifest() {
  cat > "$manifest" <<JSON
{
  "schemaVersion": 1,
  "schema": "axiom.production_language.readiness.v1",
  "umbrellaIssue": 1432,
  "rows": [
    {
      "id": "implemented_row",
      "track": "test",
      "requirement": "Runtime evidence meets the target tier.",
      "requiredForProduction": true,
      "targetTier": "runtime_complete",
      "currentTier": "runtime_complete",
      "status": "implemented",
      "governingIssue": 1124,
      "dependencies": [],
      "evidence": ["$evidence"],
      "validatingCommand": "echo ok",
      "rustCaptureRisk": "low",
      "agentInspectionImpact": "Expose the proof."
    },
    {
      "id": "blocked_row",
      "track": "test",
      "requirement": "A static spike cannot satisfy runtime readiness.",
      "requiredForProduction": true,
      "targetTier": "runtime_complete",
      "currentTier": "static_spike",
      "status": "blocked",
      "governingIssue": 1125,
      "blockerIssues": [1434],
      "dependencies": [1434],
      "evidence": ["$evidence"],
      "validatingCommand": "echo blocked",
      "rustCaptureRisk": "high",
      "agentInspectionImpact": "Expose the blocked tier."
    }
  ]
}
JSON
}

write_blocked_manifest
if python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" --issue-state-file "$issues_open" > "$tmpdir/blocked.json"; then
  echo "expected readiness to fail while a required row is blocked" >&2
  exit 1
fi

python3 - "$tmpdir/blocked.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["schema"] == "axiom.production_language.readiness.v1"
assert payload["valid"] is True
assert payload["ready"] is False
assert payload["summary"]["required_ready"] == 1
statuses = {item["name"]: item["status"] for item in payload["checks"]}
assert statuses["production_readiness_row_blocked_row"] == "pass"
assert statuses["production_readiness_required_rows"] == "fail"
assert statuses["production_readiness_issue_1434_closed"] == "fail"
PY

python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
row = payload["rows"][1]
row["currentTier"] = "runtime_complete"
row["status"] = "implemented"
row.pop("blockerIssues")
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" --issue-state-file "$issues_open" > "$tmpdir/open-dependency.json"; then
  echo "expected readiness to fail while an implemented row dependency is open" >&2
  exit 1
fi
grep -Fq 'issue #1434 is OPEN' "$tmpdir/open-dependency.json"

if python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" --issue-state-file "$issues_governing_open" > "$tmpdir/open-governing.json"; then
  echo "expected readiness to fail while an implemented row governing issue is open" >&2
  exit 1
fi
grep -Fq 'issue #1124 is OPEN' "$tmpdir/open-governing.json"

python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" --issue-state-file "$issues_closed" > "$tmpdir/ready.json"

python3 - "$tmpdir/ready.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["ready"] is True
assert payload["summary"]["required_ready"] == 2
PY

write_blocked_manifest
python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["rows"][1]["requiredForProduction"] = False
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" --issue-state-file "$issues_open" > "$tmpdir/optional-blocked.json"
python3 - "$tmpdir/optional-blocked.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
assert payload["ready"] is True
assert not any(
    item["name"] == "production_readiness_issue_1434_closed"
    for item in payload["checks"]
)
PY

if python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" --issue-state-file "$tmpdir/missing.txt" > "$tmpdir/missing-issues.json"; then
  echo "expected readiness to fail when an explicit issue state file is missing" >&2
  exit 1
fi
grep -Fq 'issue state file does not exist' "$tmpdir/missing-issues.json"

write_blocked_manifest
python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["rows"][0]["currentTier"] = "runtimeish"
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" > "$tmpdir/invalid-tier.json"; then
  echo "expected readiness to reject an invalid evidence tier" >&2
  exit 1
fi
grep -Fq "invalid currentTier 'runtimeish'" "$tmpdir/invalid-tier.json"

write_blocked_manifest
python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["schemaVersion"] = True
for row in payload["rows"]:
    row["status"] = "implemented"
    row["currentTier"] = row["targetTier"]
    row.pop("blockerIssues", None)
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --json --validate-only --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" > "$tmpdir/boolean-schema-version.json"; then
  echo "expected readiness to reject boolean schemaVersion" >&2
  exit 1
fi
grep -Fq 'schemaVersion must be 1' "$tmpdir/boolean-schema-version.json"

write_blocked_manifest
python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["rows"][0]["currentTier"] = []
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" > "$tmpdir/malformed-tier.json"; then
  echo "expected readiness to reject a malformed evidence tier without crashing" >&2
  exit 1
fi
python3 - "$tmpdir/malformed-tier.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
assert payload["valid"] is False
assert any(item["name"] == "production_readiness_json_schema" for item in payload["checks"])
PY

write_blocked_manifest
python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["rows"][0]["dependencies"] = None
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --json --validate-only --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" > "$tmpdir/null-dependencies.json"; then
  echo "expected readiness to reject null dependencies without crashing" >&2
  exit 1
fi
grep -Fq 'dependencies must be an array' "$tmpdir/null-dependencies.json"

write_blocked_manifest
python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["rows"][0]["status"] = {}
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --json --validate-only --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" > "$tmpdir/object-status.json"; then
  echo "expected readiness to reject object status without crashing" >&2
  exit 1
fi
python3 - "$tmpdir/object-status.json" <<'PY'
import json
import sys
json.load(open(sys.argv[1], encoding="utf-8"))
PY

write_blocked_manifest
python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["rows"][0]["id"] = []
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --json --validate-only --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" > "$tmpdir/array-id.json"; then
  echo "expected readiness to reject array id without crashing" >&2
  exit 1
fi
python3 - "$tmpdir/array-id.json" <<'PY'
import json
import sys
json.load(open(sys.argv[1], encoding="utf-8"))
PY

write_blocked_manifest
python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["rows"][0]["surprise"] = True
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" > "$tmpdir/unexpected-field.json"; then
  echo "expected readiness to reject fields outside the versioned schema" >&2
  exit 1
fi
grep -Fq 'unexpected fields: surprise' "$tmpdir/unexpected-field.json"

write_blocked_manifest
python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["rows"][0]["dependencies"] = [1125]
payload["rows"][1]["dependencies"] = [1124]
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" > "$tmpdir/dependency-cycle.json"; then
  echo "expected readiness to reject an issue dependency cycle" >&2
  exit 1
fi
grep -Fq 'dependency cycle:' "$tmpdir/dependency-cycle.json"

write_blocked_manifest
python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["rows"][0]["evidence"] = ["missing.ax"]
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --json --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" > "$tmpdir/missing-evidence.json"; then
  echo "expected readiness to reject missing evidence" >&2
  exit 1
fi
grep -Fq 'missing evidence: missing.ax' "$tmpdir/missing-evidence.json"

write_blocked_manifest
python3 "$checker" --json --validate-only --manifest "$manifest" --doc "$doc" \
  --schema-file "$schema" > "$tmpdir/valid-blocked.json"
python3 - "$tmpdir/valid-blocked.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
assert payload["valid"] is True
assert payload["ready"] is False
PY

echo "check-production-language-readiness regression cases passed"
