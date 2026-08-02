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
assert vectors["stale-snapshot-replay"]["expected"] == {
    "decision": "rejected",
    "primary_reason_code": "ROLLBACK_DETECTED",
    "reason_codes": [
        "ROLLBACK_DETECTED",
        "METADATA_REPLAYED",
        "OFFLINE_LOCK_MISMATCH",
    ],
}
assert vectors["duplicate-target-path-resigned"]["expected"] == {
    "decision": "rejected",
    "primary_reason_code": "DUPLICATE_TARGET_PATH",
    "reason_codes": [
        "DUPLICATE_TARGET_PATH",
        "OFFLINE_LOCK_MISMATCH",
    ],
}
assert vectors["duplicate-package-coordinate-resigned"]["expected"] == {
    "decision": "rejected",
    "primary_reason_code": "DUPLICATE_PACKAGE_COORDINATE",
    "reason_codes": [
        "DUPLICATE_PACKAGE_COORDINATE",
        "OFFLINE_LOCK_MISMATCH",
    ],
}
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

python3 - "$fixture" <<'PY'
import copy
import json
import pathlib
import sys

from scripts.ci.json_schema_v1 import validate_draft_2020_12

root = pathlib.Path.cwd()
with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle)

schema_files = {
    "package_signature": "axiom-package-signature-v1.schema.json",
    "trust_roots": "axiom-trust-roots-v1.schema.json",
    "registry_index": "axiom-registry-index-v2.schema.json",
    "verification_expectation": "axiom-package-verification-expectation-v1.schema.json",
    "verification": "axiom-package-verification-v1.schema.json",
}
schemas = {}
for section, filename in schema_files.items():
    with (root / "stage1" / "schemas" / filename).open(encoding="utf-8") as handle:
        schemas[section] = json.load(handle)
    assert contract[section]["contract_status"] == "contract_only"
    validate_draft_2020_12(contract[section], schemas[section])
    implemented = copy.deepcopy(contract[section])
    implemented["contract_status"] = "implemented"
    validate_draft_2020_12(implemented, schemas[section])


def at(value, *path):
    for segment in path:
        value = value[segment]
    return value


package_schema = schemas["package_signature"]
assert at(package_schema, "properties", "signatures", "maxItems") == 16
assert at(package_schema, "properties", "package", "properties", "namespace", "maxLength") == 256
assert at(package_schema, "properties", "package", "properties", "target_path", "maxLength") == 4096
assert at(package_schema, "properties", "registry", "properties", "source_identity", "maxLength") == 2048
assert "publication floors" in at(
    package_schema,
    "properties",
    "index",
    "description",
)
predicate_type_schema = at(
    package_schema,
    "$defs",
    "provenance",
    "properties",
    "statement",
    "properties",
    "value",
    "properties",
    "predicateType",
)
assert predicate_type_schema["maxLength"] == 2048
assert predicate_type_schema["pattern"] == r"^[A-Za-z][A-Za-z0-9+.-]*:[^\s]+$"
assert at(
    package_schema,
    "$defs",
    "provenance",
    "properties",
    "statement",
    "properties",
    "value",
    "properties",
    "subject",
    "maxItems",
) == 1024

roots_schema = schemas["trust_roots"]
for root_name in ["trusted_root", "candidate_root"]:
    signed = at(roots_schema, "properties", root_name, "properties", "signed", "properties")
    assert signed["keys"]["maxItems"] == 128
    assert signed["roles"]["maxItems"] == 64
    assert signed["namespace_grants"]["maxItems"] == 2048
    assert signed["roles"]["items"]["properties"]["threshold"]["maximum"] == 16
    assert at(
        roots_schema,
        "properties",
        root_name,
        "properties",
        "signatures",
        "maxItems",
    ) == 16
for field in [
    "candidate_signatures_by_old_root",
    "candidate_signatures_by_new_root",
]:
    assert at(
        roots_schema,
        "properties",
        "transition",
        "properties",
        field,
        "maxItems",
    ) == 16

index_schema = schemas["registry_index"]
assert at(index_schema, "properties", "signed", "properties", "releases", "maxItems") == 1024
assert at(index_schema, "properties", "signatures", "maxItems") == 16

expectation_schema = schemas["verification_expectation"]
required_signers_schema = at(expectation_schema, "properties", "required_signers", "properties")
assert required_signers_schema["required_key_ids"]["maxItems"] == 16
assert required_signers_schema["package_threshold"]["maximum"] == 16
assert required_signers_schema["index_threshold"]["maximum"] == 16
offline_lock_schema = at(expectation_schema, "properties", "offline_lock", "properties")
assert "Exact generation" in offline_lock_schema["index_generation"]["description"]
assert "Exact sequence" in offline_lock_schema["index_sequence"]["description"]
assert at(
    expectation_schema,
    "properties",
    "trusted_state",
    "properties",
    "seen_snapshots",
    "maxItems",
) == 10000

verification_schema = schemas["verification"]
assert at(verification_schema, "properties", "signers", "oneOf", 0, "maxItems") == 16
assert at(
    verification_schema,
    "properties",
    "trust",
    "oneOf",
    0,
    "properties",
    "package_threshold",
    "maximum",
) == 16

production_expectation = copy.deepcopy(contract["verification_expectation"])
production_expectation["contract_status"] = "implemented"
del production_expectation["expected"]
validate_draft_2020_12(
    production_expectation,
    schemas["verification_expectation"],
)

implemented_trusted = copy.deepcopy(contract["verification"])
implemented_trusted["contract_status"] = "implemented"
validate_draft_2020_12(implemented_trusted, schemas["verification"])

