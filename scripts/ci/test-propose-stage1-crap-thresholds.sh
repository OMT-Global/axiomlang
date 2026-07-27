#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temp_dir="$(mktemp -d)"
repo_temp_dir="$(mktemp -d "$repo_root/.crap-threshold-test.XXXXXX")"
trap 'rm -rf "$temp_dir" "$repo_temp_dir"' EXIT

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

fn unmeasured() {
    println!("no DA records");
}

fn zero_coverage() {
    println!("measured but not hit");
}

fn duplicate() {
    println!("first");
}

fn duplicate() {
    println!("second");
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
DA:15,0
DA:16,0
DA:17,0
DA:20,1
DA:21,1
DA:22,1
DA:26,1
DA:27,1
DA:30,1
DA:31,1
DA:39,0
DA:40,0
DA:43,0
DA:44,0
DA:47,0
DA:48,0
LF:28
LH:11
end_of_record
LCOV

report="$(python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$temp_dir" --lcov "$temp_dir/sample.lcov" --threshold 2)"
python3 - "$report" <<'PY'
import json
import sys

report = json.loads(sys.argv[1])
assert report["blocking"] is False
assert report["proposed_policy"]["blocking_enabled"] is False
assert report["proposed_policy"]["blocking_threshold"] is None
assert report["proposed_policy"]["status"] == "advisory proposal only"
assert report["summary"]["functions_scanned"] == 9
assert report["summary"]["functions_with_coverage"] == 8
assert report["summary"]["functions_without_coverage"] == 1
assert report["inputs"]["coverage"]["source"] == "lcov"
assert report["summary"]["hotspots_over_threshold"] >= 1
hotspot_names = {hotspot["function"] for hotspot in report["hotspots"]}
assert {"run", "limit", "reset"}.issubset(hotspot_names)
assert report["hotspots"][0]["function"] == "hotspot"
zero = next(hotspot for hotspot in report["hotspots"] if hotspot["function"] == "zero_coverage")
assert zero["coverage"] == 0.0
assert all(hotspot["function"] != "unmeasured" for hotspot in report["hotspots"])
duplicates = [hotspot for hotspot in report["hotspots"] if hotspot["function"] == "duplicate"]
assert len(duplicates) == 2
assert len({hotspot["identity"] for hotspot in duplicates}) == 2
assert {hotspot["identity"].rsplit("#", 1)[1] for hotspot in duplicates} == {"1", "2"}
PY

enforced_report="$temp_dir/enforced.json"
if python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$temp_dir" --lcov "$temp_dir/sample.lcov" --threshold 2 --enforce >"$enforced_report"; then
  echo "--enforce must fail when hotspots exceed the threshold" >&2
  exit 1
fi
python3 - "$enforced_report" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)
assert report["blocking"] is True
assert report["proposed_policy"]["blocking_enabled"] is True
assert report["proposed_policy"]["blocking_threshold"] == 2
assert report["proposed_policy"]["status"] == "legacy --enforce mode active"
PY

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
assert report["summary"]["functions_without_coverage"] == 9
assert report["summary"]["hotspots_over_threshold"] == 0
PY

cat >"$repo_temp_dir/relative.rs" <<'RS'
fn relative_path() {
    println!("relative");
}
RS
relative_source="${repo_temp_dir#"$repo_root"/}/relative.rs"
cat >"$repo_temp_dir/relative.lcov" <<LCOV
SF:$relative_source
DA:1,1
LF:1
LH:1
end_of_record
LCOV
relative_report="$(python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$repo_temp_dir" --lcov "$repo_temp_dir/relative.lcov")"
python3 - "$relative_report" <<'PY'
import json
import sys

report = json.loads(sys.argv[1])
assert not report["source_root"].startswith("/")
assert not report["inputs"]["coverage"]["path"].startswith("/")
assert not report["hotspots"][0]["path"].startswith("/")
assert "\\" not in report["hotspots"][0]["path"]
assert report["hotspots"][0]["identity"].startswith(report["hotspots"][0]["path"] + "::relative_path#1")
PY

