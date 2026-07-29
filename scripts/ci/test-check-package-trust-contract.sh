#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

script="scripts/ci/check-package-trust-contract.py"
fixture="stage1/package-trust/contract/package-trust.json"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

valid_out="$tmp_dir/valid.json"
python3 "$script" --contract "$fixture" --json >"$valid_out"
python3 - "$valid_out" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
assert payload == {
    "algorithm": "ed25519",
    "archive_digest": "sha-256",
    "fixture": "stage1/package-trust/contract/package-trust.json",
    "ok": True,
    "reason_codes": 44,
    "schema": "axiom.package_trust_contract.v1",
    "status": "contract_only",
    "vectors": 63,
}
PY

python3 - "$fixture" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
assert "signature" not in payload["package_signature"]
assert len(payload["package_signature"]["signatures"]) == 2
assert payload["verification_expectation"]["required_signers"]["package_threshold"] == 2
assert len(payload["registry_index"]["signatures"]) == 2
assert payload["verification_expectation"]["required_signers"]["index_threshold"] == 2
trusted_state = payload["verification_expectation"]["trusted_state"]
assert set(trusted_state["trusted_root_anchor"]) == {
    "root_version",
    "root_sequence",
    "root_transcript_sha256",
}
assert any(
    item["generation"] == payload["registry_index"]["signed"]["generation"]
    and item["sequence"] == payload["registry_index"]["signed"]["sequence"]
    and item["snapshot_id"]
    == payload["registry_index"]["signed"]["consistent_snapshot"]["snapshot_id"]
    and item["index_transcript_sha256"]
    == payload["registry_index"]["transcript"]["sha256"]
    for item in trusted_state["seen_snapshots"]
)
predicate = payload["package_signature"]["provenance"]["statement"]["value"]["predicate"]
assert set(predicate) == {"buildDefinition", "runDetails"}
assert "build_definition_sha256" not in predicate
assert set(predicate["buildDefinition"]) == {
    "buildType",
    "externalParameters",
    "internalParameters",
    "resolvedDependencies",
}
assert set(predicate["runDetails"]) == {"builder", "metadata", "byproducts"}
vector_ids = {
    vector["id"]
    for vector in payload["positive_vectors"] + payload["negative_vectors"]
}
assert {
    "exact-authenticated-snapshot-repeat",
    "mixed-publisher-threshold",
    "bootstrap-anchor-digest",
    "self-signed-attacker-root",
    "root-sequence-rollback-resigned",
    "stale-snapshot-replay",
    "snapshot-id-rebound",
    "supersedes-cycle-resigned",
    "duplicate-target-path-resigned",
    "duplicate-package-coordinate-resigned",
    "slsa-build-definition-missing",
    "slsa-builder-id-missing",
} <= vector_ids
vectors = {vector["id"]: vector for vector in payload["negative_vectors"]}
attacker = vectors["self-signed-attacker-root"]
assert attacker["expected"]["reason_codes"] == ["ROOT_BOOTSTRAP_MISMATCH"]
assert {
    mutation["path"] for mutation in attacker["mutations"]
} == {
    "/trust_roots/trusted_root",
    "/trust_roots/transition/candidate_signatures_by_old_root",
}
for vector_id, vector in vectors.items():
    if not (
        vector_id.endswith("-resigned")
        or "attacker" in vector_id
        or vector_id in {"stale-snapshot-replay", "snapshot-id-rebound"}
    ):
        continue
    paths = {mutation["path"] for mutation in vector["mutations"]}
    reasons = set(vector["expected"]["reason_codes"])
    if any(path.startswith("/trust_roots/") for path in paths):
        assert not reasons & {
            "ROOT_DIGEST_MISMATCH",
            "ROOT_SIGNATURE_INVALID",
            "ROOT_THRESHOLD_NOT_MET",
        }
    if any(path.startswith("/registry_index") for path in paths):
        assert not reasons & {
            "INDEX_DIGEST_MISMATCH",
            "INDEX_SIGNATURE_INVALID",
            "INDEX_THRESHOLD_NOT_MET",
        }
PY

mutate() {
  local output="$1"
  local expression="$2"
  cp "$fixture" "$output"
  python3 - "$output" "$expression" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
with path.open(encoding="utf-8") as handle:
    payload = json.load(handle)
exec(sys.argv[2], {"payload": payload})
with path.open("w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY
}

expect_failure() {
  local fixture_path="$1"
  local expected="$2"
  local output="$tmp_dir/$(basename "$fixture_path").out"
  if python3 "$script" --contract "$fixture_path" >"$output" 2>&1; then
    echo "expected package trust checker to reject $(basename "$fixture_path")" >&2
    exit 1
  fi
  grep -Fq "$expected" "$output"
}

bad_transcript="$tmp_dir/bad-transcript.json"
mutate "$bad_transcript" \
  'payload["package_signature"]["transcript"]["bytes_hex"] = "ff" + payload["package_signature"]["transcript"]["bytes_hex"][2:]'
expect_failure "$bad_transcript" "canonical expectation mismatch"

bad_public_key="$tmp_dir/bad-public-key.json"
mutate "$bad_public_key" \
  'payload["trust_roots"]["candidate_root"]["signed"]["keys"][0]["key_material"]["public_key"] = "00"'
expect_failure "$bad_public_key" "does not match required pattern"

hmac_regression="$tmp_dir/hmac-regression.json"
mutate "$hmac_regression" \
  'payload["package_signature"]["scheme"]["algorithm"] = "hmac-sha256"'
expect_failure "$hmac_regression" "must equal 'ed25519'"

missing_required_key_ids="$tmp_dir/missing-required-key-ids.json"
mutate "$missing_required_key_ids" \
  'del payload["verification_expectation"]["required_signers"]["required_key_ids"]'
expect_failure "$missing_required_key_ids" "missing required properties: required_key_ids"

unexpected_field="$tmp_dir/unexpected-field.json"
mutate "$unexpected_field" \
  'payload["registry_index"]["signed"]["legacy_hmac"] = "forbidden"'
expect_failure "$unexpected_field" "contains unknown properties: legacy_hmac"

bad_vector="$tmp_dir/bad-vector.json"
mutate "$bad_vector" \
  'payload["negative_vectors"][0]["expected"]["reason_codes"] = ["SIGNATURE_INVALID"]'
expect_failure "$bad_vector" "negative vector 'expired-index'"

duplicate_member="$tmp_dir/duplicate-member.json"
python3 - "$fixture" "$duplicate_member" <<'PY'
from pathlib import Path
import sys

source = Path(sys.argv[1]).read_text(encoding="utf-8")
needle = '  "contract": "package.trust",\n'
assert source.count(needle) == 1
Path(sys.argv[2]).write_text(source.replace(needle, needle + needle, 1), encoding="utf-8")
PY
expect_failure "$duplicate_member" "duplicate JSON member 'contract'"

python3 - <<'PY'
from scripts.ci.json_schema_v1 import validate_draft_2020_12

schema = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "additionalProperties": False,
    "required": ["packages"],
    "properties": {
        "packages": {
            "type": "object",
            "minProperties": 1,
            "additionalProperties": {"type": "string"},
        }
    },
}
try:
    validate_draft_2020_12({"packages": {}}, schema)
except ValueError as error:
    assert "at least 1 properties" in str(error)
else:
    raise AssertionError("minProperties was not enforced")
PY

echo "package trust Ed25519 contract and negative vectors passed"
