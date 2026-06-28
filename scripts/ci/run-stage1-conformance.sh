#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

report="$(mktemp "${TMPDIR:-/tmp}/axiom-stage1-conformance-cranelift.XXXXXX.json")"
if ! cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test --conformance --backend cranelift --json >"$report"; then
  cat "$report" >&2
  exit 1
fi

python3 - "$report" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
if payload.get("backend") != "cranelift":
    raise SystemExit(f"stage1 conformance must run on cranelift, got {payload.get('backend')!r}")
if payload.get("ok") is not True:
    raise SystemExit("stage1 conformance must pass on cranelift")
if payload.get("properties", {}).get("total", 0) <= 0:
    raise SystemExit("stage1 conformance must report at least one property")
for case in payload.get("cases", []):
    if case.get("generated_rust") is not None:
        raise SystemExit(f"conformance case {case.get('name')} used generated Rust")

print(
    "stage1 conformance passed on cranelift "
    f"({payload.get('properties', {}).get('passed', 0)}/{payload.get('properties', {}).get('total', 0)} properties)"
)
PY
