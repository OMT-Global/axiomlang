#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-rust-exit-readiness.sh"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

case_dir="$temp_dir/repo"
mkdir -p "$case_dir/docs" "$case_dir/scripts/ci"
cp "$script" "$case_dir/scripts/ci/check-rust-exit-readiness.sh"
cp "$repo_root/docs/rust-exit-readiness.md" "$case_dir/docs/rust-exit-readiness.md"
cp "$repo_root/docs/rust-exit-readiness.json" "$case_dir/docs/rust-exit-readiness.json"
if ! grep -q '`blocked`' "$case_dir/docs/rust-exit-readiness.md"; then
  echo "expected copied readiness doc to preserve descriptive blocked rows" >&2
  exit 1
fi
cat >"$case_dir/Makefile" <<'MAKE'
rust-exit-readiness:
	bash scripts/ci/check-rust-exit-readiness.sh --json

rust-exit-readiness-github:
	bash scripts/ci/check-rust-exit-readiness.sh --json --require-issue-states

rust-exit-readiness-test:
	bash scripts/ci/test-check-rust-exit-readiness.sh
MAKE

(
  cd "$case_dir"
  cat >"$temp_dir/blocked-issues.txt" <<'ISSUES'
927 CLOSED
928 OPEN
929 CLOSED
693 CLOSED
694 CLOSED
930 CLOSED
931 CLOSED
562 CLOSED
563 CLOSED
564 CLOSED
ISSUES
  if bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/blocked-issues.txt" >"$temp_dir/blocked.json" 2>"$temp_dir/blocked.err"; then
    echo "expected readiness check to fail while a manifest blocker issue is open" >&2
    exit 1
  fi
  python3 - "$temp_dir/blocked.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["schema"] == "axiom.rust_exit.readiness.v1"
assert payload["ready"] is False
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["readiness_doc_present"] == "pass"
assert statuses["readiness_manifest_valid"] == "pass"
assert statuses["rust_exit_issue_927_closed"] == "pass"
assert statuses["rust_exit_issue_928_closed"] == "fail"
PY
)

cat >"$temp_dir/issues.txt" <<'ISSUES'
927 CLOSED
928 CLOSED
929 CLOSED
693 CLOSED
694 CLOSED
930 CLOSED
931 CLOSED
562 CLOSED
563 CLOSED
564 CLOSED
ISSUES

(
  cd "$case_dir"
  bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/issues.txt" >"$temp_dir/ready.json"
  python3 - "$temp_dir/ready.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["ready"] is True
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert "readiness_matrix_unblocked" not in statuses
assert statuses["rust_exit_issue_927_closed"] == "pass"
assert statuses["rust_exit_issue_564_closed"] == "pass"
PY
)

echo "check-rust-exit-readiness regression cases passed"
