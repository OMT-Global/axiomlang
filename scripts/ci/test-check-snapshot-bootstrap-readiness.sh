#!/usr/bin/env bash
set -euo pipefail

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

manifest="$tmpdir/readiness.json"
snapshots="$tmpdir/snapshots.json"

cat > "$manifest" <<JSON
{
  "schemaVersion": 1,
  "schema": "axiom.self_hosting.snapshot_bootstrap_readiness.v0",
  "snapshotManifest": "$snapshots",
  "rows": [
    {
      "id": "manifest_valid",
      "requirement": "manifest valid",
      "status": "blocked",
      "governingIssue": 1253,
      "blockerIssues": [1253],
      "validatingCommand": "make snapshot-bootstrap-readiness"
    }
  ]
}
JSON

cat > "$snapshots" <<'JSON'
{
  "schema_version": "axiom.selfhost.snapshot_manifest.v0",
  "snapshots": []
}
JSON

if python3 scripts/ci/check-snapshot-bootstrap-readiness.py --json --manifest "$manifest" > "$tmpdir/blocked.json"; then
  echo "expected readiness check to fail without a pinned snapshot" >&2
  exit 1
fi

python3 - "$tmpdir/blocked.json" <<'PY'
import json, sys
payload = json.load(open(sys.argv[1], encoding="utf-8"))
assert payload["schema"] == "axiom.self_hosting.snapshot_bootstrap_readiness.v0"
assert payload["ready"] is False
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["snapshot_manifest_schema"] == "pass"
assert statuses["snapshot_available"] == "fail"
PY

python3 - "$manifest" "$snapshots" <<'PY'
import json, sys
manifest_path, snapshots_path = sys.argv[1:]
with open(manifest_path, encoding="utf-8") as handle:
    manifest = json.load(handle)
for row in manifest["rows"]:
    row["status"] = "implemented"
with open(manifest_path, "w", encoding="utf-8") as handle:
    json.dump(manifest, handle)
snapshots = {
    "schema_version": "axiom.selfhost.snapshot_manifest.v0",
    "snapshots": [{
        "version": "0.0.0-test",
        "target": "x86_64-unknown-linux-gnu",
        "sha256": "0" * 64,
        "source": "https://example.invalid/axiomc",
        "built_by": "cargo",
        "provenance": "https://example.invalid/provenance.json"
    }]
}
with open(snapshots_path, "w", encoding="utf-8") as handle:
    json.dump(snapshots, handle)
PY

python3 scripts/ci/check-snapshot-bootstrap-readiness.py --json --manifest "$manifest" > "$tmpdir/ready.json"

python3 - "$tmpdir/ready.json" <<'PY'
import json, sys
payload = json.load(open(sys.argv[1], encoding="utf-8"))
assert payload["ready"] is True
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["snapshot_available"] == "pass"
PY

echo "check-snapshot-bootstrap-readiness regression cases passed"
