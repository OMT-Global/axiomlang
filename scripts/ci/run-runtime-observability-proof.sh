#!/usr/bin/env bash
set -euo pipefail

script_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)
      [[ $# -ge 2 ]] || { echo "--root requires a checkout path" >&2; exit 2; }
      repo_root="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

[[ -n "$repo_root" ]] || { echo "--root is required" >&2; exit 2; }
repo_root="$(cd "$repo_root" && pwd -P)"
manifest="$repo_root/stage1/Cargo.toml"
harness="$script_root/fixtures/runtime-observability-proof.rs"
[[ -f "$manifest" ]] || { echo "missing PR stage1 manifest: $manifest" >&2; exit 1; }
[[ -f "$harness" ]] || { echo "missing trusted observability harness: $harness" >&2; exit 1; }

temporary="$(mktemp -d "${TMPDIR:-/tmp}/axiom-observability-proof.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT
target_dir="$temporary/target"

CARGO_TARGET_DIR="$target_dir" cargo build \
  --locked \
  --manifest-path "$manifest" \
  -p axiomc \
  --lib

shopt -s nullglob
libraries=("$target_dir"/debug/deps/libaxiomc-*.rlib)
serde_json_libraries=("$target_dir"/debug/deps/libserde_json-*.rlib)
shopt -u nullglob
if [[ ${#libraries[@]} -ne 1 ]]; then
  echo "expected exactly one freshly built axiomc library, found ${#libraries[@]}" >&2
  exit 1
fi
if [[ ${#serde_json_libraries[@]} -ne 1 ]]; then
  echo "expected exactly one freshly built serde_json library, found ${#serde_json_libraries[@]}" >&2
  exit 1
fi

rustc \
  --edition=2024 \
  "$harness" \
  --extern "axiomc=${libraries[0]}" \
  --extern "serde_json=${serde_json_libraries[0]}" \
  -L "dependency=$target_dir/debug/deps" \
  -o "$temporary/runtime-observability-proof"

"$temporary/runtime-observability-proof" >"$temporary/runtime-evidence.json"
python3 "$script_root/check-runtime-observability-v1.py" \
  --root "$repo_root" \
  --runtime-evidence "$temporary/runtime-evidence.json"
