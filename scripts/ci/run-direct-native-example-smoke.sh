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
  agent_native_authorize
  arrays
  benchmarks
  capabilities
  compiler_properties
  decision_records
  maps
  openapi_service
  tuples
  structs
  enums
  borrowed_shapes
  generic_aggregates
  modules
  outcomes
  packages
  policy_bundle_service
  proof_cli
  proof_http_service
  proof_worker
  property_smoke
  runbook_service
  semantic_verifier
  slices
  sql_migration_service
  stdlib_async
  stdlib_cli
  stdlib_collection_lookup
  stdlib_collections
  stdlib_crypto_hash
  stdlib_crypto_mac
  stdlib_doc
  stdlib_encoding
  stdlib_env
  stdlib_env_unrestricted
  stdlib_fs
  stdlib_fs_write
  stdlib_http
  stdlib_io
  stdlib_json
  stdlib_json_value
  stdlib_log
  stdlib_lsp
  stdlib_net
  stdlib_net_tcp_async
  stdlib_outcome
  stdlib_process
  stdlib_regex
  stdlib_serdes
  stdlib_sync
  stdlib_testing
  stdlib_string_builder
  stdlib_time
  terraform_runtime_service
  workspace
  "workspace_only|workspace-app"
)

validate_payload() {
  local command="$1"
  local project="$2"
  local package="$3"
  local payload="$4"
  local payload_file
  payload_file="$(mktemp "${RUNNER_TEMP:-/tmp}/axiom-direct-native-payload.XXXXXX")"
  printf '%s\n' "$payload" >"$payload_file"
  python3 - "$command" "$project" "$package" "$payload_file" <<'PY'
import json
import sys

command, project, package, payload_file = sys.argv[1:]
with open(payload_file, "r", encoding="utf-8") as handle:
    payload = json.load(handle)

errors = []
if payload.get("command") != command:
    errors.append(f"command={payload.get('command')!r}")
if payload.get("project") != project:
    errors.append(f"project={payload.get('project')!r}")
if package and command == "run" and payload.get("package") != package:
    errors.append(f"package={payload.get('package')!r}")
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

for entry in "${examples[@]}"; do
  example="${entry%%|*}"
  package=""
  if [[ "$entry" == *"|"* ]]; then
    package="${entry#*|}"
  fi
  project="stage1/examples/${example}"
  label="$project"
  if [[ -n "$package" ]]; then
    label="${label} (${package})"
  fi
  echo "direct-native example smoke: ${label}"
  if [[ -n "$package" ]]; then
    "$axiomc_bin" check "$project" --package "$package" --json >/dev/null
    build_payload="$("$axiomc_bin" build "$project" --package "$package" --backend cranelift --json)"
    run_payload="$("$axiomc_bin" run "$project" --package "$package" --backend cranelift --json)"
  else
    "$axiomc_bin" check "$project" --json >/dev/null
    build_payload="$("$axiomc_bin" build "$project" --backend cranelift --json)"
    run_payload="$("$axiomc_bin" run "$project" --backend cranelift --json)"
  fi
  validate_payload build "$project" "$package" "$build_payload"
  validate_payload run "$project" "$package" "$run_payload"
done
