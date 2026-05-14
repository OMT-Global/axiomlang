#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

mkdir -p "$tmpdir/good" "$tmpdir/bad-key" "$tmpdir/bad-env" "$tmpdir/bad-fs"

cat > "$tmpdir/good/axiom.toml" <<'TOML'
[package]
name = "good"
version = "0.1.0"

[capabilities]
fs = true
"fs:write" = true
fs_root = 'data'
env = [
  "LOG_LEVEL",
]
net = false
process = false
clock = false
crypto = false
ffi = false
async = false
unsafe_rationale = "test fixture documents unrestricted grants"
TOML

python3 "$repo_root/scripts/ci/validate-capability-manifests.py" --root "$tmpdir"

cat > "$tmpdir/bad-key/axiom.toml" <<'TOML'
[package]
name = "bad-key"
version = "0.1.0"

[capabilities]
shell = true
TOML

if python3 "$repo_root/scripts/ci/validate-capability-manifests.py" --root "$tmpdir" 2> "$tmpdir/bad-key.err"; then
  echo "validator accepted an unknown capability key" >&2
  exit 1
fi
grep -F "unknown [capabilities] key 'shell'" "$tmpdir/bad-key.err" >/dev/null
rm "$tmpdir/bad-key/axiom.toml"

cat > "$tmpdir/bad-env/axiom.toml" <<'TOML'
[package]
name = "bad-env"
version = "0.1.0"

[capabilities]
env = ["TOKEN=secret"]
TOML

if python3 "$repo_root/scripts/ci/validate-capability-manifests.py" --root "$tmpdir" 2> "$tmpdir/bad-env.err"; then
  echo "validator accepted a NAME=value env allowlist entry" >&2
  exit 1
fi
grep -F "must be a variable name, not NAME=value" "$tmpdir/bad-env.err" >/dev/null
rm "$tmpdir/bad-env/axiom.toml"

cat > "$tmpdir/bad-fs/axiom.toml" <<'TOML'
[package]
name = "bad-fs"
version = "0.1.0"

[capabilities]
fs_root = "../outside"
TOML

if python3 "$repo_root/scripts/ci/validate-capability-manifests.py" --root "$tmpdir" 2> "$tmpdir/bad-fs.err"; then
  echo "validator accepted parent traversal in fs_root" >&2
  exit 1
fi
grep -F "fs_root must not use parent traversal" "$tmpdir/bad-fs.err" >/dev/null

echo "capability manifest validator tests passed"
