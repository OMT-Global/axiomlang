#!/usr/bin/env bash
set -euo pipefail

# Broad conformance remains a generated-Rust compatibility lane until the full
# conformance surface has direct-native runtime evidence.
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test --conformance --backend generated-rust --json

target_report="$(mktemp "${TMPDIR:-/tmp}/axiom-targeted-build.XXXXXX.json")"
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
