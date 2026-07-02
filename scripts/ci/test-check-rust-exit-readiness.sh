#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-rust-exit-readiness.sh"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

case_dir="$temp_dir/repo"
mkdir -p \
  "$case_dir/docs" \
  "$case_dir/scripts/ci" \
  "$case_dir/stage1/runtime-abi" \
  "$case_dir/stage1/compiler-contracts/snapshots" \
  "$case_dir/stage1/json-fixtures/build" \
  "$case_dir/stage1/json-fixtures/test" \
  "$case_dir/stage1/schemas" \
  "$case_dir/stage1/crates/axiomc/src" \
  "$case_dir/stage1/crates/axiomc/tests" \
  "$case_dir/stage1/crates/axiomc-backend-cranelift/src"
cp "$script" "$case_dir/scripts/ci/check-rust-exit-readiness.sh"
cp "$repo_root/scripts/ci/check-direct-native-runtime-abi.py" "$case_dir/scripts/ci/check-direct-native-runtime-abi.py"
cp "$repo_root/scripts/ci/run-direct-native-runtime-abi-evidence.sh" "$case_dir/scripts/ci/run-direct-native-runtime-abi-evidence.sh"
cp "$repo_root/docs/rust-exit-readiness.md" "$case_dir/docs/rust-exit-readiness.md"
cp "$repo_root/docs/rust-exit-readiness.json" "$case_dir/docs/rust-exit-readiness.json"
cp "$repo_root/stage1/runtime-abi/direct-native-v0.json" "$case_dir/stage1/runtime-abi/direct-native-v0.json"
cp "$repo_root/stage1/runtime-abi/direct-native-v0-evidence-tests.json" "$case_dir/stage1/runtime-abi/direct-native-v0-evidence-tests.json"
cp "$repo_root/stage1/compiler-contracts/snapshots/command-lsp.json" "$case_dir/stage1/compiler-contracts/snapshots/command-lsp.json"
cp "$repo_root/stage1/compiler-contracts/snapshots/mir-backend.json" "$case_dir/stage1/compiler-contracts/snapshots/mir-backend.json"
cp "$repo_root/stage1/json-fixtures/build/success.json" "$case_dir/stage1/json-fixtures/build/success.json"
cp "$repo_root/stage1/json-fixtures/test/filter-success.json" "$case_dir/stage1/json-fixtures/test/filter-success.json"
cp "$repo_root/stage1/json-fixtures/test/failure.json" "$case_dir/stage1/json-fixtures/test/failure.json"
cp "$repo_root/stage1/schemas/axiom-artifacts-v0.schema.json" "$case_dir/stage1/schemas/axiom-artifacts-v0.schema.json"
cp "$repo_root/stage1/crates/axiomc/src/codegen.rs" "$case_dir/stage1/crates/axiomc/src/codegen.rs"
cp "$repo_root/stage1/crates/axiomc/src/main.rs" "$case_dir/stage1/crates/axiomc/src/main.rs"
cp "$repo_root/stage1/crates/axiomc/src/lsp.rs" "$case_dir/stage1/crates/axiomc/src/lsp.rs"
cp "$repo_root/stage1/crates/axiomc/src/cranelift_backend.rs" "$case_dir/stage1/crates/axiomc/src/cranelift_backend.rs"
cp "$repo_root/stage1/crates/axiomc/tests/cranelift_backend.rs" "$case_dir/stage1/crates/axiomc/tests/cranelift_backend.rs"
cp "$repo_root/stage1/crates/axiomc/tests/lsp_stdio.rs" "$case_dir/stage1/crates/axiomc/tests/lsp_stdio.rs"
cp "$repo_root/stage1/crates/axiomc-backend-cranelift/src/lib.rs" "$case_dir/stage1/crates/axiomc-backend-cranelift/src/lib.rs"
cat >"$case_dir/Makefile" <<'MAKE'
rust-exit-readiness:
	bash scripts/ci/check-rust-exit-readiness.sh --json

rust-exit-readiness-github:
	bash scripts/ci/check-rust-exit-readiness.sh --json --require-issue-states

rust-exit-readiness-test:
	bash scripts/ci/test-check-rust-exit-readiness.sh
MAKE

