#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

target_dir="${CARGO_TARGET_DIR:-$repo_root/target/direct-native-runtime-abi}"
mkdir -p "$target_dir"
export CARGO_TARGET_DIR="$target_dir"
row_test_file=""
row_list_file=""
trap '[[ -z "${row_test_file:-}" ]] || rm -f "$row_test_file"; [[ -z "${row_list_file:-}" ]] || rm -f "$row_list_file"' EXIT

if ! command -v cargo >/dev/null 2>&1; then
  for cargo_bin_dir in "${CARGO_HOME:-$HOME/.cargo}/bin" /usr/local/cargo/bin; do
    if [[ -x "$cargo_bin_dir/cargo" ]]; then
      export PATH="$cargo_bin_dir:$PATH"
      break
    fi
  done
fi

command -v cargo >/dev/null 2>&1 || {
  echo "cargo is required but was not found in PATH" >&2
  exit 127
}

python3 scripts/ci/check-direct-native-runtime-abi.py --json

dry_run=0
if [[ -n "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_DRY_RUN:-}" ]]; then
  dry_run=1
fi

selector_count=0
for selector in \
  "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW:-}" \
  "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_STATUS:-}" \
  "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER:-}"
do
  if [[ -n "$selector" ]]; then
    selector_count=$((selector_count + 1))
  fi
done

if ((selector_count > 1)); then
  echo "set only one of AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW, AXIOM_DIRECT_NATIVE_RUNTIME_ABI_STATUS, or AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER" >&2
  exit 1
fi

cranelift_cargo_base=(
  cargo
  test
  --manifest-path stage1/Cargo.toml
  -p axiomc
  --test cranelift_backend
)

run_or_print() {
  if ((dry_run)); then
    printf 'dry-run:'
    printf ' %q' "$@"
    printf '\n'
    return 0
  fi

  "$@"
}

if [[ -n "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW:-}" ]]; then
  row_test_file="$target_dir/abi-row-tests-$$.txt"
  row_tests=()
  python3 scripts/ci/check-direct-native-runtime-abi.py \
    --evidence-row "$AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW" >"$row_test_file"

  while IFS= read -r test_name; do
    row_tests+=("$test_name")
  done < "$row_test_file"
  rm -f "$row_test_file"

  if ((${#row_tests[@]} == 0)); then
    echo "direct native runtime ABI evidence row has no tests: $AXIOM_DIRECT_NATIVE_RUNTIME_ABI_ROW" >&2
    exit 1
  fi

  for row_test in "${row_tests[@]}"; do
    run_or_print "${cranelift_cargo_base[@]}" "$row_test" -- --nocapture --test-threads=1
  done
elif [[ -n "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_STATUS:-}" ]]; then
  row_test_file="$target_dir/abi-row-status-tests-$$.txt"
  row_list_file="$target_dir/abi-row-status-list-$$.json"
  row_tests=()
  python3 scripts/ci/check-direct-native-runtime-abi.py \
    --list-evidence-rows \
    --json >"$row_list_file"
  python3 - "$AXIOM_DIRECT_NATIVE_RUNTIME_ABI_STATUS" "$row_list_file" >"$row_test_file" <<'PY'
import json
import sys

status = sys.argv[1]
row_list_path = sys.argv[2]
valid_statuses = {"implemented", "partial", "blocked"}
if status not in valid_statuses:
    print(
        "unknown direct native runtime ABI row status: "
        f"{status}; expected one of {', '.join(sorted(valid_statuses))}",
        file=sys.stderr,
    )
    raise SystemExit(1)

with open(row_list_path, encoding="utf-8") as handle:
    report = json.load(handle)
seen = set()
for row in report.get("rows", []):
    if row.get("status") != status:
        continue
    for test_name in row.get("tests", []):
        if not isinstance(test_name, str) or test_name in seen:
            continue
        print(test_name)
        seen.add(test_name)
PY

  while IFS= read -r test_name; do
    row_tests+=("$test_name")
  done < "$row_test_file"
  rm -f "$row_test_file"

  if ((${#row_tests[@]} == 0)); then
    echo "direct native runtime ABI status has no tests: $AXIOM_DIRECT_NATIVE_RUNTIME_ABI_STATUS" >&2
    exit 1
  fi

  for row_test in "${row_tests[@]}"; do
    run_or_print "${cranelift_cargo_base[@]}" "$row_test" -- --nocapture --test-threads=1
  done
else
  cargo_args=("${cranelift_cargo_base[@]}")

  if [[ -n "${AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER:-}" ]]; then
    cargo_args+=("$AXIOM_DIRECT_NATIVE_RUNTIME_ABI_TEST_FILTER")
  fi

  cargo_args+=(-- --nocapture --test-threads=1)

  run_or_print "${cargo_args[@]}"
fi

for command_evidence in \
  cranelift_run_report_executes_without_generated_rust_artifact \
  cranelift_test_case_executes_without_generated_rust_artifact
do
  run_or_print \
    cargo test \
    --manifest-path stage1/Cargo.toml \
    -p axiomc \
    --lib \
    "$command_evidence" \
    -- --nocapture
done