expect_lcov_failure() {
  local name="$1"
  local pattern="$2"
  if python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" \
    --source-root "$temp_dir" \
    --lcov "$temp_dir/$name.lcov" \
    >"$temp_dir/$name.out" \
    2>"$temp_dir/$name.err"; then
    echo "$name LCOV fixture must fail" >&2
    exit 1
  fi
  grep -q "$pattern" "$temp_dir/$name.err"
}

cat >"$temp_dir/da-without-source.lcov" <<'LCOV'
DA:1,1
LCOV
expect_lcov_failure "da-without-source" "DA record without an active SF"

cat >"$temp_dir/malformed-da.lcov" <<LCOV
SF:$temp_dir/sample.rs
DA:1,nope
end_of_record
LCOV
expect_lcov_failure "malformed-da" "invalid LCOV data record"

cat >"$temp_dir/negative-hits.lcov" <<LCOV
SF:$temp_dir/sample.rs
DA:1,-1
end_of_record
LCOV
expect_lcov_failure "negative-hits" "negative LCOV hit count"

cat >"$temp_dir/truncated.lcov" <<LCOV
SF:$temp_dir/sample.rs
DA:1,1
LCOV
expect_lcov_failure "truncated" "truncated LCOV source record"

cat >"$temp_dir/nested-source.lcov" <<LCOV
SF:$temp_dir/sample.rs
SF:$temp_dir/other.rs
end_of_record
LCOV
expect_lcov_failure "nested-source" "nested SF record"

cat >"$temp_dir/duplicate-source.lcov" <<LCOV
SF:$temp_dir/sample.rs
DA:1,1
end_of_record
SF:$temp_dir/sample.rs
DA:2,1
end_of_record
LCOV
expect_lcov_failure "duplicate-source" "duplicate LCOV source record"

cat >"$temp_dir/duplicate-da.lcov" <<LCOV
SF:$temp_dir/sample.rs
DA:1,1
DA:1,1
end_of_record
LCOV
expect_lcov_failure "duplicate-da" "duplicate LCOV DA record"

cat >"$temp_dir/conflicting-da.lcov" <<LCOV
SF:$temp_dir/sample.rs
DA:1,1
DA:1,0
end_of_record
LCOV
expect_lcov_failure "conflicting-da" "conflicting LCOV DA record"

cat >"$temp_dir/summary-superset.lcov" <<LCOV
SF:$temp_dir/sample.rs
DA:1,1
LF:913
LH:827
end_of_record
LCOV
summary_report="$(python3 "$repo_root/scripts/ci/propose-stage1-crap-thresholds.py" --source-root "$temp_dir" --lcov "$temp_dir/summary-superset.lcov")"
python3 - "$summary_report" <<'PY'
import json
import sys

report = json.loads(sys.argv[1])
assert report["summary"]["functions_with_coverage"] == 1
assert report["summary"]["functions_without_coverage"] == 8
simple = next(hotspot for hotspot in report["hotspots"] if hotspot["function"] == "simple")
assert simple["coverage"] == 1.0
PY

cat >"$temp_dir/impossible-summary.lcov" <<LCOV
SF:$temp_dir/sample.rs
DA:1,1
LF:1
LH:2
end_of_record
LCOV
expect_lcov_failure "impossible-summary" "LH 2 exceeds LF 1"

cat >"$temp_dir/duplicate-lf.lcov" <<LCOV
SF:$temp_dir/sample.rs
DA:1,1
LF:1
LF:1
LH:1
end_of_record
LCOV
expect_lcov_failure "duplicate-lf" "invalid or duplicate LCOV LF record"

cat >"$temp_dir/duplicate-lh.lcov" <<LCOV
SF:$temp_dir/sample.rs
DA:1,1
LF:1
LH:1
LH:1
end_of_record
LCOV
expect_lcov_failure "duplicate-lh" "invalid or duplicate LCOV LH record"

cat >"$temp_dir/outside-source.lcov" <<'LCOV'
SF:/etc/passwd
DA:1,1
end_of_record
LCOV
expect_lcov_failure "outside-source" "escapes allowed source boundary"

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
