#!/usr/bin/env bash
set -euo pipefail

# Parity gate for the self-hosting feasibility spike (#1253).
#
# Proves the AxiOM compiler.diagnostics slice and the Rust implementation
# produce identical output over the shared corpus:
#   1. The Rust side must match the checked-in expected-output.txt
#      (cargo test self_hosting_spike_parity).
#   2. The AxiOM side must build and run through stage1 axiomc on the
#      direct-native backend with generated_rust null, and its stdout must
#      match the same expected-output.txt.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

spike_dir="stage1/selfhost/compiler-diagnostics-spike"
expected="$spike_dir/expected-output.txt"

cargo test --manifest-path stage1/Cargo.toml -p axiomc --test self_hosting_spike_parity

report="$(mktemp "${TMPDIR:-/tmp}/axiom-selfhost-spike.XXXXXX")"
trap 'rm -f "$report"' EXIT

cargo run --quiet --manifest-path stage1/Cargo.toml -p axiomc -- run "$spike_dir" --json >"$report"

python3 - "$report" "$expected" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
expected = open(sys.argv[2], encoding="utf-8").read()

if payload.get("ok") is not True:
    raise SystemExit(f"spike run failed: {payload.get('error')}")
if payload.get("backend") != "cranelift":
    raise SystemExit(f"spike must run direct-native, got backend {payload.get('backend')!r}")
if payload.get("generated_rust") is not None:
    raise SystemExit("spike run must not use generated Rust")
if payload.get("exit_code") != 0:
    raise SystemExit(f"spike exited with {payload.get('exit_code')}")
if payload.get("stdout") != expected:
    import difflib

    diff = "\n".join(
        difflib.unified_diff(
            expected.splitlines(),
            (payload.get("stdout") or "").splitlines(),
            "expected (rust)",
            "actual (axiom)",
            lineterm="",
        )
    )
    raise SystemExit(f"AxiOM spike output diverged from Rust parity corpus:\n{diff}")

lines = expected.count("\n")
print(f"self-hosting diagnostics spike parity passed ({lines} corpus lines, direct-native, generated_rust null)")
PY
