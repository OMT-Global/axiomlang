#!/usr/bin/env bash
set -euo pipefail

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

manifest="$tmpdir/readiness.json"
doc="$tmpdir/readiness.md"
issues_open="$tmpdir/issues-open.txt"
issues_closed="$tmpdir/issues-closed.txt"

mkdir -p "$tmpdir/evidence"
printf 'fixture\n' > "$tmpdir/evidence/implemented.ax"
printf '# readiness\n' > "$doc"

cat > "$manifest" <<JSON
{
  "schemaVersion": 1,
  "schema": "axiom.self_hosting.language_readiness.v0",
  "rows": [
    {
      "id": "implemented_row",
      "group": "test",
      "requirement": "Implemented row has evidence and a command.",
      "governingIssue": 1256,
      "status": "implemented",
      "directNativeStatus": "implemented",
      "evidence": ["$tmpdir/evidence/implemented.ax"],
      "validatingCommand": "echo ok"
    },
    {
      "id": "blocked_row",
      "group": "test",
      "requirement": "Blocked row keeps readiness false.",
      "governingIssue": 721,
      "status": "blocked",
      "directNativeStatus": "partial",
      "blockerIssues": [721],
      "evidence": ["$tmpdir/evidence/implemented.ax"],
      "validatingCommand": "make rust-exit-readiness"
    }
  ]
}
JSON

cat > "$issues_open" <<'STATES'
721 OPEN
STATES

cat > "$issues_closed" <<'STATES'
721 CLOSED
STATES

if python3 scripts/ci/check-self-hosting-language-readiness.py \
  --json \
  --manifest "$manifest" \
  --doc "$doc" \
  --issue-state-file "$issues_open" > "$tmpdir/blocked.json"; then
  echo "expected readiness check to fail while a row is blocked" >&2
  exit 1
fi

python3 - "$tmpdir/blocked.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["schema"] == "axiom.self_hosting.language_readiness.v0"
assert payload["ready"] is False
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["language_readiness_rows_implemented"] == "fail"
assert statuses["language_readiness_issue_721_closed"] == "fail"
PY

python3 - "$manifest" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)

for row in payload["rows"]:
    row["status"] = "implemented"
    row["directNativeStatus"] = "implemented"
    row.pop("blockerIssues", None)

with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

python3 scripts/ci/check-self-hosting-language-readiness.py \
  --json \
  --manifest "$manifest" \
  --doc "$doc" \
  --issue-state-file "$issues_closed" > "$tmpdir/ready.json"

python3 - "$tmpdir/ready.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["ready"] is True
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["language_readiness_rows_implemented"] == "pass"
PY

if python3 scripts/ci/check-self-hosting-language-readiness.py \
  --json \
  --manifest "$manifest" \
  --doc "$doc" \
  --issue-state-file "$tmpdir/missing-issues.txt" > "$tmpdir/missing-issues.json"; then
  echo "expected readiness check to fail when issue state file is missing" >&2
  exit 1
fi

python3 - "$tmpdir/missing-issues.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["language_readiness_issue_state_source"] == "fail"
PY

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

if python3 scripts/ci/check-self-hosting-language-readiness.py \
  --json \
  --manifest "$manifest" \
  --doc "$doc" > "$tmpdir/missing-evidence.json"; then
  echo "expected readiness check to fail when row evidence is missing" >&2
  exit 1
fi

grep -Fq "missing evidence: missing.ax" "$tmpdir/missing-evidence.json"

echo "check-self-hosting-language-readiness regression cases passed"
