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

report="$(python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$temp_dir" --threshold 2)"
python3 - "$report" <<'PY'
import json
import sys

report = json.loads(sys.argv[1])
assert report["blocking"] is False
assert report["summary"]["functions_scanned"] == 5
assert report["summary"]["hotspots_over_threshold"] >= 1
hotspot_names = {hotspot["function"] for hotspot in report["hotspots"]}
assert {"run", "limit", "reset"}.issubset(hotspot_names)
assert report["hotspots"][0]["function"] == "hotspot"
PY

if python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$temp_dir" --threshold 2 --enforce >/dev/null; then
  echo "--enforce must fail when hotspots exceed the threshold" >&2
  exit 1
fi

echo "stage1 CRAP threshold proposal test passed"
