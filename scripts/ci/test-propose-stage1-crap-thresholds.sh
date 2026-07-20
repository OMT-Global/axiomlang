#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

cat >"$temp_dir/sample.rs" <<'RS'
fn simple() {
    println!("ok");
}

fn hotspot(value: i32) -> i32 {
    if value > 10 {
        return value;
    }
    if value > 5 {
        return value + 1;
    }
    match value {
        1 => 1,
        2 => 2,
        _ => 0,
    }
}

impl Worker {
    pub async fn run(&self) {
        if self.ready {
            return;
        }
    }

    pub const fn limit() -> usize {
        1
    }

    unsafe fn reset(&mut self) {
        self.ready = false;
    }
}
RS

cat >"$temp_dir/sample.lcov" <<LCOV
TN:
SF:$temp_dir/sample.rs
DA:1,1
DA:2,1
DA:5,1
DA:6,1
DA:7,0
DA:8,0
DA:9,0
DA:10,0
DA:11,0
DA:12,0
DA:13,0
DA:14,0
DA:18,1
DA:19,1
DA:20,1
DA:24,1
DA:25,1
DA:28,1
DA:29,1
end_of_record
LCOV

report="$(python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$temp_dir" --lcov "$temp_dir/sample.lcov" --threshold 2)"
python3 - "$report" <<'PY'
import json
import sys

report = json.loads(sys.argv[1])
assert report["blocking"] is False
assert report["summary"]["functions_scanned"] == 5
assert report["summary"]["functions_with_coverage"] == 5
assert report["inputs"]["coverage"]["source"] == "lcov"
assert report["summary"]["hotspots_over_threshold"] >= 1
hotspot_names = {hotspot["function"] for hotspot in report["hotspots"]}
assert {"run", "limit", "reset"}.issubset(hotspot_names)
assert report["hotspots"][0]["function"] == "hotspot"
PY

if python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$temp_dir" --lcov "$temp_dir/sample.lcov" --threshold 2 --enforce >/dev/null; then
  echo "--enforce must fail when hotspots exceed the threshold" >&2
  exit 1
fi

if python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$temp_dir" --threshold 2 --enforce >/dev/null 2>"$temp_dir/enforce.err"; then
  echo "--enforce without LCOV must fail" >&2
  exit 1
fi
grep -q -- "--enforce requires --lcov" "$temp_dir/enforce.err"

unmeasured="$(python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$temp_dir" --threshold 2)"
python3 - "$unmeasured" <<'PY'
import json
import sys

report = json.loads(sys.argv[1])
assert report["inputs"]["coverage"]["source"] == "unmeasured"
assert report["summary"]["functions_without_coverage"] == 5
assert report["summary"]["hotspots_over_threshold"] == 0
PY

if python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$temp_dir/missing" >/dev/null 2>"$temp_dir/missing.err"; then
  echo "missing --source-root must fail" >&2
  exit 1
fi
grep -q "source root does not exist" "$temp_dir/missing.err"

empty_dir="$temp_dir/empty"
mkdir "$empty_dir"
if python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$empty_dir" >/dev/null 2>"$temp_dir/empty.err"; then
  echo "empty --source-root must fail" >&2
  exit 1
fi
grep -q "no Rust functions discovered" "$temp_dir/empty.err"

echo "stage1 CRAP threshold proposal test passed"
