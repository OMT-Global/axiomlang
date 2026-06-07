#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-command-lsp-boundary.py"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

python3 "$script" --json >"$temp_dir/result.json"

python3 - "$temp_dir/result.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["schema"] == "axiom.compiler.command_lsp.v1"
assert payload["ok"] is True
assert payload["commands"] == 7
assert payload["lsp_services"] == 7
PY

python3 - "$repo_root/stage1/compiler-contracts/snapshots/command-lsp.json" "$temp_dir/unexpected-field.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["commands"][0]["rust_module"] = "main.rs"

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/unexpected-field.json" >"$temp_dir/unexpected.out" 2>"$temp_dir/unexpected.err"; then
  echo "expected schema-invalid command boundary output to fail" >&2
  exit 1
fi

grep -q "unexpected fields" "$temp_dir/unexpected.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/command-lsp.json" "$temp_dir/cargo-release.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["official_release"]["requires_cargo"] = True

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/cargo-release.json" >"$temp_dir/cargo.out" 2>"$temp_dir/cargo.err"; then
  echo "expected Cargo-required release output to fail" >&2
  exit 1
fi

grep -q "must not require Cargo" "$temp_dir/cargo.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/command-lsp.json" "$temp_dir/missing-lsp-package.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

for service in payload["lsp_services"]:
    service["delegates_to"] = [
        delegated for delegated in service["delegates_to"]
        if not delegated.startswith("compiler.hir.")
    ]

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/missing-lsp-package.json" >"$temp_dir/lsp.out" 2>"$temp_dir/lsp.err"; then
  echo "expected missing LSP package delegation to fail" >&2
  exit 1
fi

grep -q "LSP services must call package graph, syntax, HIR, diagnostics, and evidence packages" "$temp_dir/lsp.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/command-lsp.json" "$temp_dir/rust-capture.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["commands"][0]["delegates_to"].append("main.rs::check_project")

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/rust-capture.json" >"$temp_dir/rust.out" 2>"$temp_dir/rust.err"; then
  echo "expected Rust-captured command delegation to fail" >&2
  exit 1
fi

grep -q "Rust capture term" "$temp_dir/rust.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/command-lsp.json" "$temp_dir/cargo-rustc-capture.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["commands"][0]["inputs"].append("cargo_metadata")
payload["commands"][1]["delegates_to"].append("rustc.invoke")

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/cargo-rustc-capture.json" >"$temp_dir/cargo-rustc.out" 2>"$temp_dir/cargo-rustc.err"; then
  echo "expected Cargo/rustc-captured command delegation to fail" >&2
  exit 1
fi

grep -q "Rust capture term 'cargo'" "$temp_dir/cargo-rustc.err"

echo "check-command-lsp-boundary regression cases passed"
