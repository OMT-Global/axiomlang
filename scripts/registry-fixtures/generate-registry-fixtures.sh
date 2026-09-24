#!/usr/bin/env bash
# Regenerate the checked-in quickstart registry fixtures (issue #1663).
#
# Produces, under ./registry (override with $1):
#   trust-roots.json           Package Trust roots (policy input; generated with
#                              the crate's own canonicalization/signing via the
#                              registry_quickstart_fixtures example)
#   verification-request.json  offline verification expectation (policy input;
#                              pinned to the exact published release and index)
#   packages/<ns>/<name>/<v>/  real `axiomc publish` output (archive, manifest,
#                              provenance, signature)
#   index.json                 real `axiomc registry-index` output (signed v2)
#
# and proves the whole set with `axiomc registry-validate` (exit 0) before
# anything is moved into place.
#
# Deterministic: fixed test-only key seeds, identities, and timestamps; no
# wall-clock or network input. Re-running from the same commit must produce
# byte-identical fixtures (the CI lane asserts exactly that).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="${1:-registry}"

REGISTRY_IDENTITY="axiom-registry-production"
SOURCE_IDENTITY="registry:axiom-production"
PUBLISHER_IDENTITY="https://publishers.example/foundation"
NAMESPACE="axiom"
PACKAGE_DIR="stage1/examples/hello"
GENERATION=1
SEQUENCE=1
ISSUED_AT="2026-09-24T00:00:00Z"
EXPIRES_AT="2036-09-24T00:00:00Z"
SNAPSHOT_ID="axiom-registry-production.1.1"
METADATA_PATH="1/1/index.v2.json"
PREVIOUS_SNAPSHOT_SHA256="0000000000000000000000000000000000000000000000000000000000000000"

AXIOMC=(cargo run --quiet --manifest-path stage1/Cargo.toml -p axiomc)
GENERATOR=(cargo run --quiet --manifest-path stage1/Cargo.toml -p axiomc
  --example registry_quickstart_fixtures)

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
INPUTS="$WORK/inputs"
STAGING="$WORK/registry"
mkdir -p "$INPUTS" "$STAGING"

echo "== step 1/7: deterministic trust inputs (roots, seeds, expectation template)"
"${GENERATOR[@]}" -- inputs \
  --out "$INPUTS" \
  --contract stage1/package-trust/contract/package-trust.json

echo "== step 2/7: provenance statement bound to the exact generated archive"
python3 - "$PACKAGE_DIR" "$NAMESPACE" "$SOURCE_IDENTITY" "$INPUTS/provenance.json" <<'PY'
import hashlib
import json
import os
import sys

project, namespace, source_identity, out = sys.argv[1:5]

name = version = None
with open(os.path.join(project, "axiom.toml"), encoding="utf-8") as handle:
    for line in handle:
        stripped = line.strip()
        if stripped.startswith("name ="):
            name = stripped.split("=", 1)[1].strip().strip('"')
        elif stripped.startswith("version ="):
            version = stripped.split("=", 1)[1].strip().strip('"')
if not name or not version:
    raise SystemExit(f"cannot read [package] name/version from {project}/axiom.toml")

# Mirror render_package_archive (stage1/crates/axiomc/src/registry.rs):
# publishable files are axiom.toml, axiom.lock, and *.ax; directories named
# .git, target, or dist are skipped; files are sorted; each record is
# "--- file <relative> <len> ---\n" + content (+ "\n" when missing).
# If this mirror ever drifts from the compiler, `axiomc publish` below fails
# closed on the provenance subject digest, so a wrong hash cannot slip in.
files = []
for dirpath, dirnames, filenames in os.walk(project):
    dirnames[:] = [d for d in dirnames if d not in (".git", "target", "dist")]
    for filename in filenames:
        if filename in ("axiom.toml", "axiom.lock") or filename.endswith(".ax"):
            files.append(os.path.join(dirpath, filename))
files.sort()
archive = b"AXIOM_PACKAGE_ARCHIVE_V1\n"
for path in files:
    relative = os.path.relpath(path, project).replace(os.sep, "/")
    with open(path, "rb") as handle:
        content = handle.read()
    archive += f"--- file {relative} {len(content)} ---\n".encode() + content
    if not content.endswith(b"\n"):
        archive += b"\n"