cat >"$temp_dir/partial-issues.txt" <<'ISSUES'
731 OPEN
ISSUES

python3 - "$case_dir/stage1/runtime-abi/direct-native-v0.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    contract = json.load(handle)

contract["status"] = "partial"
for row in contract["value_features"] + contract["capability_shims"]:
    row["status"] = "partial"
    row["blockers"] = [731]

with open(path, "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY

python3 - "$case_dir/docs/rust-exit-readiness.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)

with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

(
  cd "$case_dir"
  if bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/partial-issues.txt" >"$temp_dir/partial-abi.json" 2>"$temp_dir/partial-abi.err"; then
    echo "expected readiness check to fail while direct-native runtime ABI remains partial" >&2
    exit 1
  fi
  python3 - "$temp_dir/partial-abi.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

details = {check["name"]: check["detail"] for check in payload["checks"]}
assert "34 incomplete rows (12 value, 22 capability)" in details[
    "direct_native_runtime_abi_ready"
]
PY
)

cp "$repo_root/docs/rust-exit-readiness.json" "$case_dir/docs/rust-exit-readiness.json"

python3 - "$case_dir/stage1/runtime-abi/direct-native-v0.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    contract = json.load(handle)

contract["status"] = "implemented"
for row in contract["value_features"] + contract["capability_shims"]:
    row["status"] = "implemented"
    row.pop("blockers", None)
    row.setdefault("runtime_evidence", ["stage1/crates/axiomc/tests/cranelift_backend.rs"])

with open(path, "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY

cat >"$temp_dir/open-issues.txt" <<'ISSUES'
731 OPEN
ISSUES

(
  cd "$case_dir"
  if bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/open-issues.txt" >"$temp_dir/blocked.json" 2>"$temp_dir/blocked.err"; then
    echo "expected readiness check to fail while blocking issues remain open" >&2
    exit 1
  fi
  python3 - "$temp_dir/blocked.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["schema"] == "axiom.rust_exit.readiness.v1"
assert payload["ready"] is False
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["readiness_doc_present"] == "pass"
assert statuses["readiness_manifest_valid"] == "pass"
assert statuses["readiness_blockers_closed"] == "fail"
assert statuses["readiness_blockers_live_when_not_ready"] == "pass"
assert statuses["direct_native_runtime_abi_ready"] == "pass"
assert statuses["command_lsp_release_boundary"] == "pass"
assert statuses["lsp_stdio_harness_ready"] == "pass"
assert statuses["lsp_driver_axiom_owned"] == "pass"
assert statuses["mir_backend_direct_native_boundary"] == "pass"
assert statuses["generated_rust_cli_gate"] == "pass"
assert statuses["generated_rust_contract_gate"] == "pass"
PY
)

cat >"$temp_dir/issues.txt" <<'ISSUES'
731 CLOSED
ISSUES

python3 - "$case_dir/stage1/crates/axiomc/src/main.rs" "$case_dir/stage1/crates/axiomc/src/lsp.rs" <<'PY'
import sys
from pathlib import Path

main_path = Path(sys.argv[1])
lsp_path = Path(sys.argv[2])
main_path.write_text(
    main_path.read_text(encoding="utf-8").replace(
        "lsp::serve_stdio(io::stdin().lock(), io::stdout())",
        "lsp::run_stdio(io::stdin().lock(), io::stdout())",
    ),
    encoding="utf-8",
)
lsp_path.write_text(
    "// old fixture markers: axiom_lsp::run_stdio axiom_lsp::handle_message\n"
    + lsp_path.read_text(encoding="utf-8"),
    encoding="utf-8",
)
PY

(
  cd "$case_dir"
  if bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/issues.txt" >"$temp_dir/rust-lsp-driver.json" 2>"$temp_dir/rust-lsp-driver.err"; then
    echo "expected readiness check to fail while axiomc lsp still uses the Rust-hosted stdio loop" >&2
    exit 1
  fi
  python3 - "$temp_dir/rust-lsp-driver.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["ready"] is False
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["readiness_blockers_closed"] == "pass"
assert statuses["readiness_blockers_live_when_not_ready"] == "pass"
assert statuses["rust_exit_issue_731_closed"] == "pass"
assert statuses["direct_native_runtime_abi_ready"] == "pass"
assert statuses["command_lsp_release_boundary"] == "pass"
assert statuses["lsp_stdio_harness_ready"] == "pass"
assert statuses["lsp_driver_axiom_owned"] == "fail"
assert statuses["mir_backend_direct_native_boundary"] == "pass"
assert statuses["generated_rust_cli_gate"] == "pass"
assert statuses["generated_rust_contract_gate"] == "pass"
PY
)

python3 - "$case_dir/stage1/crates/axiomc/src/main.rs" "$case_dir/stage1/crates/axiomc/src/lsp.rs" <<'PY'
import sys
from pathlib import Path

main_path = Path(sys.argv[1])
lsp_path = Path(sys.argv[2])
main_path.write_text(
    main_path.read_text(encoding="utf-8")
    .replace(
        "lsp::run_stdio(io::stdin().lock(), io::stdout())",
        "compiler_services_lsp_serve_stdio(io::stdin().lock(), io::stdout())",
    )
    .replace(
        "lsp::serve_stdio(io::stdin().lock(), io::stdout())",
        "compiler_services_lsp_serve_stdio(io::stdin().lock(), io::stdout())",
    ),
    encoding="utf-8",
)
lsp_path.write_text(
    lsp_path.read_text(encoding="utf-8")
    .replace("axiom_lsp::run_stdio", "compiled_lsp_service::serve_stdio")
    .replace("axiom_lsp::handle_message", "compiled_lsp_service::handle_message"),
    encoding="utf-8",
)
PY

(
  cd "$case_dir"
  if ! bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/issues.txt" >"$temp_dir/ready.json" 2>"$temp_dir/ready.err"; then
    echo "expected readiness check to pass once blocking issues are closed, ABI is ready, and axiomc lsp no longer uses the Rust-hosted driver" >&2
    cat "$temp_dir/ready.json" >&2
    cat "$temp_dir/ready.err" >&2
    exit 1
  fi
  python3 - "$temp_dir/ready.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["ready"] is True
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["readiness_blockers_closed"] == "pass"
assert statuses["lsp_stdio_harness_ready"] == "pass"
assert statuses["lsp_driver_axiom_owned"] == "pass"
assert statuses["generated_rust_cli_gate"] == "pass"
assert statuses["generated_rust_contract_gate"] == "pass"
PY
)

python3 - "$case_dir/stage1/crates/axiomc/src/codegen.rs" <<'PY'
import sys
from pathlib import Path

path = Path(sys.argv[1])
source = path.read_text(encoding="utf-8")
path.write_text(
    source.replace(
        '"cranelift" => Ok(Self::Cranelift),',
        '"generated-rust" => Ok(Self::GeneratedRust),\n            "cranelift" => Ok(Self::Cranelift),',
    ),
    encoding="utf-8",
)
PY

(
  cd "$case_dir"
  if bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/issues.txt" >"$temp_dir/generated-rust-cli-gate.json" 2>"$temp_dir/generated-rust-cli-gate.err"; then
    echo "expected readiness check to fail when generated-rust CLI parsing is restored" >&2
    exit 1
  fi
  python3 - "$temp_dir/generated-rust-cli-gate.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["generated_rust_cli_gate"] == "fail"
PY
)

cp "$repo_root/stage1/crates/axiomc/src/codegen.rs" "$case_dir/stage1/crates/axiomc/src/codegen.rs"
cp "$repo_root/stage1/crates/axiomc/src/main.rs" "$case_dir/stage1/crates/axiomc/src/main.rs"

python3 - "$case_dir/stage1/json-fixtures/build/success.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)

payload["backend"] = "generated-rust"
payload["generated_rust"] = "<project>/dist/contract-app.generated.rs"
payload["cache_key"]["generated_rust_hash"] = payload["cache_key"].pop("backend_input_hash")

with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

(
  cd "$case_dir"
  if bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/issues.txt" >"$temp_dir/generated-rust-contract-gate.json" 2>"$temp_dir/generated-rust-contract-gate.err"; then
    echo "expected readiness check to fail when generated-rust remains in command fixtures" >&2
    exit 1
  fi
  python3 - "$temp_dir/generated-rust-contract-gate.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["generated_rust_contract_gate"] == "fail"
PY
)

cp "$repo_root/stage1/json-fixtures/build/success.json" "$case_dir/stage1/json-fixtures/build/success.json"

cp "$repo_root/docs/rust-exit-readiness.json" "$case_dir/docs/rust-exit-readiness.json"

python3 - "$case_dir/docs/rust-exit-readiness.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)

payload["blockingIssues"].append(
    {
        "issue": payload["finalBootstrapIssue"],
        "lane": "bootstrap",
        "check": "Rust bootstrap is no longer needed for the supported toolchain.",
    }
)

with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

(
  cd "$case_dir"
  if bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/issues.txt" >"$temp_dir/final-self-blocker.json" 2>"$temp_dir/final-self-blocker.err"; then
    echo "expected readiness check to fail when finalBootstrapIssue is also listed as a blocker" >&2
    exit 1
  fi
  if ! rg -q "finalBootstrapIssue must not also be listed as a blocker" "$temp_dir/final-self-blocker.err"; then
    echo "expected finalBootstrapIssue self-blocker validation error" >&2
    cat "$temp_dir/final-self-blocker.err" >&2
    exit 1
  fi
)

cp "$repo_root/docs/rust-exit-readiness.json" "$case_dir/docs/rust-exit-readiness.json"

cp "$repo_root/stage1/runtime-abi/direct-native-v0.json" "$case_dir/stage1/runtime-abi/direct-native-v0.json"

python3 - "$case_dir/stage1/runtime-abi/direct-native-v0.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    contract = json.load(handle)

contract["status"] = "partial"
contract["value_features"][0]["status"] = "partial"
contract["value_features"][0]["blockers"] = [1191]

with open(path, "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY

(
  cd "$case_dir"
  if bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/issues.txt" >"$temp_dir/stale-closed-blocker.json" 2>"$temp_dir/stale-closed-blocker.err"; then
    echo "expected readiness check to fail when an ABI blocker is missing from the manifest" >&2
    exit 1
  fi
  if ! rg -q "ABI blocker issues missing from readiness manifest: #1191" "$temp_dir/stale-closed-blocker.err"; then
    echo "expected missing ABI blocker validation error" >&2
    cat "$temp_dir/stale-closed-blocker.err" >&2
    exit 1
  fi
)

python3 - "$case_dir/stage1/runtime-abi/direct-native-v0.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    contract = json.load(handle)

contract["status"] = "implemented"
for row in contract["value_features"] + contract["capability_shims"]:
    row["status"] = "implemented"
    row.pop("blockers", None)
    row.setdefault("runtime_evidence", ["stage1/crates/axiomc/tests/cranelift_backend.rs"])

with open(path, "w", encoding="utf-8") as handle:
    json.dump(contract, handle)
PY

python3 - "$case_dir/stage1/compiler-contracts/snapshots/command-lsp.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
payload["official_release"]["requires_cargo"] = True
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

(
  cd "$case_dir"
  if bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/issues.txt" >"$temp_dir/cargo-boundary.json" 2>"$temp_dir/cargo-boundary.err"; then
    echo "expected readiness check to fail when command/LSP release requires Cargo" >&2
    exit 1
  fi
  python3 - "$temp_dir/cargo-boundary.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["command_lsp_release_boundary"] == "fail"
PY
)

cp "$repo_root/stage1/compiler-contracts/snapshots/command-lsp.json" "$case_dir/stage1/compiler-contracts/snapshots/command-lsp.json"

python3 - "$case_dir/stage1/compiler-contracts/snapshots/mir-backend.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
for target in payload["targets"]:
    if target["id"] == "axiom://target/stage1-direct-native":
        target["primary_artifacts"].append("rust_source")
        break
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

(
  cd "$case_dir"
  if bash scripts/ci/check-rust-exit-readiness.sh --json --issue-state-file "$temp_dir/issues.txt" >"$temp_dir/mir-boundary.json" 2>"$temp_dir/mir-boundary.err"; then
    echo "expected readiness check to fail when direct-native target emits rust_source" >&2
    exit 1
  fi
  python3 - "$temp_dir/mir-boundary.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
statuses = {check["name"]: check["status"] for check in payload["checks"]}
assert statuses["mir_backend_direct_native_boundary"] == "fail"
PY
)

echo "check-rust-exit-readiness regression cases passed"
