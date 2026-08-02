#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/check-package-graph-boundary.py"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT
cd "$repo_root"

python3 "$script" --json >"$temp_dir/result.json"

python3 - "$temp_dir/result.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

assert payload["schema"] == "axiom.compiler.package_graph.v1"
assert payload["ok"] is True
assert payload["packages"] == 3
PY

python3 - "$repo_root/stage1/compiler-contracts/snapshots/package-graph.json" "$temp_dir/unexpected-field.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["outputs"]["packages"][0]["unexpected"] = "drift"

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/unexpected-field.json" >"$temp_dir/unexpected.out" 2>"$temp_dir/unexpected.err"; then
  echo "expected schema-invalid package graph output to fail" >&2
  exit 1
fi

grep -q "unexpected fields" "$temp_dir/unexpected.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/package-graph-runtime.json" "$temp_dir/runtime-unexpected.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["packages"][0]["unexpected"] = "drift"

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --runtime-snapshot "$temp_dir/runtime-unexpected.json" >"$temp_dir/runtime-unexpected.out" 2>"$temp_dir/runtime-unexpected.err"; then
  echo "expected schema-invalid runtime package graph output to fail" >&2
  exit 1
fi

grep -q "unexpected fields" "$temp_dir/runtime-unexpected.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/package-graph-runtime.json" "$temp_dir/runtime-v2-short-hash.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

registry_package = next(
    package
    for package in payload["packages"]
    if str(package.get("id", "")).startswith("registry:")
)
registry_package["lockfile"]["hash"] = "1" * 16

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --runtime-snapshot "$temp_dir/runtime-v2-short-hash.json" >"$temp_dir/runtime-v2-short-hash.out" 2>"$temp_dir/runtime-v2-short-hash.err"; then
  echo "expected runtime lockfile v2 with a legacy short hash to fail" >&2
  exit 1
fi

grep -Fq "must match '^[0-9a-f]{64}$'" "$temp_dir/runtime-v2-short-hash.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/package-graph-runtime.json" "$temp_dir" <<'PY'
import copy
import json
import sys
from pathlib import Path

snapshot_path, output_dir = map(Path, sys.argv[1:])
payload = json.loads(snapshot_path.read_text(encoding="utf-8"))
registry_package = next(
    package
    for package in payload["packages"]
    if str(package.get("id", "")).startswith("registry:")
)
registry_edge = next(
    dependency
    for package in payload["packages"]
    for dependency in package["dependencies"]
    if dependency.get("source_kind") == "registry"
)

