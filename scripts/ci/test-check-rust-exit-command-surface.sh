#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-rust-exit-command-surface.py"
snapshot="$repo_root/stage1/compiler-contracts/snapshots/command-lsp.json"
manifest="$repo_root/docs/rust-exit-readiness.json"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

python3 "$script" --json >"$temp_dir/report.json"

python3 - "$temp_dir/report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert report["schema"] == "axiom.rust_exit.command_surface_coverage.v0"
assert report["ready"] is True
assert report["summary"]["surface_count"] == 6
assert report["summary"]["implemented"] == 6
assert report["summary"]["blocked"] == 0
assert report["summary"]["blocked_surfaces"] == []
assert report["errors"] == []
rows = {row["surface"]: row for row in report["rows"]}
for surface in ("check", "build", "run", "test"):
    assert rows[surface]["status"] == "implemented"
    assert rows[surface]["fixtures"], surface
assert rows["doc"]["status"] == "implemented"
assert rows["doc"]["blockers"] == []
assert rows["doc"]["proof_issues"] == [731]
assert rows["lsp"]["status"] == "implemented"
assert rows["lsp"]["blockers"] == []
assert rows["lsp"]["proof_issues"] == [731]
PY

if ! python3 "$script" --enforce-ready >"$temp_dir/enforce.out" 2>"$temp_dir/enforce.err"; then
  echo "expected command surface readiness enforcement to pass with doc/lsp implemented" >&2
  exit 1
fi

python3 - "$snapshot" "$temp_dir/missing-doc.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["commands"] = [
    command for command in payload["commands"] if command.get("name") != "doc"
]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" \
  --command-lsp-snapshot "$temp_dir/missing-doc.json" \
  --json >"$temp_dir/missing-doc-report.json"; then
  echo "expected missing doc command surface to fail" >&2
  exit 1
fi

python3 - "$temp_dir/missing-doc-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert any("missing commands: doc" in error for error in report["errors"])
PY

python3 - "$manifest" "$temp_dir/missing-731.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["blockingIssues"] = [
    issue for issue in payload["blockingIssues"] if issue.get("issue") != 731
]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" \
  --readiness-manifest "$temp_dir/missing-731.json" \
  --json >"$temp_dir/missing-731-report.json"; then
  echo "expected missing doc/LSP ownership proof to fail" >&2
  exit 1
fi

python3 - "$temp_dir/missing-731-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert any("proof #731" in error for error in report["errors"])
PY

python3 - "$snapshot" "$temp_dir/cargo-release.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["official_release"]["requires_cargo"] = True

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" \
  --command-lsp-snapshot "$temp_dir/cargo-release.json" \
  --json >"$temp_dir/cargo-release-report.json"; then
  echo "expected Cargo-required official release to fail" >&2
  exit 1
fi

python3 - "$temp_dir/cargo-release-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)

assert any("must not require Cargo" in error for error in report["errors"])
PY

echo "rust-exit command surface coverage regression cases passed"
