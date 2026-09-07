#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

checker="scripts/ci/check-capability-ledger.py"
snapshot="stage1/compiler-contracts/snapshots/capability-ledger.json"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

python3 "$checker" --check-docs --json >"$tmpdir/current.json"
python3 - "$tmpdir/current.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
assert payload["schema"] == "axiom.capability_ledger.v1"
assert payload["ok"] is True
assert payload["summary"]["stdlibModules"] == 34
assert payload["summary"]["capabilities"] == 9
assert payload["summary"]["evidenceTiers"]["production_qualified"] == 0
PY

python3 - "$snapshot" "$tmpdir/duplicate.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
payload["commands"].append(dict(payload["commands"][0]))
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --snapshot "$tmpdir/duplicate.json" --json >"$tmpdir/duplicate-report.json"; then
  echo "expected duplicate ledger row to fail" >&2
  exit 1
fi
grep -Fq "duplicate ledger row commands" "$tmpdir/duplicate-report.json"

python3 - "$snapshot" "$tmpdir/invalid-tier.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
payload["commands"][0]["evidenceTier"] = "maybe"
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --snapshot "$tmpdir/invalid-tier.json" --json >"$tmpdir/invalid-tier-report.json"; then
  echo "expected invalid evidence tier to fail" >&2
  exit 1
fi
grep -Fq "invalid evidence tier" "$tmpdir/invalid-tier-report.json"

python3 - "$snapshot" "$tmpdir/stale.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
payload["summary"]["stdlibModules"] -= 1
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
if python3 "$checker" --snapshot "$tmpdir/stale.json" --json >"$tmpdir/stale-report.json"; then
  echo "expected stale generated snapshot to fail" >&2
  exit 1
fi
grep -Fq "checked capability ledger is stale" "$tmpdir/stale-report.json"

python3 - "stage1/crates/axiomc/src/main.rs" "$tmpdir/main.rs" <<'PY'
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    source = handle.read()
source = source.replace("enum Command {", "enum Command {\n    FutureProbe,", 1)
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    handle.write(source)
PY
if python3 "$checker" --main-source "$tmpdir/main.rs" --json >"$tmpdir/unclassified-report.json"; then
  echo "expected unclassified compiler command to fail" >&2
  exit 1
fi
grep -Fq "command classification drift" "$tmpdir/unclassified-report.json"
grep -Fq "future-probe" "$tmpdir/unclassified-report.json"

cp README.md "$tmpdir/stale-doc.md"
printf '\nOnly `std/fs.ax read_file` is supported.\n' >>"$tmpdir/stale-doc.md"
if python3 "$checker" --check-docs --docs "$tmpdir/stale-doc.md" --json >"$tmpdir/doc-report.json"; then
  echo "expected stale checked documentation to fail" >&2
  exit 1
fi
grep -Fq "filesystem writes are implemented" "$tmpdir/doc-report.json"

cp README.md "$tmpdir/stale-issue-doc.md"
printf '\nIssue #216 is current production closure evidence.\n' >>"$tmpdir/stale-issue-doc.md"
if python3 "$checker" --check-docs --docs "$tmpdir/stale-issue-doc.md" --json >"$tmpdir/stale-issue-report.json"; then
  echo "expected stale historical issue-state claim to fail" >&2
  exit 1
fi
grep -Fq "historical evidence" "$tmpdir/stale-issue-report.json"

python3 - "$checker" "$snapshot" "$tmpdir" <<'PY'
import importlib.util
import json
from pathlib import Path
import sys

spec = importlib.util.spec_from_file_location("capability_ledger", sys.argv[1])
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)
ledger = json.loads(Path(sys.argv[2]).read_text())
root = Path(sys.argv[3])
marker = checker.expected_doc_marker(ledger)
modules = ledger["summary"]["stdlibModules"]
functions = ledger["summary"]["stdlibFunctions"]
for text, valid in [
    (f"{modules} synthetic standard-library\nmodules with {functions} exported\nfunctions.", True),
    (f"{modules - 1} stdlib modules with {functions} exported functions.", False),
    (f"{modules} stdlib modules with {functions - 1} exported\nfunctions.", False),
]:
    (root / "inventory.md").write_text(marker + "\n" + text)
    errors = checker.validate_docs(root, ["inventory.md"], ledger)
    assert bool(errors) is not valid, (text, errors)
    if not valid:
        assert any("prose count" in error for error in errors), errors
PY

echo "capability ledger regression cases passed"