target = f"{namespace}/{name}/{version}/package.axp"
statement = {
    "_type": "https://in-toto.io/Statement/v1",
    "subject": [{"name": target, "digest": {"sha256": hashlib.sha256(archive).hexdigest()}}],
    "predicateType": "https://slsa.dev/provenance/v1",
    "predicate": {
        "buildDefinition": {
            "buildType": "axiom:build/package-v1",
            "externalParameters": {},
            "internalParameters": {},
            "resolvedDependencies": [
                {"uri": source_identity, "digest": {"sha256": "11" * 32}}
            ],
        },
        "runDetails": {
            "builder": {
                "id": "axiom:builder/quickstart-fixture",
                "builderDependencies": [],
                "version": {"axiomc": "0.1.0"},
            },
            "metadata": {
                "invocationId": "urn:uuid:00000000-0000-4000-8000-000000001663",
                "startedOn": "2026-09-24T00:00:00Z",
                "finishedOn": "2026-09-24T00:00:01Z",
            },
            "byproducts": [],
        },
    },
}
with open(out, "w", encoding="utf-8") as handle:
    json.dump(statement, handle, indent=2, sort_keys=True)
    handle.write("\n")
print(f"provenance subject: {target}")
PY

echo "== step 3/7: publish the hello example through the real CLI"
"${AXIOMC[@]}" -- publish "$PACKAGE_DIR" \
  --registry-dir "$STAGING/packages" \
  --namespace "$NAMESPACE" \
  --registry-identity "$REGISTRY_IDENTITY" \
  --source-identity "$SOURCE_IDENTITY" \
  --publisher-identity "$PUBLISHER_IDENTITY" \
  --index-generation "$GENERATION" \
  --index-sequence "$SEQUENCE" \
  --provenance "$INPUTS/provenance.json" \
  --trust-roots "$INPUTS/trust-roots.json" \
  --expectation "$INPUTS/expectation-template.json" \
  --signing-key-file "$INPUTS/seeds/package-a.seed" \
  --signing-key-file "$INPUTS/seeds/package-b.seed"

SIGNATURE="$(find "$STAGING/packages" -name 'package.axp.sig' -print -quit)"
test -n "$SIGNATURE"

echo "== step 4/7: pin the expectation to the exact release and index coordinates"
PINNED_INDEX_HASH="$("${GENERATOR[@]}" -- prepin \
  --template "$INPUTS/expectation-template.json" \
  --roots "$INPUTS/trust-roots.json" \
  --signature "$SIGNATURE" \
  --out "$INPUTS/verification-request.json" \
  --registry-identity "$REGISTRY_IDENTITY" \
  --source-identity "$SOURCE_IDENTITY" \
  --generation "$GENERATION" \
  --sequence "$SEQUENCE" \
  --issued-at "$ISSUED_AT" \
  --expires-at "$EXPIRES_AT" \
  --snapshot-id "$SNAPSHOT_ID" \
  --metadata-path "$METADATA_PATH" \
  --previous-snapshot-sha256 "$PREVIOUS_SNAPSHOT_SHA256")"

echo "== step 5/7: build the signed v2 index through the real CLI"
"${AXIOMC[@]}" -- registry-index "$STAGING/packages" \
  --registry-identity "$REGISTRY_IDENTITY" \
  --source-identity "$SOURCE_IDENTITY" \
  --generation "$GENERATION" \
  --sequence "$SEQUENCE" \
  --issued-at "$ISSUED_AT" \
  --expires-at "$EXPIRES_AT" \
  --snapshot-id "$SNAPSHOT_ID" \
  --metadata-path "$METADATA_PATH" \
  --previous-snapshot-sha256 "$PREVIOUS_SNAPSHOT_SHA256" \
  --trust-roots "$INPUTS/trust-roots.json" \
  --expectation "$INPUTS/verification-request.json" \
  --signing-key-file "$INPUTS/seeds/index-a.seed" \
  --signing-key-file "$INPUTS/seeds/index-b.seed" \
  --out "$STAGING/index.json"

echo "== step 6/7: reconstructed index transcript must match the real index"
python3 - "$STAGING/index.json" "$PINNED_INDEX_HASH" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    envelope = json.load(handle)
actual = envelope["transcript"]["sha256"]
if actual != sys.argv[2]:
    raise SystemExit(f"index transcript {actual} != pinned {sys.argv[2]}")
print(f"index transcript sha256: {actual}")
PY

echo "== step 7/7: validate the generated set through the real CLI (exit 0 required)"
"${AXIOMC[@]}" -- registry-validate "$STAGING/index.json" \
  --packages-dir "$STAGING/packages" \
  --trust-roots "$INPUTS/trust-roots.json" \
  --expectation "$INPUTS/verification-request.json"

STAGED_OUT="$WORK/out"
mkdir -p "$STAGED_OUT"
cp "$STAGING/index.json" "$STAGED_OUT/index.json"
cp -R "$STAGING/packages" "$STAGED_OUT/packages"
cp "$INPUTS/trust-roots.json" "$STAGED_OUT/trust-roots.json"
cp "$INPUTS/verification-request.json" "$STAGED_OUT/verification-request.json"
rm -rf "${OUT_DIR:?}"
mkdir -p "$(dirname "$OUT_DIR")"
mv "$STAGED_OUT" "$OUT_DIR"
echo "fixtures written to $OUT_DIR"
