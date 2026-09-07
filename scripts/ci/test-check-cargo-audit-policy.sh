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

python3 - "$checker" "$tmp_dir" <<'PY'
import json
import pathlib
import subprocess
import sys

checker, directory = sys.argv[1:]
root = pathlib.Path(directory)
package = {"name": "yanked-crate", "version": "1.2.3", "source": "registry+https://github.com/rust-lang/crates.io-index"}

def check(report, policy="empty-policy.json"):
    report_path = root / "warning.json"
    report_path.write_text(json.dumps(report))
    result = subprocess.run(
        [sys.executable, checker, "--report", str(report_path), "--policy", str(root / policy),
         "--today", "2026-08-10"],
        check=False, capture_output=True, text=True,
    )
    return result.returncode, json.loads(result.stdout)

# Match cargo-audit's package-only yanked warning, without an advisory ID.
for kind in ("yanked", "unmaintained", "unsound", "future-warning"):
    report = {"vulnerabilities": {"list": []}, "warnings": {kind: [{"package": package}]}}
    code, output = check(report)
    assert code == 1 and output["status"] == "fail", output
    assert output["active_advisories"] == [], output
    assert output["active_non_advisory_findings"] == [{"kind": f"warning:{kind}", "package": package}], output
    assert any("cannot be excepted" in error for error in output["errors"]), output

# A valid exception for another active advisory cannot hide a package-only warning.
report = json.loads((root / "finding.json").read_text())
report["warnings"]["yanked"] = [{"package": package}]
code, output = check(report, "valid-exception.json")
assert code == 1 and len(output["active_advisories"]) == 1, output
assert output["exceptions"] == ["RUSTSEC-2099-0001"], output
assert output["active_non_advisory_findings"] == [{"kind": "warning:yanked", "package": package}], output

# Advisory-backed warnings retain the existing issue-linked exception path.
report = {"warnings": {"unmaintained": [{"advisory": {"id": "RUSTSEC-2099-0001"}, "package": package}]}}
code, output = check(report, "valid-exception.json")
assert code == 0 and output["status"] == "pass", output
assert output["active_non_advisory_findings"] == [], output

# Missing package metadata does not turn a finding without an ID into a pass.
for report, kind in (({"warnings": {"yanked": [{}]}}, "warning:yanked"),
                     ({"vulnerabilities": {"list": [{}]}}, "vulnerability")):
    code, output = check(report)
    assert code == 1, output
    assert output["active_non_advisory_findings"] == [{"kind": kind, "package": None}], output
PY

echo "cargo-audit policy validation passed"
