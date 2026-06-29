#!/usr/bin/env bash
set -euo pipefail

conformance_report="$(mktemp "${TMPDIR:-/tmp}/axiom-conformance-cranelift.XXXXXX")"
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test --conformance --backend cranelift --json >"$conformance_report"

python3 - "$conformance_report" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
if payload.get("ok") is not True:
    raise SystemExit("direct-native conformance must pass")
if payload.get("backend") != "cranelift":
    raise SystemExit(f"conformance must run on cranelift, got {payload.get('backend')!r}")
if payload.get("generated_rust") is not None:
    raise SystemExit("direct-native conformance must not emit generated Rust")
for case in payload.get("cases", []):
    if case.get("generated_rust") is not None:
        raise SystemExit(f"conformance case {case.get('name')!r} emitted generated Rust")
properties = payload.get("properties", {})
if properties.get("total") != properties.get("passed") or properties.get("failed") != 0:
    raise SystemExit(f"unexpected conformance property totals: {properties!r}")
PY

target_report="$(mktemp "${TMPDIR:-/tmp}/axiom-targeted-build.XXXXXX")"
if cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --target wasm32 --json >"$target_report"; then
  echo "default targeted builds must not silently fall back to generated Rust" >&2
  exit 1
fi

python3 - "$target_report" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
message = payload.get("error", {}).get("message", "")
if payload.get("ok") is not False:
    raise SystemExit("targeted build failure must return ok=false")
if "cranelift backend spike currently supports only the host target" not in message:
    raise SystemExit(f"unexpected targeted build failure: {message}")
PY
