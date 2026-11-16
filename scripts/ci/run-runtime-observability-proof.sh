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

# Ask Cargo where it put each artifact instead of assuming target/debug/deps:
# Cargo 1.100 moved intermediate artifacts to per-package build directories.
CARGO_TARGET_DIR="$target_dir" cargo build \
  --locked \
  --manifest-path "$manifest" \
  -p axiomc \
  --lib \
  --message-format=json >"$temporary/cargo-messages.jsonl"

python3 - "$temporary/cargo-messages.jsonl" "$temporary/artifacts" <<'PY'
import json
import os
import sys

messages_path, output_dir = sys.argv[1:3]
wanted = {"axiomc": [], "serde_json": []}
search_dirs = []
with open(messages_path, encoding="utf-8") as messages:
    for line in messages:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(message, dict) or message.get("reason") != "compiler-artifact":
            continue
        target = message.get("target", {})
        filenames = [name for name in message.get("filenames", []) if isinstance(name, str)]
        for name in filenames:
            directory = os.path.dirname(name)
            if directory not in search_dirs:
                search_dirs.append(directory)
        name = target.get("name")
        if name in wanted and "lib" in target.get("kind", []):
            wanted[name].extend(f for f in filenames if f.endswith(".rlib"))

os.makedirs(output_dir)
for name, libraries in wanted.items():
    with open(os.path.join(output_dir, name), "w", encoding="utf-8") as handle:
        handle.writelines(f"{library}\n" for library in libraries)
with open(os.path.join(output_dir, "search-dirs"), "w", encoding="utf-8") as handle:
    handle.writelines(f"{directory}\n" for directory in search_dirs)
PY

mapfile -t libraries <"$temporary/artifacts/axiomc"
mapfile -t serde_json_libraries <"$temporary/artifacts/serde_json"
mapfile -t search_dirs <"$temporary/artifacts/search-dirs"
if [[ ${#libraries[@]} -ne 1 ]]; then
  echo "expected exactly one freshly built axiomc library, found ${#libraries[@]}" >&2
  exit 1
fi
if [[ ${#serde_json_libraries[@]} -ne 1 ]]; then
  echo "expected exactly one freshly built serde_json library, found ${#serde_json_libraries[@]}" >&2
  exit 1
fi

search_args=()
for directory in "${search_dirs[@]}"; do
  search_args+=(-L "dependency=$directory")
done

rustc \
  --edition=2024 \
  "$harness" \
  --extern "axiomc=${libraries[0]}" \
  --extern "serde_json=${serde_json_libraries[0]}" \
  "${search_args[@]}" \
  -o "$temporary/runtime-observability-proof"

"$temporary/runtime-observability-proof" >"$temporary/runtime-evidence.json"
python3 "$script_root/check-runtime-observability-v1.py" \
  --root "$repo_root" \
  --runtime-evidence "$temporary/runtime-evidence.json"