for field in ("source", "trust", "materialization"):
    mutated = copy.deepcopy(payload)
    target = next(
        package
        for package in mutated["packages"]
        if package.get("id") == registry_package["id"]
    )
    target.pop(field)
    (output_dir / f"runtime-package-missing-{field}.json").write_text(
        json.dumps(mutated, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

for field in ("index_sha256", "verification_sha256"):
    mutated = copy.deepcopy(payload)
    target = next(
        package
        for package in mutated["packages"]
        if package.get("id") == registry_package["id"]
    )
    target["trust"].pop(field)
    (output_dir / f"runtime-trust-missing-{field}.json").write_text(
        json.dumps(mutated, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

missing_version = copy.deepcopy(payload)
next(
    package
    for package in missing_version["packages"]
    if package.get("id") == registry_package["id"]
)["lockfile"].pop("version")
(output_dir / "runtime-registry-lock-missing-version.json").write_text(
    json.dumps(missing_version, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)

v1_lock = copy.deepcopy(payload)
v1_target = next(
    package
    for package in v1_lock["packages"]
    if package.get("id") == registry_package["id"]
)
v1_target["lockfile"]["version"] = 1
v1_target["lockfile"]["hash"] = "1" * 16
(output_dir / "runtime-registry-lock-v1.json").write_text(
    json.dumps(v1_lock, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)

for field in ("package_id", "source_kind", "requested", "reason"):
    mutated = copy.deepcopy(payload)
    target = next(
        dependency
        for package in mutated["packages"]
        for dependency in package["dependencies"]
        if dependency.get("package_id") == registry_edge["package_id"]
    )
    target.pop(field)
    (output_dir / f"runtime-edge-missing-{field}.json").write_text(
        json.dumps(mutated, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
PY

for field in source trust materialization; do
  name="runtime-package-missing-$field"
  if python3 "$script" --runtime-snapshot "$temp_dir/$name.json" >"$temp_dir/$name.out" 2>"$temp_dir/$name.err"; then
    echo "expected runtime registry package without $field to fail" >&2
    exit 1
  fi
  grep -Fq "missing required fields: $field" "$temp_dir/$name.err"
done

for field in index_sha256 verification_sha256; do
  name="runtime-trust-missing-$field"
  if python3 "$script" --runtime-snapshot "$temp_dir/$name.json" >"$temp_dir/$name.out" 2>"$temp_dir/$name.err"; then
    echo "expected runtime registry trust without $field to fail" >&2
    exit 1
  fi
  grep -Fq "missing required fields: $field" "$temp_dir/$name.err"
done

if python3 "$script" --runtime-snapshot "$temp_dir/runtime-registry-lock-missing-version.json" >"$temp_dir/runtime-registry-lock-missing-version.out" 2>"$temp_dir/runtime-registry-lock-missing-version.err"; then
  echo "expected runtime registry package without lock version to fail" >&2
  exit 1
fi
grep -Fq "missing required fields: version" "$temp_dir/runtime-registry-lock-missing-version.err"

if python3 "$script" --runtime-snapshot "$temp_dir/runtime-registry-lock-v1.json" >"$temp_dir/runtime-registry-lock-v1.out" 2>"$temp_dir/runtime-registry-lock-v1.err"; then
  echo "expected runtime registry package with lock version 1 to fail" >&2
  exit 1
fi
grep -Eq "must equal 2|must match '\\^\\[0-9a-f\\]\\{64\\}\\$'" "$temp_dir/runtime-registry-lock-v1.err"

for field in package_id source_kind requested reason; do
  name="runtime-edge-missing-$field"
  if python3 "$script" --runtime-snapshot "$temp_dir/$name.json" >"$temp_dir/$name.out" 2>"$temp_dir/$name.err"; then
    echo "expected runtime registry edge without $field to fail" >&2
    exit 1
  fi
  grep -Fq "missing required fields: $field" "$temp_dir/$name.err"
done

python3 - "$repo_root/stage1/compiler-contracts/snapshots/package-graph.json" "$temp_dir/cargo-derived.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["outputs"]["packages"][0]["source"] = "Cargo.toml"

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/cargo-derived.json" >"$temp_dir/cargo.out" 2>"$temp_dir/cargo.err"; then
  echo "expected Cargo-derived package graph output to fail" >&2
  exit 1
fi

grep -q "Cargo-derived" "$temp_dir/cargo.err"

python3 - "$repo_root/stage1/compiler-contracts/snapshots/package-graph.json" "$temp_dir/stale-lockfile.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

payload["outputs"]["lockfile_integrity"]["packages"][1]["version"] = "9.9.9"

with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

if python3 "$script" --snapshot "$temp_dir/stale-lockfile.json" >"$temp_dir/stale.out" 2>"$temp_dir/stale.err"; then
  echo "expected stale lockfile integrity fixture to fail" >&2
  exit 1
fi

grep -q "lockfile_integrity packages" "$temp_dir/stale.err"

python3 - \
  "$repo_root/stage1/package-resolver/fixtures/manifest-registry.json" \
  "$repo_root/stage1/package-resolver/fixtures/lockfile-v2.json" \
  "$repo_root/stage1/package-resolver/fixtures/resolution-v1.json" \
  "$temp_dir" <<'PY'
import copy
import json
import sys
from pathlib import Path

manifest_path, lock_path, resolution_path, output_dir = map(Path, sys.argv[1:])
output_dir.mkdir(parents=True, exist_ok=True)

with manifest_path.open(encoding="utf-8") as handle:
    manifest = json.load(handle)
with lock_path.open(encoding="utf-8") as handle:
    lockfile = json.load(handle)
with resolution_path.open(encoding="utf-8") as handle:
    resolution = json.load(handle)


def write(name, value):
    with (output_dir / name).open("w", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2, sort_keys=True)
        handle.write("\n")


registry_name = copy.deepcopy(manifest)
registry_name["registry"]["name"] = "other"
registry_name["dependencies"]["core"]["registry"] = "other"
write("resolver-manifest-registry-name.json", registry_name)

registry_source = copy.deepcopy(lockfile)
registry_source["registry"][0]["source"] = "file:///registry/other-index.json"
write("resolver-lock-registry-source.json", registry_source)

root_drift = copy.deepcopy(lockfile)
root_drift["roots"] = [
    next(
        package["id"]
        for package in root_drift["package"]
        if package["source"].startswith("registry:")
    )
]
write("resolver-lock-root.json", root_drift)

package_id = copy.deepcopy(lockfile)
registry_package = next(
    package for package in package_id["package"] if package["source"].startswith("registry:")
)
old_id = registry_package["id"]
registry_package["id"] = "registry:fixture/axiom/core@1.2.4"
for edge in package_id["edge"]:
    if edge["to"] == old_id:
        edge["to"] = registry_package["id"]
write("resolver-lock-package-id.json", package_id)

edge_drift = copy.deepcopy(lockfile)
next(edge for edge in edge_drift["edge"] if edge["alias"] == "core")["requested"] = "^1.2.1"
write("resolver-lock-edge.json", edge_drift)

expectation_digest = copy.deepcopy(lockfile)
expectation_digest["registry"][0]["expectation_sha256"] = "not-a-digest"
write("resolver-lock-expectation.json", expectation_digest)

source_identity = copy.deepcopy(resolution)


def replace_source(value):
    if isinstance(value, dict):
        if {"registry", "source", "namespace", "name"}.issubset(value):
            value["source"] = "registry:other-source"
        for nested in value.values():
            replace_source(nested)
    elif isinstance(value, list):
        for nested in value:
            replace_source(nested)


replace_source(source_identity)
write("resolver-resolution-source.json", source_identity)
PY

expect_resolver_failure() {
  local name="$1"
  local expected="$2"
  shift 2
  if python3 "$script" "$@" >"$temp_dir/$name.out" 2>"$temp_dir/$name.err"; then
    echo "expected resolver fixture drift case $name to fail" >&2
    exit 1
  fi
  grep -Fq "$expected" "$temp_dir/$name.err"
}

expect_resolver_failure \
  resolver-manifest-registry-name \
  "lockfile registry names must exactly match the manifest registry name" \
  --manifest-fixture "$temp_dir/resolver-manifest-registry-name.json"
expect_resolver_failure \
  resolver-lock-registry-source \
  "lockfile registry source must exactly match the manifest registry index" \
  --lockfile-v2-fixture "$temp_dir/resolver-lock-registry-source.json"
expect_resolver_failure \
  resolver-lock-root \
  "lockfile roots must contain the canonical manifest root package identity" \
  --lockfile-v2-fixture "$temp_dir/resolver-lock-root.json"
expect_resolver_failure \
  resolver-lock-package-id \
  "resolved package registry:fixture/axiom/core@1.2.3 must exist in lockfile packages" \
  --lockfile-v2-fixture "$temp_dir/resolver-lock-package-id.json"
expect_resolver_failure \
  resolver-lock-edge \
  "resolution and lockfile dependency edges must match exactly" \
  --lockfile-v2-fixture "$temp_dir/resolver-lock-edge.json"
expect_resolver_failure \
  resolver-lock-expectation \
  "expectation_sha256" \
  --lockfile-v2-fixture "$temp_dir/resolver-lock-expectation.json"
expect_resolver_failure \
  resolver-resolution-source \
  "resolved package source must match the authenticated lock registry source identity" \
  --resolution-fixture "$temp_dir/resolver-resolution-source.json"

echo "check-package-graph-boundary regression cases passed"