observed_non_slsa = copy.deepcopy(contract["package_signature"])
observed_non_slsa["provenance"]["statement"]["value"][
    "predicateType"
] = "https://example.test/other"
validate_draft_2020_12(observed_non_slsa, schemas["package_signature"])

rejected_partial = {
    "schema_version": "axiom.package_verification.v1",
    "contract": "package.verification",
    "contract_status": "implemented",
    "decision": "rejected",
    "primary_reason_code": "OFFLINE_INPUT_MISSING",
    "reason_codes": ["OFFLINE_INPUT_MISSING"],
    "observed": {"registry_identity": "axiom-registry-production"},
    "signers": [],
    "archive": None,
    "manifest_digest": None,
    "provenance": None,
    "trust": {
        "package_threshold": 0,
        "package_valid_signers": 0,
        "index_threshold": 0,
        "index_valid_signers": 0,
    },
}
validate_draft_2020_12(rejected_partial, schemas["verification"])

rejected_unavailable = copy.deepcopy(rejected_partial)
rejected_unavailable["observed"] = {
    "registry_identity": None,
    "source_identity": None,
    "namespace": None,
    "name": None,
    "version": None,
    "target_path": None,
    "publisher_identity": None,
}
rejected_unavailable["archive"] = {}
rejected_unavailable["manifest_digest"] = {}
rejected_unavailable["provenance"] = {}
rejected_unavailable["trust"] = {
    "root_version": None,
    "root_sequence": None,
    "root_transition_from": None,
    "index_generation": 0,
    "index_sequence": 0,
    "package_threshold": 0,
    "package_valid_signers": 0,
    "index_threshold": None,
    "index_valid_signers": 0,
    "offline_mode": None,
    "network_fallback": None,
    "consistent_snapshot": None,
}
validate_draft_2020_12(rejected_unavailable, schemas["verification"])


def assert_invalid(value, message):
    try:
        validate_draft_2020_12(value, schemas["verification"])
    except ValueError:
        return
    raise AssertionError(message)


trusted_missing = copy.deepcopy(implemented_trusted)
del trusted_missing["archive"]
assert_invalid(trusted_missing, "trusted results must carry complete evidence")

trusted_zero_signers = copy.deepcopy(implemented_trusted)
trusted_zero_signers["signers"] = []
assert_invalid(trusted_zero_signers, "trusted results must carry a nonzero signer set")

trusted_zero_counts = copy.deepcopy(implemented_trusted)
trusted_zero_counts["trust"]["package_threshold"] = 0
trusted_zero_counts["trust"]["package_valid_signers"] = 0
assert_invalid(
    trusted_zero_counts,
    "trusted results must carry nonzero threshold and signer counts",
)

rejected_unknown = copy.deepcopy(rejected_partial)
rejected_unknown["observed"]["legacy_hmac"] = "forbidden"
assert_invalid(rejected_unknown, "rejected partial evidence must stay closed")

rejected_non_slsa = copy.deepcopy(implemented_trusted)
rejected_non_slsa["decision"] = "rejected"
rejected_non_slsa["primary_reason_code"] = "PROVENANCE_PREDICATE_MISMATCH"
rejected_non_slsa["reason_codes"] = ["PROVENANCE_PREDICATE_MISMATCH"]
rejected_non_slsa["provenance"]["statement"]["value"][
    "predicateType"
] = "https://example.test/other"
validate_draft_2020_12(rejected_non_slsa, schemas["verification"])

trusted_non_slsa = copy.deepcopy(implemented_trusted)
trusted_non_slsa["provenance"]["statement"]["value"][
    "predicateType"
] = "https://example.test/other"
assert_invalid(
    trusted_non_slsa,
    "trusted results must retain the expected SLSA v1 predicate",
)

relative_predicate = copy.deepcopy(observed_non_slsa)
relative_predicate["provenance"]["statement"]["value"][
    "predicateType"
] = "not-an-absolute-uri"
try:
    validate_draft_2020_12(relative_predicate, schemas["package_signature"])
except ValueError:
    pass
else:
    raise AssertionError("package predicateType must be an absolute URI")
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

unsigned_transition_time="$tmp_dir/unsigned-transition-time.json"
mutate "$unsigned_transition_time" \
  'payload["trust_roots"]["transition"]["transition_time"] = "9999-12-31T23:59:59Z"'
python3 "$script" --contract "$unsigned_transition_time" >/dev/null

overlong_namespace="$tmp_dir/overlong-namespace.json"
mutate "$overlong_namespace" \
  'payload["package_signature"]["package"]["namespace"] = "n" * 257'
expect_failure "$overlong_namespace" "length at most 256"

too_many_signatures="$tmp_dir/too-many-signatures.json"
mutate "$too_many_signatures" \
  'payload["package_signature"]["signatures"] = payload["package_signature"]["signatures"] * 9'
expect_failure "$too_many_signatures" "at most 16 items"

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

bounded_text_schema = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "additionalProperties": False,
    "required": ["value"],
    "properties": {
        "value": {
            "type": "string",
            "maxLength": 2,
        }
    },
}
validate_draft_2020_12({"value": "é🙂"}, bounded_text_schema)
try:
    validate_draft_2020_12({"value": "é🙂x"}, bounded_text_schema)
except ValueError as error:
    assert "length at most 2" in str(error)
else:
    raise AssertionError("maxLength was not enforced by Unicode code point length")
PY

echo "package trust Ed25519 contract and negative vectors passed"
