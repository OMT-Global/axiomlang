#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

target_dir="${CARGO_TARGET_DIR:-${RUNNER_TEMP:-/tmp}/axiom-direct-native-example-smoke-target}"
mkdir -p "$target_dir"
export CARGO_TARGET_DIR="$target_dir"

cargo build --manifest-path stage1/Cargo.toml -p axiomc
axiomc_bin="$target_dir/debug/axiomc"

examples=(
  hello
  arrays
  maps
  tuples
  structs
  enums
  borrowed_shapes
  generic_aggregates
  modules
  outcomes
  packages
  stdlib_async
  stdlib_cli
  stdlib_collection_lookup
  stdlib_collections
  stdlib_crypto_hash
  stdlib_crypto_mac
  stdlib_doc
  stdlib_encoding
  stdlib_env
  stdlib_fs
  stdlib_fs_write
  stdlib_http
  stdlib_io
  stdlib_json
  stdlib_json_value
  stdlib_log
  stdlib_lsp
  stdlib_outcome
  stdlib_process
  stdlib_regex
  stdlib_serdes
  stdlib_sync
  stdlib_testing
  stdlib_string_builder
  stdlib_time
  workspace
)

validate_payload() {
  local command="$1"
  local project="$2"
  local payload="$3"
  local payload_file
  payload_file="$(mktemp "${RUNNER_TEMP:-/tmp}/axiom-direct-native-payload.XXXXXX")"
  printf '%s\n' "$payload" >"$payload_file"
  python3 - "$command" "$project" "$payload_file" <<'PY'
import json
import sys

command, project, payload_file = sys.argv[1:]
with open(payload_file, "r", encoding="utf-8") as handle:
    payload = json.load(handle)

errors = []
if payload.get("command") != command:
    errors.append(f"command={payload.get('command')!r}")
if payload.get("project") != project:
    errors.append(f"project={payload.get('project')!r}")
if payload.get("backend") != "cranelift":
    errors.append(f"backend={payload.get('backend')!r}")
if payload.get("generated_rust") is not None:
    errors.append("generated_rust is not null")
if payload.get("ok") is not True:
    errors.append(f"ok={payload.get('ok')!r}")
if command == "run" and payload.get("exit_code") != 0:
    errors.append(f"exit_code={payload.get('exit_code')!r}")

if errors:
    print(
        f"{project}: invalid direct-native {command} payload: {', '.join(errors)}",
        file=sys.stderr,
    )
    sys.exit(1)
PY
  rm -f "$payload_file"
}

for example in "${examples[@]}"; do
  project="stage1/examples/${example}"
  echo "direct-native example smoke: ${project}"
  "$axiomc_bin" check "$project" --json >/dev/null
  build_payload="$("$axiomc_bin" build "$project" --backend cranelift --json)"
  validate_payload build "$project" "$build_payload"
  run_payload="$("$axiomc_bin" run "$project" --backend cranelift --json)"
  validate_payload run "$project" "$run_payload"
done
