#!/usr/bin/env python3
"""Validate the Axiom Package Trust v1 contract fixture and vectors."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import struct
import sys
import unicodedata
from datetime import datetime, timezone
from functools import lru_cache
from pathlib import Path, PurePosixPath
from typing import Any

from json_schema_v1 import validate_draft_2020_12

SCHEMA_VERSION = "axiom.package_trust_contract.v1"
CONTRACT = "package.trust"
DEFAULT_CONTRACT = Path("stage1/package-trust/contract/package-trust.json")
SCHEMAS = {
    "package_signature": Path("stage1/schemas/axiom-package-signature-v1.schema.json"),
    "trust_roots": Path("stage1/schemas/axiom-trust-roots-v1.schema.json"),
    "registry_index": Path("stage1/schemas/axiom-registry-index-v2.schema.json"),
    "verification_expectation": Path(
        "stage1/schemas/axiom-package-verification-expectation-v1.schema.json"
    ),
    "verification": Path("stage1/schemas/axiom-package-verification-v1.schema.json"),
}
TOP_LEVEL_FIELDS = {
    "schema_version",
    "contract",
    "contract_status",
    "specification",
    *SCHEMAS,
    "positive_vectors",
    "negative_vectors",
}
PACKAGE_DOMAIN = "AXIOM-PACKAGE-TRUST-V1"
ROOT_DOMAIN = "AXIOM-TRUST-ROOT-V1"
INDEX_DOMAIN = "AXIOM-REGISTRY-INDEX-V2"
PACKAGE_FIELDS = [
    "transcript_format_version",
    "signature_algorithm",
    "signature_version",
    "signature_message_mode",
    "archive_digest_algorithm",
    "archive_digest",
    "archive_length",
    "manifest_digest",
    "package_namespace",
    "package_name",
    "package_version",
    "target_path",
    "registry_identity",
    "source_identity",
    "publisher_identity",
    "provenance_statement_digest",
    "provenance_statement_type",
    "provenance_predicate_type",
    "provenance_subject_name",
    "provenance_subject_digest",
    "index_generation",
    "index_sequence",
    "package_signature_threshold",
]
REASON_PRECEDENCE = [
    "OFFLINE_INPUT_MISSING",
    "ROOT_BOOTSTRAP_MISMATCH",
    "METADATA_EXPIRED",
    "ROOT_ROTATION_INVALID",
    "ROOT_DIGEST_MISMATCH",
    "ROOT_SIGNATURE_INVALID",
    "ROOT_THRESHOLD_NOT_MET",
    "ROOT_ROLLBACK",
    "ROLLBACK_DETECTED",
    "METADATA_REPLAYED",
    "VERSION_DOWNGRADE",
    "INDEX_DIGEST_MISMATCH",
    "INDEX_SIGNATURE_INVALID",
    "INDEX_THRESHOLD_NOT_MET",
    "DUPLICATE_RELEASE",
    "DUPLICATE_TARGET_PATH",
    "DUPLICATE_PACKAGE_COORDINATE",
    "TARGET_PATH_INVALID",
    "ARCHIVE_DIGEST_MISMATCH",
    "MANIFEST_DIGEST_MISMATCH",
    "PROVENANCE_STATEMENT_MISMATCH",
    "PROVENANCE_PREDICATE_MISMATCH",
    "PROVENANCE_SUBJECT_MISMATCH",
    "DELEGATION_INVALID",
    "NAMESPACE_GRANT_MISMATCH",
    "DUPLICATE_KEY",
    "KEY_MALFORMED",
    "KEY_ID_MISMATCH",
    "KEY_SUPERSESSION_INVALID",
    "KEY_UNKNOWN",
    "KEY_REVOKED",
    "KEY_RETIRED",
    "KEY_NOT_YET_VALID",
    "SIGNER_PUBLISHER_MISMATCH",
    "PUBLISHER_MISMATCH",
    "NAMESPACE_MISMATCH",
    "PACKAGE_NAME_MISMATCH",
    "PACKAGE_VERSION_MISMATCH",
    "SOURCE_MISMATCH",
    "TARGET_PATH_MISMATCH",
    "SIGNATURE_MALFORMED",
    "SIGNATURE_INVALID",
    "PACKAGE_THRESHOLD_NOT_MET",
    "OFFLINE_LOCK_MISMATCH",
]


class DuplicateJsonMember(ValueError):
    pass


def reject_duplicate_members(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for key, item in pairs:
        if key in value:
            raise DuplicateJsonMember(f"duplicate JSON member {key!r}")
        value[key] = item
    return value


def load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle, object_pairs_hook=reject_duplicate_members)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def sha256(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def canonical_json(value: Any) -> bytes:
    """Encode the NFC, integer-only Axiom canonical JSON subset."""

    def check(node: Any, path: str) -> None:
        if isinstance(node, str):
            require(node == unicodedata.normalize("NFC", node), f"{path} must be NFC")
        elif isinstance(node, list):
            for index, item in enumerate(node):
                check(item, f"{path}[{index}]")
        elif isinstance(node, dict):
            for key, item in node.items():
                require(isinstance(key, str), f"{path} keys must be strings")
                check(key, f"{path} key")
                check(item, f"{path}.{key}")
        else:
            require(
                node is None or isinstance(node, (bool, int)),
                f"{path} contains a non-canonical JSON value",
            )

    check(value, "$")
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def metadata_transcript(domain: str, signed: dict[str, Any]) -> bytes:
    domain_bytes = domain.encode("ascii")
    payload = canonical_json(signed)
    return (
        struct.pack(">H", len(domain_bytes))
        + domain_bytes
        + struct.pack(">Q", len(payload))
        + payload
    )


def package_field_values(
    package: dict[str, Any], package_threshold: int
) -> dict[str, str | int | bytes]:
    provenance = package["provenance"]
    statement = provenance["statement"]
    selected = provenance["selected_subject"]
    return {
        "transcript_format_version": 1,
        "signature_algorithm": package["scheme"]["algorithm"],
        "signature_version": package["scheme"]["version"],
        "signature_message_mode": package["scheme"]["message_mode"],
        "archive_digest_algorithm": package["archive"]["digest"]["algorithm"],
        "archive_digest": bytes.fromhex(package["archive"]["digest"]["value"]),
        "archive_length": package["archive"]["size"],
        "manifest_digest": bytes.fromhex(package["manifest"]["value"]),
        "package_namespace": package["package"]["namespace"],
        "package_name": package["package"]["name"],
        "package_version": package["package"]["version"],
        "target_path": package["package"]["target_path"],
        "registry_identity": package["registry"]["registry_identity"],
        "source_identity": package["registry"]["source_identity"],
        "publisher_identity": package["publisher"]["publisher_identity"],
        "provenance_statement_digest": bytes.fromhex(statement["digest"]["value"]),
        "provenance_statement_type": statement["value"]["_type"],
        "provenance_predicate_type": statement["value"]["predicateType"],
        "provenance_subject_name": selected["name"],
        "provenance_subject_digest": bytes.fromhex(selected["digest"]["sha256"]),
        "index_generation": package["index"]["generation"],
        "index_sequence": package["index"]["sequence"],
        "package_signature_threshold": package_threshold,
    }


def package_transcript(package: dict[str, Any], package_threshold: int) -> bytes:
    values = package_field_values(package, package_threshold)
    domain = PACKAGE_DOMAIN.encode("ascii")
    parts = [struct.pack(">H", len(domain)), domain, struct.pack(">H", len(PACKAGE_FIELDS))]
    for name in PACKAGE_FIELDS:
        label = name.encode("ascii")
        value = values[name]
        if isinstance(value, int):
            encoded = struct.pack(">Q", value)
        elif isinstance(value, str):
            require(value == unicodedata.normalize("NFC", value), f"{name} must be NFC")
            encoded = value.encode("utf-8")
        else:
            encoded = value
        parts.extend(
            [
                struct.pack(">H", len(label)),
                label,
                struct.pack(">Q", len(encoded)),
                encoded,
            ]
        )
    return b"".join(parts)


# Fixture-only RFC 8032 verifier. This validates vectors and is not a production
# Axiom cryptographic API.
Q = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493


def inverse(value: int) -> int:
    return pow(value, Q - 2, Q)


D = (-121665 * inverse(121666)) % Q
I = pow(2, (Q - 1) // 4, Q)
IDENTITY = (0, 1)


def recover_x(y: int, sign: int) -> int:
    xx = (y * y - 1) * inverse(D * y * y + 1) % Q
    x = pow(xx, (Q + 3) // 8, Q)
    if (x * x - xx) % Q:
        x = x * I % Q
    require((x * x - xx) % Q == 0, "invalid Ed25519 point")
    if x & 1 != sign:
        x = Q - x
    return x


def encode_point(point: tuple[int, int]) -> bytes:
    x, y = point
    return (y | ((x & 1) << 255)).to_bytes(32, "little")


def point_add(left: tuple[int, int], right: tuple[int, int]) -> tuple[int, int]:
    x1, y1 = left
    x2, y2 = right
    product = D * x1 * x2 * y1 * y2
    return (
        (x1 * y2 + y1 * x2) * inverse(1 + product) % Q,
        (y1 * y2 + x1 * x2) * inverse(1 - product) % Q,
    )


def scalar_multiply(point: tuple[int, int], scalar: int) -> tuple[int, int]:
    result = IDENTITY
    addend = point
    while scalar:
        if scalar & 1:
            result = point_add(result, addend)
        addend = point_add(addend, addend)
        scalar >>= 1
    return result


BASE = (recover_x(4 * inverse(5) % Q, 0), 4 * inverse(5) % Q)


def decode_point_strict(encoded: bytes) -> tuple[int, int]:
    require(len(encoded) == 32, "Ed25519 points must be 32 bytes")
    raw = int.from_bytes(encoded, "little")
    sign = raw >> 255
    y = raw & ((1 << 255) - 1)
    require(y < Q, "non-canonical Ed25519 point")
    point = (recover_x(y, sign), y)
    require(encode_point(point) == encoded, "non-canonical Ed25519 encoding")
    require(scalar_multiply(point, 8) != IDENTITY, "small-order Ed25519 point")
    require(scalar_multiply(point, L) == IDENTITY, "Ed25519 point outside prime subgroup")
    return point


@lru_cache(maxsize=256)
def public_key_status(public_key_hex: Any) -> str | None:
    try:
        if not isinstance(public_key_hex, str):
            return "KEY_MALFORMED"
        public_key = bytes.fromhex(public_key_hex)
        decode_point_strict(public_key)
        return None
    except (ValueError, OverflowError):
        return "KEY_MALFORMED"


@lru_cache(maxsize=512)
def signature_status(
    public_key_hex: Any, message: bytes, signature_hex: Any
) -> str | None:
    try:
        if not isinstance(public_key_hex, str):
            return "KEY_MALFORMED"
        public_key = bytes.fromhex(public_key_hex)
        public_point = decode_point_strict(public_key)
    except (ValueError, OverflowError):
        return "KEY_MALFORMED"
    try:
        if not isinstance(signature_hex, str):
            return "SIGNATURE_MALFORMED"
        signature = bytes.fromhex(signature_hex)
        if len(signature) != 64:
            return "SIGNATURE_MALFORMED"
        r_encoded, s_encoded = signature[:32], signature[32:]
        scalar = int.from_bytes(s_encoded, "little")
        if scalar >= L:
            return "SIGNATURE_MALFORMED"
        r_point = decode_point_strict(r_encoded)
    except (ValueError, OverflowError):
        return "SIGNATURE_MALFORMED"
    challenge = int.from_bytes(
        hashlib.sha512(r_encoded + public_key + message).digest(), "little"
    ) % L
    if scalar_multiply(BASE, scalar) != point_add(
        r_point, scalar_multiply(public_point, challenge)
    ):
        return "SIGNATURE_INVALID"
    return None


def validate_rfc_8032_reference_vector() -> None:
    public_key = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    signature = (
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155"
        "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    )
    require(
        signature_status(public_key, b"", signature) is None,
        "RFC 8032 section 7.1 empty-message vector failed",
    )


def derived_key_id(key_material: Any) -> str | None:
    try:
        if not isinstance(key_material, dict):
            return None
        return "sha256:" + sha256(canonical_json(key_material))
    except (TypeError, ValueError):
        return None


def key_maps(
    signed_root: dict[str, Any], failures: set[str]
) -> tuple[dict[str, dict[str, Any]], dict[str, str]]:
    keys: dict[str, dict[str, Any]] = {}
    fingerprints: dict[str, str] = {}
    seen_public: set[str] = set()
    for key in signed_root.get("keys", []):
        if not isinstance(key, dict):
            failures.add("KEY_MALFORMED")
            continue
        key_id = key.get("key_id")
        material = key.get("key_material")
        public_key = material.get("public_key") if isinstance(material, dict) else None
        if public_key_status(public_key) is not None:
            failures.add("KEY_MALFORMED")
        derived = derived_key_id(material)
        if not isinstance(key_id, str) or derived != key_id:
            failures.add("KEY_ID_MISMATCH")
        if isinstance(public_key, str):
            if public_key in seen_public:
                failures.add("DUPLICATE_KEY")
            seen_public.add(public_key)
        if isinstance(key_id, str):
            if key_id in keys:
                failures.add("DUPLICATE_KEY")
            keys[key_id] = key
            if derived is not None:
                fingerprints[key_id] = derived
    return keys, fingerprints


def validate_key_supersession(
    keys: dict[str, dict[str, Any]], failures: set[str]
) -> None:
    graph: dict[str, list[str]] = {}
    for key_id, key in keys.items():
        supersedes = key.get("supersedes_key_ids")
        if not isinstance(supersedes, list) or len(supersedes) != len(
            set(supersedes)
        ):
            failures.add("KEY_SUPERSESSION_INVALID")
            continue
        graph[key_id] = [item for item in supersedes if isinstance(item, str)]
        status = key.get("status")
        revocation = key.get("revocation")
        valid_from = key.get("valid_from_sequence")
        if status == "revoked":
            if (
                not isinstance(revocation, dict)
                or not isinstance(valid_from, int)
                or not isinstance(revocation.get("effective_sequence"), int)
                or revocation["effective_sequence"] < valid_from
            ):
                failures.add("KEY_SUPERSESSION_INVALID")
        elif revocation is not None:
            failures.add("KEY_SUPERSESSION_INVALID")
        if supersedes and status != "active":
            failures.add("KEY_SUPERSESSION_INVALID")
        for predecessor_id in graph[key_id]:
            predecessor = keys.get(predecessor_id)
            if (
                predecessor is None
                or predecessor_id == key_id
                or predecessor.get("publisher_identity")
                != key.get("publisher_identity")
                or predecessor.get("status") not in {"retired", "revoked"}
                or not isinstance(valid_from, int)
                or not isinstance(predecessor.get("valid_from_sequence"), int)
                or valid_from <= predecessor["valid_from_sequence"]
            ):
                failures.add("KEY_SUPERSESSION_INVALID")

    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(key_id: str) -> None:
        if key_id in visiting:
            failures.add("KEY_SUPERSESSION_INVALID")
            return
        if key_id in visited:
            return
        visiting.add(key_id)
        for predecessor_id in graph.get(key_id, []):
            if predecessor_id in keys:
                visit(predecessor_id)
        visiting.remove(key_id)
        visited.add(key_id)

    for key_id in graph:
        visit(key_id)


def role_maps(
    signed_root: dict[str, Any], keys: dict[str, dict[str, Any]], failures: set[str]
) -> dict[str, dict[str, Any]]:
    roles: dict[str, dict[str, Any]] = {}
    for role in signed_root.get("roles", []):
        if not isinstance(role, dict) or not isinstance(role.get("role_id"), str):
            failures.add("DELEGATION_INVALID")
            continue
        role_id = role["role_id"]
        if role_id in roles:
            failures.add("DELEGATION_INVALID")
        roles[role_id] = role
        key_ids = role.get("key_ids", [])
        if (
            not isinstance(key_ids, list)
            or len(key_ids) != len(set(key_ids))
            or not isinstance(role.get("threshold"), int)
            or role["threshold"] > len(key_ids)
            or any(key_id not in keys for key_id in key_ids)
        ):
            failures.add("DELEGATION_INVALID")
    for role_id, role in roles.items():
        seen: set[str] = set()
        current = role_id
        while current != "root":
            if current in seen or current not in roles:
                failures.add("DELEGATION_INVALID")
                break
            seen.add(current)
            parent = roles[current].get("delegated_by")
            if not isinstance(parent, str):
                failures.add("DELEGATION_INVALID")
                break
            current = parent
        if role_id == "root" and role.get("delegated_by") is not None:
            failures.add("DELEGATION_INVALID")
    return roles


def parse_time(value: Any) -> datetime | None:
    try:
        if not isinstance(value, str):
            return None
        return datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(
            tzinfo=timezone.utc
        )
    except ValueError:
        return None


def semver(value: Any) -> tuple[int, int, int, tuple[tuple[int, int | str], ...]] | None:
    if not isinstance(value, str):
        return None
    core_and_pre = value.split("+", 1)[0]
    core, separator, prerelease = core_and_pre.partition("-")
    parts = core.split(".")
    if len(parts) != 3 or any(not part.isdigit() for part in parts):
        return None
    if any(len(part) > 1 and part.startswith("0") for part in parts):
        return None
    pre: list[tuple[int, int | str]] = []
    if separator:
        identifiers = prerelease.split(".")
        if not identifiers or any(not item for item in identifiers):
            return None
        for item in identifiers:
            if not all(char.isalnum() or char == "-" for char in item):
                return None
            if item.isdigit():
                if len(item) > 1 and item.startswith("0"):
                    return None
                pre.append((0, int(item)))
            else:
                pre.append((1, item))
    return (int(parts[0]), int(parts[1]), int(parts[2]), tuple(pre))


def compare_semver(left: Any, right: Any) -> int:
    parsed_left = semver(left)
    parsed_right = semver(right)
    if parsed_left is None or parsed_right is None:
        return -1
    if parsed_left[:3] != parsed_right[:3]:
        return (parsed_left[:3] > parsed_right[:3]) - (
            parsed_left[:3] < parsed_right[:3]
        )
    left_pre, right_pre = parsed_left[3], parsed_right[3]
    if not left_pre and not right_pre:
        return 0
    if not left_pre:
        return 1
    if not right_pre:
        return -1
    for left_id, right_id in zip(left_pre, right_pre):
        if left_id == right_id:
            continue
        if left_id[0] != right_id[0]:
            return -1 if left_id[0] == 0 else 1
        return (left_id[1] > right_id[1]) - (left_id[1] < right_id[1])
    return (len(left_pre) > len(right_pre)) - (len(left_pre) < len(right_pre))


def safe_target_path(path: Any) -> bool:
    if not isinstance(path, str) or path != unicodedata.normalize("NFC", path):
        return False
    if not path or path.startswith("/") or "\\" in path or "\x00" in path:
        return False
    parts = path.split("/")
    if any(part in {"", ".", ".."} for part in parts):
        return False
    return str(PurePosixPath(*parts)) == path


def valid_type_uri(value: Any) -> bool:
    return isinstance(value, str) and ":" in value and not any(
        char.isspace() for char in value
    )


def valid_slsa_resource(value: Any) -> bool:
    if not isinstance(value, dict) or set(value) != {"uri", "digest"}:
        return False
    digest = value.get("digest")
    return (
        valid_type_uri(value.get("uri"))
        and isinstance(digest, dict)
        and bool(digest)
        and all(
            isinstance(algorithm, str)
            and bool(algorithm)
            and isinstance(encoded, str)
            and len(encoded) == 64
            and all(char in "0123456789abcdef" for char in encoded)
            for algorithm, encoded in digest.items()
        )
    )


def valid_slsa_predicate(value: Any) -> bool:
    if not isinstance(value, dict) or set(value) != {
        "buildDefinition",
        "runDetails",
    }:
        return False
    build_definition = value.get("buildDefinition")
    run_details = value.get("runDetails")
    if (
        not isinstance(build_definition, dict)
        or set(build_definition)
        != {
            "buildType",
            "externalParameters",
            "internalParameters",
            "resolvedDependencies",
        }
        or not valid_type_uri(build_definition.get("buildType"))
        or not isinstance(build_definition.get("externalParameters"), dict)
        or not isinstance(build_definition.get("internalParameters"), dict)
        or not isinstance(build_definition.get("resolvedDependencies"), list)
        or not build_definition["resolvedDependencies"]
        or not all(
            valid_slsa_resource(resource)
            for resource in build_definition["resolvedDependencies"]
        )
    ):
        return False
    if not isinstance(run_details, dict) or set(run_details) != {
        "builder",
        "metadata",
        "byproducts",
    }:
        return False
    builder = run_details.get("builder")
    metadata = run_details.get("metadata")
    byproducts = run_details.get("byproducts")
    if (
        not isinstance(builder, dict)
        or set(builder) != {"id", "builderDependencies", "version"}
        or not valid_type_uri(builder.get("id"))
        or not isinstance(builder.get("builderDependencies"), list)
        or not all(
            valid_slsa_resource(resource)
            for resource in builder["builderDependencies"]
        )
        or not isinstance(builder.get("version"), dict)
        or not builder["version"]
        or not all(
            isinstance(name, str)
            and bool(name)
            and isinstance(version, str)
            and bool(version)
            for name, version in builder["version"].items()
        )
        or not isinstance(byproducts, list)
        or not all(valid_slsa_resource(resource) for resource in byproducts)
    ):
        return False
    if not isinstance(metadata, dict) or set(metadata) != {
        "invocationId",
        "startedOn",
        "finishedOn",
    }:
        return False
    started = parse_time(metadata.get("startedOn"))
    finished = parse_time(metadata.get("finishedOn"))
    return (
        isinstance(metadata.get("invocationId"), str)
        and bool(metadata["invocationId"])
        and started is not None
        and finished is not None
        and started <= finished
    )


def statement_failures(provenance: Any, archive: Any, target_path: Any) -> set[str]:
    failures: set[str] = set()
    if not isinstance(provenance, dict):
        return {
            "PROVENANCE_STATEMENT_MISMATCH",
            "PROVENANCE_SUBJECT_MISMATCH",
        }
    statement = provenance.get("statement")
    selected = provenance.get("selected_subject")
    if not isinstance(statement, dict) or not isinstance(statement.get("value"), dict):
        failures.add("PROVENANCE_STATEMENT_MISMATCH")
        return failures
    value = statement["value"]
    try:
        encoded = canonical_json(value)
    except ValueError:
        failures.add("PROVENANCE_STATEMENT_MISMATCH")
        return failures
    digest = statement.get("digest", {}).get("value")
    if (
        statement.get("canonical_bytes_hex") != encoded.hex()
        or digest != sha256(encoded)
        or value.get("_type") != "https://in-toto.io/Statement/v1"
    ):
        failures.add("PROVENANCE_STATEMENT_MISMATCH")
    if (
        value.get("predicateType") != "https://slsa.dev/provenance/v1"
        or not valid_slsa_predicate(value.get("predicate"))
    ):
        failures.add("PROVENANCE_PREDICATE_MISMATCH")
    subjects = value.get("subject")
    if (
        not isinstance(selected, dict)
        or not isinstance(subjects, list)
        or selected not in subjects
        or selected.get("name") != target_path
        or selected.get("digest", {}).get("sha256")
        != archive.get("digest", {}).get("value")
    ):
        failures.add("PROVENANCE_SUBJECT_MISMATCH")
    return failures


def key_eligibility(
    key: dict[str, Any], sequence: int, verification_time: datetime
) -> str | None:
    if key.get("valid_from_sequence", sequence + 1) > sequence:
        return "KEY_NOT_YET_VALID"
    if key.get("status") == "retired":
        return "KEY_RETIRED"
    if key.get("status") == "revoked":
        revocation = key.get("revocation")
        if not isinstance(revocation, dict):
            return "KEY_REVOKED"
        effective_time = parse_time(revocation.get("effective_time"))
        if revocation.get("effective_sequence", sequence + 1) <= sequence or (
            effective_time is not None and effective_time <= verification_time
        ):
            return "KEY_REVOKED"
    return None


def signature_evidence(
    signatures: Any,
    role: dict[str, Any] | None,
    keys: dict[str, dict[str, Any]],
    message: bytes,
    sequence: int,
    verification_time: datetime,
    failures: set[str],
    *,
    context: str,
    required_key_ids: set[str] | None = None,
    expected_publisher: str | None = None,
    publisher_grant_authorized: bool = True,
) -> tuple[set[str], int]:
    valid_key_ids: set[str] = set()
    valid_fingerprints: set[str] = set()
    if not isinstance(signatures, list) or role is None:
        failures.add(f"{context}_THRESHOLD_NOT_MET")
        return valid_key_ids, 0
    for signature in signatures:
        if not isinstance(signature, dict):
            failures.add(
                "SIGNATURE_MALFORMED"
                if context == "PACKAGE"
                else f"{context}_SIGNATURE_INVALID"
            )
            continue
        key_id = signature.get("key_id")
        key = keys.get(key_id)
        if key is None:
            if context == "PACKAGE":
                failures.add("KEY_UNKNOWN")
            else:
                failures.add(f"{context}_SIGNATURE_INVALID")
            continue
        if key_id not in role.get("key_ids", []):
            if context == "PACKAGE":
                failures.add("DELEGATION_INVALID")
            else:
                failures.add(f"{context}_SIGNATURE_INVALID")
            continue
        if context == "PACKAGE" and (
            not publisher_grant_authorized
            or key.get("publisher_identity") != expected_publisher
        ):
            failures.add("SIGNER_PUBLISHER_MISMATCH")
            continue
        eligibility = key_eligibility(key, sequence, verification_time)
        if eligibility is not None:
            if context == "PACKAGE":
                failures.add(eligibility)
            else:
                failures.add(f"{context}_SIGNATURE_INVALID")
            continue
        public_key = key.get("key_material", {}).get("public_key")
        status = signature_status(public_key, message, signature.get("value"))
        if status is not None:
            if context == "PACKAGE":
                failures.add(status)
            else:
                failures.add(f"{context}_SIGNATURE_INVALID")
            continue
        fingerprint = derived_key_id(key.get("key_material"))
        if fingerprint is None:
            if context == "PACKAGE":
                failures.add("KEY_MALFORMED")
            else:
                failures.add(f"{context}_SIGNATURE_INVALID")
            continue
        valid_key_ids.add(key_id)
        valid_fingerprints.add(fingerprint)
    threshold = role.get("threshold", 1)
    if len(valid_fingerprints) < threshold:
        failures.add(f"{context}_THRESHOLD_NOT_MET")
    if required_key_ids is not None and not required_key_ids <= valid_key_ids:
        failures.add(f"{context}_THRESHOLD_NOT_MET")
    return valid_key_ids, len(valid_fingerprints)


def pointer_get(document: Any, path: str) -> Any:
    value = document
    for raw in path.split("/")[1:]:
        segment = raw.replace("~1", "/").replace("~0", "~")
        value = value[int(segment)] if isinstance(value, list) else value[segment]
    return value


def apply_mutations(document: dict[str, Any], mutations: list[dict[str, Any]]) -> None:
    for mutation in mutations:
        path = mutation["path"]
        segments = [
            item.replace("~1", "/").replace("~0", "~")
            for item in path.split("/")[1:]
        ]
        require(segments, "mutation must not target document root")
        parent: Any = document
        for segment in segments[:-1]:
            parent = parent[int(segment)] if isinstance(parent, list) else parent[segment]
        final = segments[-1]
        operation = mutation["operation"]
        if operation == "replace":
            if isinstance(parent, list):
                parent[int(final)] = mutation["value"]
            else:
                parent[final] = mutation["value"]
        elif operation == "remove":
            if isinstance(parent, list):
                del parent[int(final)]
            else:
                del parent[final]
        elif operation == "append_copy":
            target = pointer_get(document, path)
            require(isinstance(target, list), "append_copy target must be an array")
            target.append(copy.deepcopy(pointer_get(document, mutation["from"])))
        else:
            raise ValueError(f"unsupported mutation operation {operation!r}")


def evaluate(contract: dict[str, Any]) -> dict[str, Any]:
    failures: set[str] = set()
    expectation = contract.get("verification_expectation")
    package = contract.get("package_signature")
    index = contract.get("registry_index")
    roots = contract.get("trust_roots")
    if not all(isinstance(item, dict) for item in [expectation, package, index, roots]):
        failures.add("OFFLINE_INPUT_MISSING")
        return {
            "schema_version": "axiom.package_verification.v1",
            "contract": "package.verification",
            "contract_status": "contract_only",
            "decision": "rejected",
            "primary_reason_code": "OFFLINE_INPUT_MISSING",
            "reason_codes": ["OFFLINE_INPUT_MISSING"],
            "observed": {},
            "signers": [],
            "archive": None,
            "manifest_digest": None,
            "provenance": None,
            "trust": {},
        }
    assert isinstance(expectation, dict)
    assert isinstance(package, dict)
    assert isinstance(index, dict)
    assert isinstance(roots, dict)
    verification_time = parse_time(expectation.get("verification_time")) or datetime.max.replace(
        tzinfo=timezone.utc
    )
    request = expectation.get("request", {})
    required_signers = expectation.get("required_signers", {})
    trusted_state = expectation.get("trusted_state", {})
    offline = expectation.get("offline_lock", {})

    trusted_root = roots.get("trusted_root", {})
    candidate_root = roots.get("candidate_root", {})
    transition = roots.get("transition", {})
    old_signed = trusted_root.get("signed", {})
    candidate_signed = candidate_root.get("signed", {})
    old_keys, _ = key_maps(old_signed, failures)
    candidate_keys, fingerprints = key_maps(candidate_signed, failures)
    validate_key_supersession(old_keys, failures)
    validate_key_supersession(candidate_keys, failures)
    old_roles = role_maps(old_signed, old_keys, failures)
    candidate_roles = role_maps(candidate_signed, candidate_keys, failures)

    try:
        old_raw = metadata_transcript(ROOT_DOMAIN, old_signed)
        candidate_raw = metadata_transcript(ROOT_DOMAIN, candidate_signed)
    except (TypeError, ValueError):
        old_raw = b""
        candidate_raw = b""
        failures.add("ROOT_DIGEST_MISMATCH")
    for envelope, raw in [(trusted_root, old_raw), (candidate_root, candidate_raw)]:
        transcript = envelope.get("transcript", {})
        if transcript.get("bytes_hex") != raw.hex() or transcript.get("sha256") != sha256(raw):
            failures.add("ROOT_DIGEST_MISMATCH")

    old_version = old_signed.get("root_version")
    old_sequence = old_signed.get("sequence")
    candidate_version = candidate_signed.get("root_version")
    candidate_sequence = candidate_signed.get("sequence")
    bootstrap_anchor = trusted_state.get("trusted_root_anchor")
    if bootstrap_anchor != {
        "root_version": old_version,
        "root_sequence": old_sequence,
        "root_transcript_sha256": sha256(old_raw),
    }:
        failures.add("ROOT_BOOTSTRAP_MISMATCH")
    old_expiry = parse_time(old_signed.get("expires_at"))
    candidate_issued = parse_time(candidate_signed.get("issued_at"))
    candidate_expiry = parse_time(candidate_signed.get("expires_at"))
    if (
        not isinstance(old_version, int)
        or not isinstance(old_sequence, int)
        or not isinstance(candidate_version, int)
        or not isinstance(candidate_sequence, int)
        or candidate_version != old_version + 1
        or candidate_sequence <= old_sequence
        or transition.get("from_version") != old_version
        or transition.get("to_version") != candidate_version
        or old_expiry is None
        or candidate_issued is None
        or candidate_expiry is None
        or candidate_issued >= old_expiry
        or candidate_issued >= candidate_expiry
    ):
        failures.add("ROOT_ROTATION_INVALID")
    if old_expiry is not None and candidate_issued is not None and old_expiry <= candidate_issued:
        failures.add("METADATA_EXPIRED")
    if candidate_expiry is not None and candidate_expiry <= verification_time:
        failures.add("METADATA_EXPIRED")

    old_root_role = old_roles.get("root")
    new_root_role = candidate_roles.get("root")
    signature_evidence(
        trusted_root.get("signatures"),
        old_root_role,
        old_keys,
        old_raw,
        old_signed.get("sequence", 0),
        verification_time,
        failures,
        context="ROOT",
    )
    old_valid, old_count = signature_evidence(
        transition.get("candidate_signatures_by_old_root"),
        old_root_role,
        old_keys,
        candidate_raw,
        candidate_signed.get("sequence", 0),
        verification_time,
        failures,
        context="ROOT",
    )
    new_valid, new_count = signature_evidence(
        transition.get("candidate_signatures_by_new_root"),
        new_root_role,
        candidate_keys,
        candidate_raw,
        candidate_signed.get("sequence", 0),
        verification_time,
        failures,
        context="ROOT",
    )
    candidate_valid, _ = signature_evidence(
        candidate_root.get("signatures"),
        new_root_role,
        candidate_keys,
        candidate_raw,
        candidate_signed.get("sequence", 0),
        verification_time,
        failures,
        context="ROOT",
    )
    if new_valid != candidate_valid or not old_valid or not new_valid:
        failures.add("ROOT_THRESHOLD_NOT_MET")

    if candidate_version is not None and candidate_version < max(
        trusted_state.get("highest_root_version", candidate_version),
        offline.get("root_version", candidate_version),
    ):
        failures.add("ROOT_ROLLBACK")
    if candidate_sequence is not None and candidate_sequence < max(
        trusted_state.get("highest_root_sequence", candidate_sequence),
        offline.get("root_sequence", candidate_sequence),
    ):
        failures.add("ROOT_ROLLBACK")

    index_signed = index.get("signed", {})
    try:
        index_raw = metadata_transcript(INDEX_DOMAIN, index_signed)
    except (TypeError, ValueError):
        index_raw = b""
        failures.add("INDEX_DIGEST_MISMATCH")
    index_transcript = index.get("transcript", {})
    index_transcript_matches = (
        index_transcript.get("bytes_hex") == index_raw.hex()
        and index_transcript.get("sha256") == sha256(index_raw)
    )
    if not index_transcript_matches:
        failures.add("INDEX_DIGEST_MISMATCH")
    index_expiry = parse_time(index_signed.get("expires_at"))
    if index_expiry is None or index_expiry <= verification_time:
        failures.add("METADATA_EXPIRED")
    generation = index_signed.get("generation", 0)
    sequence = index_signed.get("sequence", 0)
    index_role = candidate_roles.get(required_signers.get("index_role_id"))
    index_valid, index_valid_count = signature_evidence(
        index.get("signatures"),
        index_role,
        candidate_keys,
        index_raw,
        sequence,
        verification_time,
        failures,
        context="INDEX",
    )
    if index_role is not None and index_role.get("threshold") != required_signers.get(
        "index_threshold"
    ):
        failures.add("INDEX_THRESHOLD_NOT_MET")
    index_authenticated = (
        index_transcript_matches
        and index_role is not None
        and index_role.get("threshold") == required_signers.get("index_threshold")
        and index_valid_count >= index_role.get("threshold", 1)
    )
    if index_authenticated:
        highest_generation = trusted_state.get(
            "highest_index_generation", generation
        )
        highest_sequence = trusted_state.get("highest_index_sequence", sequence)
        if generation < highest_generation or sequence < highest_sequence:
            failures.add("ROLLBACK_DETECTED")
        seen_snapshots = trusted_state.get("seen_snapshots")
        if not isinstance(seen_snapshots, list):
            failures.add("OFFLINE_INPUT_MISSING")
            seen_snapshots = []
        snapshot_state = {
            "generation": generation,
            "sequence": sequence,
            "snapshot_id": index_signed.get("consistent_snapshot", {}).get(
                "snapshot_id"
            ),
            "index_transcript_sha256": sha256(index_raw),
        }
        exact_repeat = snapshot_state in seen_snapshots
        rebound = any(
            isinstance(seen, dict)
            and seen != snapshot_state
            and (
                (
                    seen.get("generation") == generation
                    and seen.get("sequence") == sequence
                )
                or seen.get("snapshot_id") == snapshot_state["snapshot_id"]
                or seen.get("index_transcript_sha256")
                == snapshot_state["index_transcript_sha256"]
            )
            for seen in seen_snapshots
        )
        highest_position_seen = any(
            isinstance(seen, dict)
            and seen.get("generation") == highest_generation
            and seen.get("sequence") == highest_sequence
            for seen in seen_snapshots
        )
        if not highest_position_seen:
            failures.add("OFFLINE_INPUT_MISSING")
        if rebound or (
            generation == highest_generation
            and sequence == highest_sequence
            and highest_position_seen
            and not exact_repeat
        ):
            failures.add("METADATA_REPLAYED")

    releases = index_signed.get("releases", [])
    release_tuples: list[tuple[Any, ...]] = []
    release_coordinates: list[tuple[Any, ...]] = []
    release_target_paths: list[Any] = []
    for item in releases if isinstance(releases, list) else []:
        if isinstance(item, dict):
            coordinate = (
                item.get("registry_identity"),
                item.get("source_identity"),
                item.get("namespace"),
                item.get("name"),
                item.get("version"),
            )
            release_coordinates.append(coordinate)
            release_target_paths.append(item.get("target_path"))
            release_tuples.append((*coordinate, item.get("target_path")))
    if len(release_tuples) != len(set(release_tuples)):
        failures.add("DUPLICATE_RELEASE")
    if len(release_target_paths) != len(set(release_target_paths)):
        failures.add("DUPLICATE_TARGET_PATH")
    if len(release_coordinates) != len(set(release_coordinates)):
        failures.add("DUPLICATE_PACKAGE_COORDINATE")
    selected_release = next(
        (
            item
            for item in releases
            if isinstance(item, dict)
            and item.get("registry_identity") == request.get("registry_identity")
            and item.get("source_identity") == request.get("source_identity")
            and item.get("namespace") == request.get("namespace")
            and item.get("name") == request.get("name")
            and item.get("version") == request.get("version")
            and item.get("target_path") == request.get("target_path")
        ),
        None,
    )
    if selected_release is None:
        failures.add("OFFLINE_INPUT_MISSING")

    package_path = package.get("package", {}).get("target_path")
    if not safe_target_path(package_path):
        failures.add("TARGET_PATH_INVALID")
    if compare_semver(
        package.get("package", {}).get("version"),
        trusted_state.get("minimum_package_version"),
    ) < 0:
        failures.add("VERSION_DOWNGRADE")

    grants = candidate_signed.get("namespace_grants", [])
    exact_grant = any(
        isinstance(grant, dict)
        and grant.get("publisher_identity") == request.get("publisher_identity")
        and grant.get("namespace") == request.get("namespace")
        and request.get("name") in grant.get("package_names", [])
        and request.get("registry_identity") in grant.get("registry_identities", [])
        and request.get("source_identity") in grant.get("source_identities", [])
        and grant.get("role_id") == required_signers.get("package_role_id")
        for grant in grants
    )
    if not exact_grant:
        failures.add("NAMESPACE_GRANT_MISMATCH")

    package_threshold = required_signers.get("package_threshold", 1)
    try:
        package_raw = package_transcript(package, package_threshold)
    except (TypeError, ValueError, KeyError, struct.error):
        package_raw = b""
        failures.add("SIGNATURE_INVALID")
    package_transcript_value = package.get("transcript", {})
    if (
        package_transcript_value.get("field_order") != PACKAGE_FIELDS
        or package_transcript_value.get("bytes_hex") != package_raw.hex()
        or package_transcript_value.get("sha256") != sha256(package_raw)
    ):
        failures.add("SIGNATURE_INVALID")
    package_role = candidate_roles.get(required_signers.get("package_role_id"))
    package_valid, package_valid_count = signature_evidence(
        package.get("signatures"),
        package_role,
        candidate_keys,
        package_raw,
        sequence,
        verification_time,
        failures,
        context="PACKAGE",
        required_key_ids=set(required_signers.get("required_key_ids", [])),
        expected_publisher=request.get("publisher_identity"),
        publisher_grant_authorized=exact_grant,
    )
    if package_role is not None and package_role.get("threshold") != package_threshold:
        failures.add("PACKAGE_THRESHOLD_NOT_MET")
    package_authenticated = (
        package_transcript_value.get("field_order") == PACKAGE_FIELDS
        and package_transcript_value.get("bytes_hex") == package_raw.hex()
        and package_transcript_value.get("sha256") == sha256(package_raw)
        and package_role is not None
        and package_role.get("threshold") == package_threshold
        and package_valid_count >= package_threshold
        and set(required_signers.get("required_key_ids", [])) <= package_valid
    )
    package_index = package.get("index", {})
    package_generation = package_index.get("generation")
    package_sequence = package_index.get("sequence")
    # These signed package coordinates are independent publication floors, not
    # an exact binding to the current index. Only authenticated metadata may
    # establish that the current index predates either floor.
    if (
        index_authenticated
        and package_authenticated
        and isinstance(package_generation, int)
        and isinstance(package_sequence, int)
        and (
            package_generation > generation
            or package_sequence > sequence
        )
    ):
        failures.add("METADATA_REPLAYED")

    package_archive = {
        "length": package.get("archive", {}).get("size"),
        "digest": package.get("archive", {}).get("digest"),
    }
    if package_archive != request.get("archive"):
        failures.add("ARCHIVE_DIGEST_MISMATCH")
    if package.get("manifest") != request.get("manifest"):
        failures.add("MANIFEST_DIGEST_MISMATCH")
    package_provenance = package.get("provenance")
    failures.update(statement_failures(package_provenance, package_archive, package_path))
    request_provenance = request.get("provenance", {})
    if (
        package_provenance.get("statement", {}).get("digest")
        != request_provenance.get("statement", {}).get("digest")
    ):
        failures.add("PROVENANCE_STATEMENT_MISMATCH")
    if (
        package_provenance.get("statement", {}).get("value", {}).get("predicateType")
        != request_provenance.get("statement", {}).get("value", {}).get("predicateType")
    ):
        failures.add("PROVENANCE_PREDICATE_MISMATCH")
    if package_provenance.get("selected_subject") != request_provenance.get(
        "selected_subject"
    ):
        failures.add("PROVENANCE_SUBJECT_MISMATCH")

    if package.get("publisher", {}).get("publisher_identity") != request.get(
        "publisher_identity"
    ):
        failures.add("PUBLISHER_MISMATCH")
    if package.get("package", {}).get("namespace") != request.get("namespace"):
        failures.add("NAMESPACE_MISMATCH")
    if package.get("package", {}).get("name") != request.get("name"):
        failures.add("PACKAGE_NAME_MISMATCH")
    if package.get("package", {}).get("version") != request.get("version"):
        failures.add("PACKAGE_VERSION_MISMATCH")
    if (
        package.get("registry", {}).get("registry_identity")
        != request.get("registry_identity")
        or package.get("registry", {}).get("source_identity")
        != request.get("source_identity")
    ):
        failures.add("SOURCE_MISMATCH")
    if package_path != request.get("target_path"):
        failures.add("TARGET_PATH_MISMATCH")

    if selected_release is not None:
        if selected_release.get("archive") != request.get("archive"):
            failures.add("ARCHIVE_DIGEST_MISMATCH")
        if selected_release.get("manifest") != request.get("manifest"):
            failures.add("MANIFEST_DIGEST_MISMATCH")
        selected_provenance = selected_release.get("provenance", {})
        if (
            selected_provenance.get("statement", {}).get("digest")
            != request_provenance.get("statement", {}).get("digest")
        ):
            failures.add("PROVENANCE_STATEMENT_MISMATCH")
        if (
            selected_provenance.get("statement", {})
            .get("value", {})
            .get("predicateType")
            != request_provenance.get("statement", {})
            .get("value", {})
            .get("predicateType")
        ):
            failures.add("PROVENANCE_PREDICATE_MISMATCH")
        if selected_provenance.get("selected_subject") != request_provenance.get(
            "selected_subject"
        ):
            failures.add("PROVENANCE_SUBJECT_MISMATCH")
        if selected_release.get("publisher_identity") != request.get(
            "publisher_identity"
        ):
            failures.add("PUBLISHER_MISMATCH")

    try:
        package_signature_hash = sha256(canonical_json(package))
    except ValueError:
        package_signature_hash = ""
    observed_lock_release = (
        {
            "registry_identity": selected_release.get("registry_identity"),
            "source_identity": selected_release.get("source_identity"),
            "namespace": selected_release.get("namespace"),
            "name": selected_release.get("name"),
            "version": selected_release.get("version"),
            "target_path": selected_release.get("target_path"),
            "publisher_identity": selected_release.get("publisher_identity"),
            "archive": selected_release.get("archive"),
            "manifest": selected_release.get("manifest"),
            "provenance_statement_sha256": selected_release.get("provenance", {})
            .get("statement", {})
            .get("digest", {})
            .get("value"),
            "provenance_predicate_type": selected_release.get("provenance", {})
            .get("statement", {})
            .get("value", {})
            .get("predicateType"),
            "provenance_subject": selected_release.get("provenance", {}).get(
                "selected_subject"
            ),
            "package_signature_sha256": selected_release.get(
                "package_signature_sha256"
            ),
        }
        if selected_release is not None
        else None
    )
    if (
        offline.get("network_fallback") is not False
        or offline.get("root_version") != candidate_version
        or offline.get("root_sequence") != candidate_signed.get("sequence")
        or offline.get("root_transcript_sha256")
        != candidate_root.get("transcript", {}).get("sha256")
        or offline.get("index_generation") != generation
        or offline.get("index_sequence") != sequence
        or offline.get("index_transcript_sha256")
        != index.get("transcript", {}).get("sha256")
        or offline.get("release") != observed_lock_release
        or (
            selected_release is not None
            and selected_release.get("package_signature_sha256")
            != package_signature_hash
        )
    ):
        failures.add("OFFLINE_LOCK_MISMATCH")

    precedence = expectation.get("reason_precedence", REASON_PRECEDENCE)
    if precedence != REASON_PRECEDENCE:
        failures.add("OFFLINE_INPUT_MISSING")
        precedence = REASON_PRECEDENCE
    ordered = [code for code in precedence if code in failures]
    if not ordered:
        ordered = ["OK"]
    valid_signers = []
    for key_id in sorted(package_valid):
        key = candidate_keys.get(key_id)
        if key is None:
            continue
        valid_signers.append(
            {
                "key_id": key_id,
                "public_key_fingerprint": fingerprints.get(key_id, key_id),
                "publisher_identity": key.get("publisher_identity"),
                "role_id": required_signers.get("package_role_id"),
                "algorithm": "ed25519",
                "status": key.get("status"),
            }
        )
    evidence = {
        "observed": {
            "registry_identity": package.get("registry", {}).get("registry_identity"),
            "source_identity": package.get("registry", {}).get("source_identity"),
            "namespace": package.get("package", {}).get("namespace"),
            "name": package.get("package", {}).get("name"),
            "version": package.get("package", {}).get("version"),
            "target_path": package_path,
            "publisher_identity": package.get("publisher", {}).get(
                "publisher_identity"
            ),
        },
        "signers": valid_signers,
        "archive": package_archive,
        "manifest_digest": package.get("manifest"),
        "provenance": package_provenance,
        "trust": {
            "root_version": candidate_version,
            "root_sequence": candidate_signed.get("sequence"),
            "root_transition_from": old_version,
            "index_generation": generation,
            "index_sequence": sequence,
            "package_threshold": package_threshold,
            "package_valid_signers": package_valid_count,
            "index_threshold": required_signers.get("index_threshold"),
            "index_valid_signers": index_valid_count,
            "offline_mode": offline.get("mode"),
            "network_fallback": offline.get("network_fallback"),
            "consistent_snapshot": index_signed.get("consistent_snapshot", {}).get(
                "enabled"
            ),
        },
    }
    return {
        "schema_version": "axiom.package_verification.v1",
        "contract": "package.verification",
        "contract_status": "contract_only",
        "decision": "trusted" if ordered == ["OK"] else "rejected",
        "primary_reason_code": ordered[0],
        "reason_codes": ordered,
        **evidence,
    }


def expected_verification(evaluation: dict[str, Any]) -> dict[str, Any]:
    return copy.deepcopy(evaluation)


def validate_contract(contract: dict[str, Any]) -> None:
    require(set(contract) == TOP_LEVEL_FIELDS, "package trust top-level fields mismatch")
    require(contract["schema_version"] == SCHEMA_VERSION, "contract schema_version mismatch")
    require(contract["contract"] == CONTRACT, "contract name mismatch")
    require(contract["contract_status"] == "contract_only", "contract must remain contract_only")
    require(
        "No production" in contract["specification"]["implementation_claim"],
        "fixture must not claim a production verifier",
    )
    schemas = {name: load_json(path) for name, path in SCHEMAS.items()}
    for name, schema in schemas.items():
        validate_draft_2020_12(contract[name], schema)
    validate_rfc_8032_reference_vector()
    evaluation = evaluate(contract)
    expectation = contract["verification_expectation"]["expected"]
    require(
        {
            "decision": evaluation["decision"],
            "primary_reason_code": evaluation["primary_reason_code"],
            "reason_codes": evaluation["reason_codes"],
        }
        == expectation,
        f"canonical expectation mismatch: {evaluation}",
    )
    require(
        contract["verification"] == expected_verification(evaluation),
        "verification result does not match every computed evidence field",
    )

    transition_time_changed = copy.deepcopy(contract)
    transition_time_changed["trust_roots"]["transition"]["transition_time"] = (
        "9999-12-31T23:59:59Z"
    )
    require(
        evaluate(transition_time_changed) == evaluation,
        "unsigned transition_time must not affect a trusted decision or evidence",
    )
    rejected_transition_time_changed = copy.deepcopy(contract)
    rejected_transition_time_changed["package_signature"]["signatures"][0]["value"] = (
        "not-lowercase-hex"
    )
    rejected_before = evaluate(rejected_transition_time_changed)
    rejected_transition_time_changed["trust_roots"]["transition"]["transition_time"] = (
        "1900-01-01T00:00:00Z"
    )
    require(
        evaluate(rejected_transition_time_changed) == rejected_before,
        "unsigned transition_time must not convert a rejection to trust or alter its reasons",
    )

    missing_result = evaluate({})
    require(
        missing_result["primary_reason_code"] == "OFFLINE_INPUT_MISSING",
        "missing required runtime material must map to OFFLINE_INPUT_MISSING",
    )
    validate_draft_2020_12(missing_result, schemas["verification"])

    malformed_cases = [
        (
            "root",
            "/trust_roots/trusted_root/transcript/bytes_hex",
            None,
            "ROOT_DIGEST_MISMATCH",
        ),
        (
            "index",
            "/registry_index/transcript/bytes_hex",
            None,
            "INDEX_DIGEST_MISMATCH",
        ),
        (
            "package signature",
            "/package_signature/signatures/0/value",
            "not-lowercase-hex",
            "SIGNATURE_MALFORMED",
        ),
        (
            "package transcript",
            "/package_signature/transcript/field_order",
            None,
            "SIGNATURE_INVALID",
        ),
    ]
    for label, path, value, expected_reason in malformed_cases:
        malformed = copy.deepcopy(contract)
        apply_mutations(
            malformed,
            [{"operation": "replace", "path": path, "value": value}],
        )
        malformed_result = evaluate(malformed)
        require(
            expected_reason in malformed_result["reason_codes"],
            f"malformed {label} must map to {expected_reason}",
        )
        validate_draft_2020_12(malformed_result, schemas["verification"])

    require(
        contract["package_signature"]["transcript"]["field_order"] == PACKAGE_FIELDS,
        "package signing field order mismatch",
    )
    vector_ids: set[str] = set()
    for vector in contract["positive_vectors"]:
        require(vector["id"] not in vector_ids, f"duplicate vector id {vector['id']!r}")
        vector_ids.add(vector["id"])
        require(
            {
                "decision": evaluation["decision"],
                "primary_reason_code": evaluation["primary_reason_code"],
                "reason_codes": evaluation["reason_codes"],
            }
            == vector["expected"],
            f"positive vector {vector['id']!r} mismatch",
        )
    observed_reasons: set[str] = set()
    for vector in contract["negative_vectors"]:
        require(vector["id"] not in vector_ids, f"duplicate vector id {vector['id']!r}")
        vector_ids.add(vector["id"])
        mutated = copy.deepcopy(contract)
        apply_mutations(mutated, vector["mutations"])
        actual = evaluate(mutated)
        expected = vector["expected"]
        actual_projection = {
            "decision": actual["decision"],
            "primary_reason_code": actual["primary_reason_code"],
            "reason_codes": actual["reason_codes"],
        }
        require(
            actual_projection == expected,
            f"negative vector {vector['id']!r}: expected {expected}, got {actual_projection}",
        )
        vector_id = vector["id"]
        if (
            vector_id.endswith("-resigned")
            or "attacker" in vector_id
            or vector_id in {"stale-snapshot-replay", "snapshot-id-rebound"}
        ):
            mutation_paths = {
                mutation.get("path") for mutation in vector.get("mutations", [])
            }
            if any(
                isinstance(path, str) and path.startswith("/trust_roots/")
                for path in mutation_paths
            ):
                unintended_root_failures = {
                    "ROOT_DIGEST_MISMATCH",
                    "ROOT_SIGNATURE_INVALID",
                    "ROOT_THRESHOLD_NOT_MET",
                } & set(actual["reason_codes"])
                require(
                    not unintended_root_failures,
                    f"semantic vector {vector_id!r} is not root-cryptographically clean: "
                    + ", ".join(sorted(unintended_root_failures)),
                )
            if any(
                isinstance(path, str) and path.startswith("/registry_index")
                for path in mutation_paths
            ):
                unintended_index_failures = {
                    "INDEX_DIGEST_MISMATCH",
                    "INDEX_SIGNATURE_INVALID",
                    "INDEX_THRESHOLD_NOT_MET",
                } & set(actual["reason_codes"])
                require(
                    not unintended_index_failures,
                    f"semantic vector {vector_id!r} is not index-cryptographically clean: "
                    + ", ".join(sorted(unintended_index_failures)),
                )
        if vector_id == "self-signed-attacker-root":
            require(
                actual["reason_codes"] == ["ROOT_BOOTSTRAP_MISMATCH"],
                "self-signed attacker root must fail only the out-of-band bootstrap anchor",
            )
        observed_reasons.update(actual["reason_codes"])
    require(
        set(REASON_PRECEDENCE) <= observed_reasons,
        "negative vectors do not cover stable reasons: "
        + ", ".join(sorted(set(REASON_PRECEDENCE) - observed_reasons)),
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--contract", type=Path, default=DEFAULT_CONTRACT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    try:
        contract = load_json(args.contract)
        require(isinstance(contract, dict), "contract fixture must be a JSON object")
        validate_contract(contract)
    except (
        OSError,
        json.JSONDecodeError,
        DuplicateJsonMember,
        KeyError,
        TypeError,
        ValueError,
    ) as error:
        print(f"package trust contract invalid: {error}", file=sys.stderr)
        return 1
    result = {
        "schema": SCHEMA_VERSION,
        "status": "contract_only",
        "ok": True,
        "algorithm": "ed25519",
        "archive_digest": "sha-256",
        "vectors": len(contract["positive_vectors"]) + len(contract["negative_vectors"]),
        "reason_codes": len(REASON_PRECEDENCE),
        "fixture": str(args.contract),
    }
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(
            "package trust contract-only fixture ok: "
            f"{result['algorithm']} + {result['archive_digest']}; "
            f"{result['vectors']} vectors"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
