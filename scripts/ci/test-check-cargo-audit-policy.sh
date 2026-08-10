#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
checker="$repo_root/scripts/ci/check-cargo-audit-policy.py"
[[ -x "$checker" ]] || { echo "missing executable checker: $checker" >&2; exit 1; }

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/axiom-cargo-audit-policy.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT

python3 - "$tmp_dir" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
clean = {
    "vulnerabilities": {"found": False, "list": []},
    "warnings": {"unmaintained": [], "unsound": [], "yanked": []},
}
finding = {
    "vulnerabilities": {
        "found": True,
        "list": [{"advisory": {"id": "RUSTSEC-2099-0001"}}],
    },
    "warnings": {"unmaintained": [], "unsound": [], "yanked": []},
}
for name, value in {"clean.json": clean, "finding.json": finding}.items():
    (root / name).write_text(json.dumps(value))

(root / "empty-policy.json").write_text(json.dumps({"version": 1, "exceptions": []}))
(root / "valid-exception.json").write_text(json.dumps({
    "version": 1,
    "exceptions": [{
        "advisory": "RUSTSEC-2099-0001",
        "issue": "https://github.com/OMT-Global/axiomlang/issues/1564",
        "expires_at": "2099-01-01",
        "reason": "Upgrade is scheduled after the next compatibility test cycle.",
    }],
}))
(root / "expired-exception.json").write_text(json.dumps({
    "version": 1,
    "exceptions": [{
        "advisory": "RUSTSEC-2099-0001",
        "issue": "https://github.com/OMT-Global/axiomlang/issues/1564",
        "expires_at": "2020-01-01",
        "reason": "Temporary exception while the upgrade is prepared.",
    }],
}))
PY

python3 "$checker" --report "$tmp_dir/clean.json" --policy "$tmp_dir/empty-policy.json" >/dev/null
if python3 "$checker" --report "$tmp_dir/finding.json" --policy "$tmp_dir/empty-policy.json" >/dev/null; then
  echo "unexcepted advisories must fail" >&2
  exit 1
fi
python3 "$checker" --report "$tmp_dir/finding.json" --policy "$tmp_dir/valid-exception.json" --today 2026-08-10 >/dev/null
if python3 "$checker" --report "$tmp_dir/finding.json" --policy "$tmp_dir/expired-exception.json" --today 2026-08-10 >/dev/null; then
  echo "expired exceptions must fail" >&2
  exit 1
fi
if python3 "$checker" --report "$tmp_dir/clean.json" --policy "$tmp_dir/valid-exception.json" >/dev/null; then
  echo "orphaned exceptions must fail" >&2
  exit 1
fi

echo "cargo-audit policy validation passed"
