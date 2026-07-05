#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/render-direct-native-runtime-abi-status.py"
contract="$repo_root/stage1/runtime-abi/direct-native-v0.json"
doc="$repo_root/docs/direct-native-runtime-abi-v0.md"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

python3 "$script" --contract "$contract" >"$temp_dir/status.md"

grep -Fq '<!-- direct-native-runtime-abi-status:start -->' "$temp_dir/status.md"
grep -Fq '| `numeric.scalars` | `implemented` | - | evidence:1, runtime:1 |' "$temp_dir/status.md"
grep -Fq '| `io.logging_stdio` | `implemented` | - | evidence:1, runtime:2 |' "$temp_dir/status.md"
grep -Fq '<!-- direct-native-runtime-abi-status:end -->' "$temp_dir/status.md"

python3 "$script" --contract "$contract" --check-doc "$doc"

cp "$doc" "$temp_dir/stale.md"
perl -0pi -e 's/`numeric\.scalars`/`numeric.scalars.stale`/' "$temp_dir/stale.md"
if python3 "$script" --contract "$contract" --check-doc "$temp_dir/stale.md"; then
  echo "expected stale generated ABI status table to fail" >&2
  exit 1
fi

echo "direct native runtime ABI status renderer regression cases passed"
