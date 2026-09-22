#!/usr/bin/env python3
"""Validate the review-gated Runtime Crypto Provider Policy v1 contract."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import sys
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any
from urllib.parse import urlsplit


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = (
    "stage1/compiler-contracts/schemas/"
    "axiom.runtime_crypto_provider_policy.v1.schema.json"
)
SNAPSHOT = (
    "stage1/compiler-contracts/snapshots/runtime-crypto-provider-policy-v1.json"
)
FIXTURES = (
    "stage1/compiler-contracts/fixtures/runtime-crypto-provider-policy-v1"
)
ACTIVATION_ARTIFACT = (
    "stage1/compiler-contracts/snapshots/"
    "runtime-crypto-provider-policy-v1.activation.json"
)
MAX_CONTRACT_BYTES = 1024 * 1024
READ_CHUNK_BYTES = 64 * 1024
EXPECTED_SCHEMA_SHA256 = "6d95c9421ba16533889412100909af60059d01523c8cef42cf52f055d6ee2b28"

EXPECTED_ALGORITHMS: dict[str, dict[str, Any]] = {
    "aes-128-gcm@1": {
        "kind": "aead",
        "axiom_names": ["Aes128Gcm"],
        "operations": ["open", "seal"],
        "standard": "https://csrc.nist.gov/pubs/sp/800/38/d/final",
        "input_model": "runtime_bytes",
        "public_encoding": "ciphertext_then_tag_bytes",
        "output_bytes": 0,
        "key_bytes": {"allowed": [16], "minimum": 16, "maximum": 16},
        "nonce_bytes": [12],
        "tag_bytes": 16,
        "validation_rules": [
            "reject_non_12_byte_nonce",
            "reject_non_16_byte_key",
            "return_no_plaintext_before_tag_verification",
        ],
    },
    "aes-256-gcm@1": {
        "kind": "aead",
        "axiom_names": ["Aes256Gcm"],
        "operations": ["open", "seal"],
        "standard": "https://csrc.nist.gov/pubs/sp/800/38/d/final",
        "input_model": "runtime_bytes",
        "public_encoding": "ciphertext_then_tag_bytes",
        "output_bytes": 0,
        "key_bytes": {"allowed": [32], "minimum": 32, "maximum": 32},
        "nonce_bytes": [12],
        "tag_bytes": 16,
        "validation_rules": [
            "reject_non_12_byte_nonce",
            "reject_non_32_byte_key",
            "return_no_plaintext_before_tag_verification",
        ],
    },
    "chacha20-poly1305@1": {
        "kind": "aead",
        "axiom_names": ["ChaCha20Poly1305"],
        "operations": ["open", "seal"],
        "standard": "https://www.rfc-editor.org/rfc/rfc8439",
        "input_model": "runtime_bytes",
        "public_encoding": "ciphertext_then_tag_bytes",
        "output_bytes": 0,
        "key_bytes": {"allowed": [32], "minimum": 32, "maximum": 32},
        "nonce_bytes": [12],
        "tag_bytes": 16,
        "validation_rules": [
            "reject_non_12_byte_nonce",
            "reject_non_32_byte_key",
            "return_no_plaintext_before_tag_verification",
        ],
    },
    "ed25519@1": {
        "kind": "signature",
        "axiom_names": [
            "ed25519_keygen",
            "ed25519_sign",
            "ed25519_verify",
        ],
        "operations": ["keygen", "sign", "verify"],
        "standard": "https://www.rfc-editor.org/rfc/rfc8032",
        "input_model": "runtime_bytes",
        "public_encoding": "raw_bytes",
        "output_bytes": 0,
        "key_bytes": {"allowed": [32], "minimum": 32, "maximum": 32},
        "nonce_bytes": [],
        "tag_bytes": 0,
        "validation_rules": [
            "public_key_is_32_bytes",
            "secret_key_is_32_byte_seed_only",
            "signature_is_64_bytes",
            "verification_failure_returns_false",
        ],
    },
    "hmac-sha2-256@1": {
        "kind": "mac",
        "axiom_names": ["hmac_sha256", "verify_sha256"],
        "operations": ["compute", "verify"],
        "standard": "https://csrc.nist.gov/pubs/fips/198-1/final",
        "input_model": "runtime_bytes_or_utf8_text",
        "public_encoding": "lowercase_hex_text",
        "output_bytes": 32,
        "key_bytes": {"allowed": [], "minimum": 14, "maximum": 65536},
        "nonce_bytes": [],
        "tag_bytes": 0,
        "validation_rules": [
            "reject_keys_below_112_bits_outside_published_test_vectors",
            "verification_uses_constant_time_comparison",
        ],
    },
    "hmac-sha2-512@1": {
        "kind": "mac",
        "axiom_names": ["hmac_sha512", "verify_sha512"],
        "operations": ["compute", "verify"],
        "standard": "https://csrc.nist.gov/pubs/fips/198-1/final",
        "input_model": "runtime_bytes_or_utf8_text",
        "public_encoding": "lowercase_hex_text",
        "output_bytes": 64,
        "key_bytes": {"allowed": [], "minimum": 14, "maximum": 65536},
        "nonce_bytes": [],
        "tag_bytes": 0,
        "validation_rules": [
            "reject_keys_below_112_bits_outside_published_test_vectors",
            "verification_uses_constant_time_comparison",
        ],
    },
    "sha2-256@1": {
        "kind": "hash",
        "axiom_names": ["sha256"],
        "operations": ["digest"],
        "standard": "https://csrc.nist.gov/pubs/fips/180-4/upd1/final",
        "input_model": "runtime_bytes_or_utf8_text",
        "public_encoding": "lowercase_hex_text",
        "output_bytes": 32,
        "key_bytes": {"allowed": [], "minimum": 0, "maximum": 0},
        "nonce_bytes": [],
        "tag_bytes": 0,
        "validation_rules": [
            "accept_empty_input",
            "process_runtime_sized_input_in_bounded_chunks",
        ],
    },
}

EXPECTED_OPERATIONS = [
    "aead.open",
    "aead.seal",
    "entropy.fill",
    "hash.digest",
    "mac.compute",
    "mac.verify",
    "signature.keygen",
    "signature.sign",
    "signature.verify",
]

ALGORITHM_OPERATIONS = {
    algorithm_id: {f"{policy['kind']}.{operation}" for operation in policy["operations"]}
    for algorithm_id, policy in EXPECTED_ALGORITHMS.items()
}

INSPECTION_ENTROPY_ALGORITHM = "system-entropy@1"
ENTROPY_PROVIDER_BY_TARGET = {
    "linux-x86_64": "linux-getrandom",
    "macos-arm64": "apple-security-secrandom",
}
OPAQUE_KEY_OPERATIONS = {
    "aead.open",
    "aead.seal",
    "mac.compute",
    "mac.verify",
    "signature.sign",
}
NOT_APPLICABLE_KEY_OPERATIONS = set(EXPECTED_OPERATIONS) - OPAQUE_KEY_OPERATIONS

INSPECTION_INPUT_LENGTH_FIELDS = {
    "aead.open": {"aad", "ciphertext", "key", "nonce"},
    "aead.seal": {"aad", "key", "nonce", "plaintext"},
    "entropy.fill": {"requested"},
    "hash.digest": {"input"},
    "mac.compute": {"input", "key"},
    "mac.verify": {"input", "key", "tag"},
    "signature.keygen": set(),
    "signature.sign": {"message", "secret_key"},
    "signature.verify": {"message", "public_key", "signature"},
}

EXPECTED_VECTOR_SOURCES: dict[str, dict[str, Any]] = {
    "aes-128-gcm/nist-empty-plaintext": {
        "algorithm": "aes-128-gcm@1",
        "source": "https://csrc.nist.gov/projects/cryptographic-algorithm-validation-program/block-ciphers",
        "source_case": "GCM authenticated-encryption validation vectors",
        "required_outcomes": [
            "exact_ciphertext_and_tag",
            "tampered_tag_returns_no_plaintext",
        ],
    },
    "aes-256-gcm/nist-empty-plaintext": {
        "algorithm": "aes-256-gcm@1",
        "source": "https://csrc.nist.gov/projects/cryptographic-algorithm-validation-program/block-ciphers",
        "source_case": "GCM authenticated-encryption validation vectors",
        "required_outcomes": [
            "exact_ciphertext_and_tag",
            "tampered_tag_returns_no_plaintext",
        ],
    },
    "chacha20-poly1305/rfc8439-section-2.8.2": {
        "algorithm": "chacha20-poly1305@1",
        "source": "https://www.rfc-editor.org/rfc/rfc8439#section-2.8.2",
        "source_case": "AEAD_CHACHA20_POLY1305 example and test vector",
        "required_outcomes": [
            "exact_ciphertext_and_tag",
            "tampered_tag_returns_no_plaintext",
        ],
    },
    "ed25519/rfc8032-section-7.1-test-1": {
        "algorithm": "ed25519@1",
        "source": "https://www.rfc-editor.org/rfc/rfc8032#section-7.1",
        "source_case": "Ed25519 empty-message test vector 1",
        "required_outcomes": [
            "exact_public_key_and_signature",
            "changed_message_returns_false",
        ],
    },
    "hmac-sha2-256/nist-cavp": {
        "algorithm": "hmac-sha2-256@1",
        "source": "https://csrc.nist.gov/projects/cryptographic-algorithm-validation-program/message-authentication",
        "source_case": "HMAC-SHA2-256 validation vectors",
        "required_outcomes": ["exact_tag", "changed_message_returns_false"],
    },
    "hmac-sha2-512/nist-cavp": {
        "algorithm": "hmac-sha2-512@1",
        "source": "https://csrc.nist.gov/projects/cryptographic-algorithm-validation-program/message-authentication",
        "source_case": "HMAC-SHA2-512 validation vectors",
        "required_outcomes": ["exact_tag", "changed_message_returns_false"],
    },
    "sha2-256/nist-abc": {
        "algorithm": "sha2-256@1",
        "source": "https://csrc.nist.gov/projects/cryptographic-standards-and-guidelines/example-values",
        "source_case": "SHA-256 abc example",
        "required_outcomes": [
            "exact_digest",
            "runtime_and_static_inputs_match",
        ],
    },
}

EXPECTED_FAILURES = {
    "allocation_failed": ("aead.seal", "none"),
    "authentication_failed": ("aead.open", "none"),
    "capability_denied": ("hash.digest", "none"),
    "entropy_unavailable": ("entropy.fill", "discarded"),
    "invalid_key_length": ("mac.compute", "none"),
    "invalid_nonce_length": ("aead.seal", "none"),
    "malformed_input": ("signature.verify", "false"),
    "provider_failure": ("signature.sign", "none"),
    "provider_unavailable": ("hash.digest", "none"),
    "unsupported_algorithm": ("aead.seal", "none"),
    "unsupported_target": ("entropy.fill", "none"),
    "verification_failed": ("signature.verify", "false"),
}

EXPECTED_FIXTURES: dict[str, tuple[str, str, list[str]]] = {
    "algorithm-vectors.json": (
        "axiom://runtime-crypto-provider-policy/v1/algorithm-vectors",
        "vector_source_catalog",
        [
            "every_approved_algorithm_has_an_authoritative_https_source",
            "catalog_makes_no_executable_vector_claim_without_embedded_material",
        ],
    ),
    "failure-matrix.json": (
        "axiom://runtime-crypto-provider-policy/v1/failure-matrix",
        "failure_catalog",
        [
            "every_stable_failure_code_has_one_closed_catalog_entry",
            "catalog_makes_no_executable_failure_claim",
        ],
    ),
    "inspection-redaction.json": (
        "axiom://runtime-crypto-provider-policy/v1/inspection-redaction",
        "inspection",
        [
            "entropy_and_unkeyed_inspection_profiles_are_representable",
            "inspection_values_and_nested_channels_are_closed",
            "marker_secret_absence_is_regression_tested",
        ],
    ),
    "provider-matrix.json": (
        "axiom://runtime-crypto-provider-policy/v1/provider-matrix",
        "provider_matrix",
        [
            "provider_requirements_are_separate_from_qualification",
            "qualification_remains_false_without_target_evidence",
        ],
    ),
}


class ContractError(ValueError):
    """Raised when the policy or one of its fixtures drifts."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def _path_components(path: os.PathLike[str] | str, *, root: bool) -> tuple[bool, list[str]]:
    raw = os.fspath(path)
    require(isinstance(raw, str), "repository path must be text")
    require(raw != "", "repository path must not be empty")
    require("\x00" not in raw, "repository path must not contain NUL")
    windows = PureWindowsPath(raw)
    require(
        not windows.is_absolute() and not windows.drive,
        f"repository path must not be Windows absolute: {raw!r}",
    )
    absolute = PurePosixPath(raw).is_absolute()
    require(root or not absolute, f"repository path must be relative: {raw!r}")
    if root and raw == "/":
        return True, []
    if root and raw == ".":
        return False, []
    components = raw.split("/")
    if absolute:
        components = components[1:]
    require(all(component != "" for component in components), f"repository path has an empty component: {raw!r}")
    require(all(component not in {".", ".."} for component in components), f"repository path has a forbidden component: {raw!r}")
    return absolute, components


def _kind(mode: int) -> str:
    if stat.S_ISLNK(mode):
        return "symlink"
    if stat.S_ISDIR(mode):
        return "directory"
    if stat.S_ISFIFO(mode):
        return "fifo"
    if stat.S_ISSOCK(mode):
        return "socket"
    if stat.S_ISCHR(mode) or stat.S_ISBLK(mode):
        return "device"
    if stat.S_ISREG(mode):
        return "regular file"
    return "nonregular file"


class RepositoryReader:
    """Race-resistant bounded reader for untrusted checkout data."""

    def __init__(self, root: Path | str):
        absolute, components = _path_components(root, root=True)
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_NONBLOCK
        flags |= getattr(os, "O_CLOEXEC", 0)
        anchor = "/" if absolute else "."
        try:
            current = os.open(anchor, flags)
        except OSError as error:
            raise ContractError("unable to open repository root") from error
        try:
            for component in components:
                current = self._descend(current, component, root=True)
        except Exception:
            os.close(current)
            raise
        self._root_fd = current

    def __enter__(self) -> RepositoryReader:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    def close(self) -> None:
        root_fd = getattr(self, "_root_fd", -1)
        if root_fd >= 0:
            os.close(root_fd)
            self._root_fd = -1

    @staticmethod
    def _lstat(directory_fd: int, component: str, label: str) -> os.stat_result:
        try:
            return os.stat(component, dir_fd=directory_fd, follow_symlinks=False)
        except FileNotFoundError as error:
            raise ContractError(f"repository path is missing: {label}") from error
        except OSError as error:
            raise ContractError(f"unable to inspect repository path: {label}") from error

    @classmethod
    def _descend(cls, directory_fd: int, component: str, *, root: bool = False) -> int:
        label = "repository root" if root else component
        metadata = cls._lstat(directory_fd, component, label)
        require(stat.S_ISDIR(metadata.st_mode), f"repository path component is {_kind(metadata.st_mode)}, not directory: {label}")
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_NONBLOCK
        flags |= getattr(os, "O_CLOEXEC", 0)
        try:
            next_fd = os.open(component, flags, dir_fd=directory_fd)
        except OSError as error:
            raise ContractError(f"unable to open repository directory: {label}") from error
        os.close(directory_fd)
        return next_fd

    def _parent(self, relative: os.PathLike[str] | str) -> tuple[int, str, str]:
        _, components = _path_components(relative, root=False)
        require(components, "repository path must name a file")
        directory_fd = os.dup(self._root_fd)
        try:
            for component in components[:-1]:
                directory_fd = self._descend(directory_fd, component)
        except Exception:
            os.close(directory_fd)
            raise
        return directory_fd, components[-1], "/".join(components)

    def exists(self, relative: os.PathLike[str] | str) -> bool:
        try:
            directory_fd, component, _ = self._parent(relative)
        except ContractError as error:
            if error.__cause__ and isinstance(error.__cause__, FileNotFoundError):
                return False
            raise
        try:
            try:
                os.stat(component, dir_fd=directory_fd, follow_symlinks=False)
            except FileNotFoundError:
                return False
            except OSError as error:
                raise ContractError("unable to inspect repository path") from error
            return True
        finally:
            os.close(directory_fd)

    def read_bytes(self, relative: os.PathLike[str] | str) -> bytes:
        directory_fd, component, label = self._parent(relative)
        file_fd = -1
        try:
            metadata = self._lstat(directory_fd, component, label)
            require(
                stat.S_ISREG(metadata.st_mode),
                f"repository path is {_kind(metadata.st_mode)}, not regular file: {label}",
            )
            require(metadata.st_size <= MAX_CONTRACT_BYTES, f"repository file exceeds {MAX_CONTRACT_BYTES} bytes: {label}")
            flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
            flags |= getattr(os, "O_CLOEXEC", 0)
            try:
                file_fd = os.open(component, flags, dir_fd=directory_fd)
            except OSError as error:
                raise ContractError(f"unable to open repository file: {label}") from error
            opened = os.fstat(file_fd)
            require(stat.S_ISREG(opened.st_mode), f"repository path changed to {_kind(opened.st_mode)}: {label}")
            output = bytearray()
            while len(output) <= MAX_CONTRACT_BYTES:
                try:
                    chunk = os.read(
                        file_fd,
                        min(READ_CHUNK_BYTES, MAX_CONTRACT_BYTES + 1 - len(output)),
                    )
                except BlockingIOError as error:
                    raise ContractError(f"repository file read would block: {label}") from error
                except OSError as error:
                    raise ContractError(f"unable to read repository file: {label}") from error
                if not chunk:
                    break
                output.extend(chunk)
            require(len(output) <= MAX_CONTRACT_BYTES, f"repository file exceeds {MAX_CONTRACT_BYTES} bytes: {label}")
            return bytes(output)
        finally:
            if file_fd >= 0:
                os.close(file_fd)
            os.close(directory_fd)

    def read_json(self, relative: os.PathLike[str] | str) -> Any:
        label = os.fspath(relative)
        return decode_json(self.read_bytes(relative), label)


def decode_json(raw: bytes, label: str) -> Any:
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ContractError(f"repository file is not valid UTF-8: {label}") from error
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:
        raise ContractError(
            f"repository file is not valid JSON at line {error.lineno} column {error.colno}: {label}"
        ) from error
    except (RecursionError, ValueError) as error:
        raise ContractError(f"repository file exceeds JSON parser limits: {label}") from error


def json_kind(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        return "object"
    return "unknown"


def json_equal(left: Any, right: Any) -> bool:
    if isinstance(left, bool) or isinstance(right, bool):
        return isinstance(left, bool) and isinstance(right, bool) and left == right
    if isinstance(left, (int, float)) or isinstance(right, (int, float)):
        return (
            isinstance(left, (int, float))
            and not isinstance(left, bool)
            and isinstance(right, (int, float))
            and not isinstance(right, bool)
            and left == right
        )
    if isinstance(left, list) or isinstance(right, list):
        return (
            isinstance(left, list)
            and isinstance(right, list)
            and len(left) == len(right)
            and all(json_equal(a, b) for a, b in zip(left, right))
        )
    if isinstance(left, dict) or isinstance(right, dict):
        return (
            isinstance(left, dict)
            and isinstance(right, dict)
            and set(left) == set(right)
            and all(json_equal(left[key], right[key]) for key in left)
        )
    return type(left) is type(right) and left == right


def validate_schema(value: Any, schema: dict[str, Any]) -> None:
    """Validate the JSON Schema vocabulary used by this checked-in contract."""

    definitions = schema.get("$defs", {})

    def visit(node: Any, rule: dict[str, Any], path: str) -> None:
        if "$ref" in rule:
            prefix = "#/$defs/"
            reference = rule["$ref"]
            require(reference.startswith(prefix), f"{path}: unsupported schema ref")
            name = reference[len(prefix) :]
            require(name in definitions, f"{path}: unknown schema ref {name}")
            visit(node, definitions[name], path)
            return

        if "const" in rule:
            require(json_equal(node, rule["const"]), f"{path}: const mismatch")
        if "enum" in rule:
            require(any(json_equal(node, candidate) for candidate in rule["enum"]), f"{path}: enum mismatch")
        expected_type = rule.get("type")
        if expected_type is not None:
            require(json_kind(node) == expected_type, f"{path}: expected {expected_type}")

        if isinstance(node, dict):
            properties = rule.get("properties", {})
            missing = sorted(set(rule.get("required", [])) - set(node))
            require(not missing, f"{path}: missing fields {', '.join(missing)}")
            if rule.get("additionalProperties") is False:
                unexpected = sorted(set(node) - set(properties))
                require(not unexpected, f"{path}: unknown fields {', '.join(unexpected)}")
            for field, nested in node.items():
                if field in properties:
                    visit(nested, properties[field], f"{path}.{field}")

        if isinstance(node, list):
            require(len(node) >= rule.get("minItems", 0), f"{path}: too few items")
            if rule.get("uniqueItems"):
                canonical = [json.dumps(item, sort_keys=True) for item in node]
                require(len(canonical) == len(set(canonical)), f"{path}: duplicate items")
            if "items" in rule:
                for index, item in enumerate(node):
                    visit(item, rule["items"], f"{path}[{index}]")

        if isinstance(node, str):
            require(len(node) >= rule.get("minLength", 0), f"{path}: string too short")
            if "pattern" in rule:
                require(re.search(rule["pattern"], node) is not None, f"{path}: pattern mismatch")

        if isinstance(node, (int, float)) and not isinstance(node, bool):
            require(node >= rule.get("minimum", node), f"{path}: below minimum")

    visit(value, schema, "$")


def require_sorted_unique(values: list[Any], label: str) -> None:
    require(values == sorted(values), f"{label} must be sorted")
    require(len(values) == len(set(values)), f"{label} must be unique")


def validate_algorithms(snapshot: dict[str, Any]) -> None:
    algorithms = snapshot["algorithms"]
    ids = [algorithm["id"] for algorithm in algorithms]
    require_sorted_unique(ids, "algorithm ids")
    require(set(ids) == set(EXPECTED_ALGORITHMS), "approved algorithm set drift")
    for algorithm in algorithms:
        algorithm_id = algorithm["id"]
        expected = {
            "id": algorithm_id,
            "provider": "openssl-3.5-evp",
            "status": "approved_on_activation",
            **EXPECTED_ALGORITHMS[algorithm_id],
        }
        require(algorithm == expected, f"{algorithm_id}: algorithm policy drift")
        for field in ("axiom_names", "operations", "validation_rules"):
            require_sorted_unique(algorithm[field], f"{algorithm_id}.{field}")


def validate_provider(snapshot: dict[str, Any]) -> None:
    provider = snapshot["algorithm_provider"]
    require(provider["id"] == "openssl-3.5-evp", "algorithm provider id drift")
    require(provider["kind"] == "algorithm", "algorithm provider kind drift")
    require(provider["version_requirement"] == ">=3.5.0,<3.6.0", "provider version range drift")
    require(provider["support_end"] == "2030-04-08", "provider support horizon drift")
    require(provider["targets"] == ["linux-x86_64", "macos-arm64"], "provider targets drift")
    require(provider["load_policy"] == "bundled_pinned_attested", "provider load policy drift")
    require(provider["ambient_host_loading"] is False, "ambient provider loading is forbidden")
    require(provider["provider_name"] == "default", "OpenSSL provider name drift")
    require(provider["fips_claim"] == "none", "the policy must not imply FIPS validation")
    require(
        provider["qualification_requirements"]
        == [
            "artifact_digest",
            "attestation_identity",
            "attestation_subject",
            "executable_equivalence",
            "isolated_config_loading",
            "isolated_provider_loading",
            "openssl_default_provider_selection",
            "provider_version",
            "sbom_digest",
            "signer_identity",
            "target_abi",
        ],
        "provider qualification requirements drift",
    )
    provenance = provider["provenance"]
    require(
        provenance["artifact_digest_required"]
        and provenance["sbom_required"]
        and provenance["version_evidence_required"],
        "provider provenance evidence is incomplete",
    )
    require(
        provenance["source"] == "https://openssl-library.org/source/",
        "provider artifact source drift",
    )
    require(
        provenance["support_policy"]
        == "https://openssl-library.org/post/2025-02-20-openssl-3.5-lts/",
        "provider support policy drift",
    )


def validate_entropy(snapshot: dict[str, Any]) -> None:
    sources = snapshot["entropy_sources"]
    ids = [source["id"] for source in sources]
    require_sorted_unique(ids, "entropy source ids")
    require(ids == ["apple-security-secrandom", "linux-getrandom"], "entropy source set drift")
    by_id = {source["id"]: source for source in sources}
    apple = by_id["apple-security-secrandom"]
    require(apple["target"] == "macos-arm64", "Apple entropy target drift")
    require(apple["interface"] == "SecRandomCopyBytes(kSecRandomDefault)", "Apple entropy interface drift")
    require(
        apple["source"]
        == "https://developer.apple.com/documentation/security/secrandomcopybytes(_:_:_:)",
        "Apple entropy source drift",
    )
    require(apple["maximum_request_bytes"] == 65536, "Apple entropy bound drift")
    require(apple["chunk_bytes"] == 65536, "Apple entropy chunk bound drift")
    require(
        apple["success_rule"] == "status_equals_errSecSuccess_and_full_buffer_written",
        "Apple entropy success rule drift",
    )
    require(apple["retry_rule"] == "none", "Apple entropy retry rule drift")
    linux = by_id["linux-getrandom"]
    require(linux["target"] == "linux-x86_64", "Linux entropy target drift")
    require(linux["interface"] == "getrandom(flags=0)", "Linux entropy interface drift")
    require(
        linux["source"] == "https://man7.org/linux/man-pages/man2/getrandom.2.html",
        "Linux entropy source drift",
    )
    require(linux["maximum_request_bytes"] == 65536, "Linux entropy bound drift")
    require(linux["chunk_bytes"] <= 256, "Linux getrandom chunks must not exceed 256 bytes")
    require(linux["success_rule"] == "loop_until_full_buffer_written", "Linux entropy success rule drift")
    require(linux["retry_rule"] == "retry_eintr_and_partial_reads", "Linux entropy retry rule drift")
    for source in sources:
        require(source["failure_code"] == "entropy_unavailable", f"{source['id']}: failure code drift")
        require(source["fallback"] == "none", f"{source['id']}: entropy fallback is forbidden")
        require(source["source"].startswith("https://"), f"{source['id']}: source must be HTTPS")
    require(
        snapshot["entropy_health"]
        == {
            "health_signal": "provider_api_success",
            "application_statistical_tests": "forbidden",
            "failure_behavior": "discard_buffer_and_return_entropy_unavailable",
            "fallback": "none",
        },
        "entropy health policy drift",
    )


def validate_targets(snapshot: dict[str, Any]) -> None:
    expected = [
        {
            "id": "linux-x86_64",
            "algorithm_provider": "openssl-3.5-evp",
            "entropy_source": "linux-getrandom",
            "semantic_requirement": "equivalent",
            "implementation_status": "compatibility_backend_ambient_loading_only",
            "provider_qualification": "missing",
            "abi_qualification": "missing",
            "executable_equivalence": "missing",
        },
        {
            "id": "macos-arm64",
            "algorithm_provider": "openssl-3.5-evp",
            "entropy_source": "apple-security-secrandom",
            "semantic_requirement": "equivalent",
            "implementation_status": "compatibility_backend_ambient_loading_only",
            "provider_qualification": "missing",
            "abi_qualification": "missing",
            "executable_equivalence": "missing",
        },
    ]
    require(snapshot["targets"] == expected, "supported-target provider matrix drift")


def validate_text_evidence(
    record: dict[str, Any],
    label: str,
    pattern: re.Pattern[str],
) -> bool:
    require(set(record) == {"status", "value"}, f"{label}: evidence fields drift")
    require(record["status"] in {"missing", "present"}, f"{label}: invalid evidence status")
    require(isinstance(record["value"], str), f"{label}: evidence value must be a string")
    if record["status"] == "missing":
        require(record["value"] == "", f"{label}: missing evidence must have an empty value")
        return False
    require(pattern.fullmatch(record["value"]) is not None, f"{label}: invalid evidence value")
    return True


def validate_boolean_evidence(record: dict[str, Any], label: str) -> bool:
    require(set(record) == {"status", "value"}, f"{label}: evidence fields drift")
    require(record["status"] in {"missing", "present"}, f"{label}: invalid evidence status")
    require(type(record["value"]) is bool, f"{label}: evidence value must be a boolean")
    require(
        record["value"] is (record["status"] == "present"),
        f"{label}: positive evidence must be present and true",
    )
    return record["status"] == "present"


def validate_qualification(snapshot: dict[str, Any]) -> None:
    qualification = snapshot["qualification"]
    require(
        set(qualification) == {"qualified", "status", "target_evidence"},
        "qualification fields drift",
    )
    records = qualification["target_evidence"]
    require(isinstance(records, list), "qualification target evidence must be an array")
    require(
        [record["target"] for record in records] == ["linux-x86_64", "macos-arm64"],
        "qualification target evidence set or ordering drift",
    )
    expected_fields = {
        "target",
        "qualified",
        "provider_version",
        "artifact_digest",
        "sbom_digest",
        "signer_identity",
        "attestation_identity",
        "attestation_subject",
        "target_abi",
        "isolated_config_loading",
        "isolated_provider_loading",
        "openssl_default_provider_selected",
        "executable_equivalence_evidence",
    }
    digest = re.compile(r"sha256:[0-9a-f]{64}")
    version = re.compile(r"3\.5\.[0-9]+")
    identity = re.compile(r"(?:https|spiffe)://[^\s]+")
    subject = re.compile(r"[A-Za-z0-9][A-Za-z0-9._~:/@+-]+")
    equivalence = re.compile(r"sha256:[0-9a-f]{64}")
    expected_abi = {
        "linux-x86_64": re.compile(r"x86_64-unknown-linux-gnu;openssl_abi=3"),
        "macos-arm64": re.compile(r"aarch64-apple-darwin;openssl_abi=3"),
    }
    all_complete = True
    for record in records:
        target = record["target"]
        require(set(record) == expected_fields, f"{target}: qualification fields drift")
        complete = all(
            [
                validate_text_evidence(record["provider_version"], f"{target}.provider_version", version),
                validate_text_evidence(record["artifact_digest"], f"{target}.artifact_digest", digest),
                validate_text_evidence(record["sbom_digest"], f"{target}.sbom_digest", digest),
                validate_text_evidence(record["signer_identity"], f"{target}.signer_identity", identity),
                validate_text_evidence(record["attestation_identity"], f"{target}.attestation_identity", identity),
                validate_text_evidence(record["attestation_subject"], f"{target}.attestation_subject", subject),
                validate_text_evidence(record["target_abi"], f"{target}.target_abi", expected_abi[target]),
                validate_boolean_evidence(record["isolated_config_loading"], f"{target}.isolated_config_loading"),
                validate_boolean_evidence(record["isolated_provider_loading"], f"{target}.isolated_provider_loading"),
                validate_boolean_evidence(
                    record["openssl_default_provider_selected"],
                    f"{target}.openssl_default_provider_selected",
                ),
                validate_text_evidence(
                    record["executable_equivalence_evidence"],
                    f"{target}.executable_equivalence_evidence",
                    equivalence,
                ),
            ]
        )
        require(record["qualified"] is complete, f"{target}: qualification boolean disagrees with evidence")
        all_complete = all_complete and complete
    require(qualification["qualified"] is all_complete, "aggregate qualification disagrees with target evidence")
    expected_status = "qualified" if all_complete else "requirements_only"
    require(qualification["status"] == expected_status, "qualification status disagrees with evidence")
    require(qualification["qualified"] is False, "checked-in provider qualification must remain false without evidence")


def validate_readiness(snapshot: dict[str, Any]) -> None:
    require(
        snapshot["readiness"]
        == {
            "evidence_tier": "static_spike",
            "status": "partial",
            "production_qualified": False,
            "blocking_gaps": [
                "executable_algorithm_vectors",
                "linux_x86_64_provider_artifact_and_abi",
                "macos_arm64_provider_artifact_and_abi",
                "provider_attestation_and_sbom",
                "target_equivalence_execution",
            ],
        },
        "runtime crypto readiness drift",
    )


def validate_execution_and_failures(snapshot: dict[str, Any]) -> None:
    require(
        snapshot["execution"]
        == {
            "effect_phase": "runtime_only",
            "compile_time_effects": "forbidden",
            "runtime_input_length": "checked_and_chunked",
            "streaming_threshold_bytes": 65536,
            "allocation_failure": "allocation_failed",
            "build_evidence_values": "metadata_only",
        },
        "runtime-only execution policy drift",
    )
    failure = snapshot["failure_model"]
    require(failure["fail_closed"] is True, "crypto failures must fail closed")
    require(failure["codes"] == sorted(EXPECTED_FAILURES), "stable failure code set drift")
    require(failure["partial_entropy"] == "discard_buffer_and_fail", "partial entropy policy drift")
    require(failure["provider_failure"] == "return_no_output", "provider failure output drift")
    require(
        failure["authentication_failure"] == "return_no_plaintext",
        "authentication failure must return no plaintext",
    )
    require(
        failure["verification_failure"] == "return_false_and_record_stable_code_only",
        "verification failure behavior drift",
    )


def validate_secret_handling(snapshot: dict[str, Any]) -> None:
    handling = snapshot["secret_handling"]
    require_sorted_unique(handling["secret_classes"], "secret classes")
    require(
        handling["secret_classes"]
        == ["aead_key", "generated_entropy", "mac_key", "signature_secret_key"],
        "secret classes drift",
    )
    require(
        handling["comparison"] == "constant_time_for_mac_tag_and_secret_derived_values",
        "constant-time comparison policy drift",
    )
    require(
        handling["key_identity"]
        == "fixed_opaque_runtime_handle_state_not_an_identifier_and_not_derived_from_secret",
        "key identity must be opaque and non-derived",
    )
    require(
        handling["zeroization"] == "best_effort_for_axiom_owned_secret_buffers_and_provider_contexts",
        "zeroization claim drift",
    )
    require_sorted_unique(handling["zeroization_limits"], "zeroization limits")
    require(len(handling["zeroization_limits"]) >= 4, "zeroization limitations are incomplete")
    forbidden = handling["forbidden_evidence_fields"]
    require_sorted_unique(forbidden, "forbidden evidence fields")
    require(
        set(forbidden)
        >= {
            "generated_bytes",
            "key",
            "message",
            "nonce",
            "plaintext",
            "private_key",
            "secret_key",
        },
        "secret-bearing evidence fields are not fully forbidden",
    )
    inspection = snapshot["inspection"]
    require_sorted_unique(inspection["allowed_fields"], "inspection fields")
    require(
        inspection["allowed_fields"]
        == [
            "algorithm",
            "capability",
            "input_lengths",
            "key_identity",
            "operation",
            "outcome",
            "provider",
            "provider_version",
            "runtime_origin",
            "target",
        ],
        "inspection field allowlist drift",
    )
    require(not (set(forbidden) & set(inspection["allowed_fields"])), "inspection permits a forbidden field")
    require(inspection["runtime_origin"] == "native", "inspection origin is not runtime-native")
    require(inspection["secret_values"] == "forbidden", "inspection permits secret values")
    require(inspection["failure_detail"] == "stable_code_only", "inspection failure detail drift")
    require(inspection["approved_operations"] == EXPECTED_OPERATIONS, "inspection operation allowlist drift")
    require(
        inspection["outcome_codes"] == sorted(["ok", *EXPECTED_FAILURES]),
        "inspection outcome code allowlist drift",
    )
    require(
        inspection["key_identity_values"] == ["not_applicable", "opaque_runtime_handle"],
        "inspection key identity values drift",
    )
    require(
        inspection["entropy_algorithm"] == INSPECTION_ENTROPY_ALGORITHM,
        "inspection entropy algorithm drift",
    )
    require(
        inspection["entropy_provider_by_target"] == ENTROPY_PROVIDER_BY_TARGET,
        "inspection entropy provider mapping drift",
    )
    require(
        inspection["opaque_key_operations"] == sorted(OPAQUE_KEY_OPERATIONS),
        "inspection opaque-key operation mapping drift",
    )
    require(
        inspection["not_applicable_key_operations"] == sorted(NOT_APPLICABLE_KEY_OPERATIONS),
        "inspection unkeyed operation mapping drift",
    )
    require(
        inspection["channels"] == ["errors", "evidence", "logs", "serialized_inspection", "traces"],
        "inspection channel set drift",
    )


def validate_algorithm_vectors(fixture: dict[str, Any]) -> None:
    require(isinstance(fixture, dict), "vector source catalog must be an object")
    require(
        set(fixture) == {"schema_version", "catalog_kind", "vectors"},
        "vector source catalog fields drift",
    )
    require(
        fixture.get("schema_version") == "axiom.runtime_crypto_vector_source_catalog.v1",
        "vector source catalog schema drift",
    )
    require(
        fixture.get("catalog_kind") == "authoritative_sources_only_not_executed",
        "vector source catalog must not claim execution",
    )
    vectors = fixture.get("vectors")
    require(isinstance(vectors, list), "vectors must be an array")
    expected_fields = {
        "id",
        "algorithm",
        "source",
        "source_case",
        "material",
        "execution_status",
        "required_outcomes",
    }
    for vector in vectors:
        require(isinstance(vector, dict), "each vector must be an object")
        require(set(vector) == expected_fields, f"{vector.get('id')}: vector fields drift")
        require(isinstance(vector["id"], str), "vector id must be a string")
        require(vector["id"] in EXPECTED_VECTOR_SOURCES, f"{vector['id']}: unknown vector source")
        expected = {
            "id": vector["id"],
            **EXPECTED_VECTOR_SOURCES[vector["id"]],
            "material": "not_embedded",
            "execution_status": "not_executed",
        }
        require(vector == expected, f"{vector['id']}: vector source catalog drift")
        require(type(vector["source_case"]) is str, f"{vector['id']}: source_case must be scalar text")
        parsed = urlsplit(vector["source"])
        require(
            parsed.scheme == "https" and parsed.hostname in {"csrc.nist.gov", "www.rfc-editor.org"},
            f"{vector['id']}: source is not an authoritative HTTPS origin",
        )
    ids = [vector["id"] for vector in vectors]
    require_sorted_unique(ids, "vector ids")
    require(set(ids) == set(EXPECTED_VECTOR_SOURCES), "vector source catalog coverage drift")
    algorithms = [vector["algorithm"] for vector in vectors]
    require(len(algorithms) == len(EXPECTED_ALGORITHMS), "each algorithm needs one primary vector")
    require(set(algorithms) == set(EXPECTED_ALGORITHMS), "algorithm vector coverage drift")
    require(len(algorithms) == len(set(algorithms)), "algorithm vectors must be one-to-one")


def validate_failure_matrix(fixture: dict[str, Any]) -> None:
    require(isinstance(fixture, dict), "failure catalog must be an object")
    require(
        set(fixture) == {"schema_version", "catalog_kind", "cases"},
        "failure catalog fields drift",
    )
    require(
        fixture.get("schema_version") == "axiom.runtime_crypto_failure_catalog.v1",
        "failure catalog schema drift",
    )
    require(
        fixture.get("catalog_kind") == "contract_cases_only_not_executed",
        "failure catalog must not claim execution",
    )
    cases = fixture.get("cases")
    require(isinstance(cases, list), "failure cases must be an array")
    for case in cases:
        require(isinstance(case, dict), "each failure case must be an object")
        require(
            set(case)
            == {"code", "operation", "output", "secret_output", "execution_status"},
            f"{case.get('code')}: failure case fields drift",
        )
        require(case["code"] in EXPECTED_FAILURES, f"{case['code']}: unknown failure code")
        expected_operation, expected_output = EXPECTED_FAILURES[case["code"]]
        require(case["operation"] == expected_operation, f"{case['code']}: operation drift")
        require(case["output"] == expected_output, f"{case['code']}: output drift")
        require(case["secret_output"] is False, f"{case['code']}: failure returns secret output")
        require(case["execution_status"] == "not_executed", f"{case['code']}: false execution claim")
    codes = [case["code"] for case in cases]
    require(codes == sorted(EXPECTED_FAILURES), "failure matrix coverage or ordering drift")


def validate_outcome(value: Any, snapshot: dict[str, Any], label: str) -> None:
    require(isinstance(value, dict), f"{label}: outcome must be an object")
    require(set(value) == {"status", "code"}, f"{label}: outcome fields drift")
    require(value["status"] in {"success", "failure"}, f"{label}: outcome status drift")
    require(value["code"] in snapshot["inspection"]["outcome_codes"], f"{label}: outcome code drift")
    require(
        (value["status"] == "success") == (value["code"] == "ok"),
        f"{label}: outcome status and code disagree",
    )


def validate_inspection_report(report: Any, snapshot: dict[str, Any], label: str) -> None:
    require(isinstance(report, dict), f"{label}: inspection report must be an object")
    require(set(report) == set(snapshot["inspection"]["allowed_fields"]), f"{label}: inspection fields exceed allowlist")
    algorithm = report["algorithm"]
    operation = report["operation"]
    require(operation in EXPECTED_OPERATIONS, f"{label}: inspection operation is not approved")
    if operation == "entropy.fill":
        require(algorithm == INSPECTION_ENTROPY_ALGORITHM, f"{label}: entropy algorithm drift")
    else:
        require(algorithm in EXPECTED_ALGORITHMS, f"{label}: inspection algorithm is not approved")
        require(operation in ALGORITHM_OPERATIONS[algorithm], f"{label}: operation is invalid for algorithm")
    require(report["capability"] == "crypto", f"{label}: inspection capability drift")
    require(type(report["provider_version"]) is str, f"{label}: provider version must be a string")
    require(report["runtime_origin"] == "native", f"{label}: inspection is not runtime-origin evidence")
    require(report["target"] in {"linux-x86_64", "macos-arm64"}, f"{label}: inspection target unsupported")
    if operation == "entropy.fill":
        require(
            report["provider"] == ENTROPY_PROVIDER_BY_TARGET[report["target"]],
            f"{label}: entropy provider does not match target",
        )
        require(report["provider_version"] == "system-api", f"{label}: entropy provider version drift")
    else:
        require(report["provider"] == "openssl-3.5-evp", f"{label}: inspection provider drift")
        require(re.fullmatch(r"3\.5\.[0-9]+", report["provider_version"]) is not None, f"{label}: inspection version drift")
    require(
        report["key_identity"] in snapshot["inspection"]["key_identity_values"],
        f"{label}: key identity value drift",
    )
    expected_key_identity = (
        "opaque_runtime_handle" if operation in OPAQUE_KEY_OPERATIONS else "not_applicable"
    )
    require(report["key_identity"] == expected_key_identity, f"{label}: key identity is invalid for operation")
    validate_outcome(report["outcome"], snapshot, label)
    lengths = report["input_lengths"]
    require(isinstance(lengths, dict), f"{label}: inspection input lengths must be an object")
    require(set(lengths) == INSPECTION_INPUT_LENGTH_FIELDS[operation], f"{label}: input length fields drift")
    require(
        all(type(value) is int and value >= 0 for value in lengths.values()),
        f"{label}: inspection input lengths must be non-negative integers",
    )
    if report["outcome"]["status"] == "success":
        if operation == "entropy.fill":
            entropy_policy = next(
                source
                for source in snapshot["entropy_sources"]
                if source["id"] == report["provider"]
            )
            require(
                0 < lengths["requested"] <= entropy_policy["maximum_request_bytes"],
                f"{label}: entropy request exceeds the provider policy",
            )
        else:
            policy = EXPECTED_ALGORITHMS[algorithm]
            if "key" in lengths:
                key_policy = policy["key_bytes"]
                allowed = key_policy["allowed"]
                require(
                    lengths["key"] in allowed
                    if allowed
                    else key_policy["minimum"] <= lengths["key"] <= key_policy["maximum"],
                    f"{label}: key length contradicts the algorithm policy",
                )
            if "nonce" in lengths:
                require(
                    lengths["nonce"] in policy["nonce_bytes"],
                    f"{label}: nonce length contradicts the algorithm policy",
                )
            if "secret_key" in lengths:
                require(
                    algorithm == "ed25519@1" and lengths["secret_key"] == 32,
                    f"{label}: secret key length contradicts the algorithm policy",
                )
            if "public_key" in lengths:
                require(
                    algorithm == "ed25519@1" and lengths["public_key"] == 32,
                    f"{label}: public key length contradicts the algorithm policy",
                )
            if "signature" in lengths:
                require(
                    algorithm == "ed25519@1" and lengths["signature"] == 64,
                    f"{label}: signature length contradicts the algorithm policy",
                )
            if "tag" in lengths:
                require(
                    lengths["tag"] == policy["output_bytes"],
                    f"{label}: tag length contradicts the algorithm policy",
                )
            if operation == "aead.open":
                require(
                    lengths["ciphertext"] >= policy["tag_bytes"],
                    f"{label}: ciphertext is shorter than the authentication tag",
                )


def assert_markers_absent(value: Any, markers: list[str], label: str) -> None:
    encoded = json.dumps(value, ensure_ascii=True, separators=(",", ":"), sort_keys=True)
    for marker in markers:
        require(marker not in encoded, f"{label}: secret marker material is present")


def validate_inspection_fixture(fixture: dict[str, Any], snapshot: dict[str, Any]) -> None:
    require(isinstance(fixture, dict), "inspection fixture must be an object")
    require(
        set(fixture) == {"schema_version", "report", "representative_reports", "channels"},
        "inspection fixture fields drift",
    )
    require(
        fixture.get("schema_version") == "axiom.runtime_crypto_inspection.v1",
        "inspection fixture schema drift",
    )
    report = fixture.get("report")
    validate_inspection_report(report, snapshot, "inspection.report")
    representative_reports = fixture["representative_reports"]
    require(
        isinstance(representative_reports, list) and len(representative_reports) == 2,
        "inspection representative reports drift",
    )
    for index, representative in enumerate(representative_reports):
        validate_inspection_report(representative, snapshot, f"inspection.representative_reports[{index}]")
    require(
        {item["operation"] for item in representative_reports} == {"entropy.fill", "hash.digest"},
        "inspection representative operation coverage drift",
    )
    channels = fixture["channels"]
    require(isinstance(channels, dict), "inspection channels must be an object")
    require(set(channels) == set(snapshot["inspection"]["channels"]), "inspection channel set drift")
    require(channels["serialized_inspection"] == report, "serialized inspection must be the closed report")
    logs = channels["logs"]
    require(
        logs
        == {
            "algorithm": "aes-256-gcm@1",
            "key_identity": "opaque_runtime_handle",
            "operation": "aead.seal",
            "outcome": {"status": "success", "code": "ok"},
        },
        "closed log channel drift",
    )
    traces = channels["traces"]
    require(
        traces
        == {
            **logs,
            "provider": "openssl-3.5-evp",
            "target": "linux-x86_64",
        },
        "closed trace channel drift",
    )
    require(
        channels["errors"]
        == {
            "code": "authentication_failed",
            "key_identity": "opaque_runtime_handle",
            "operation": "aead.open",
        },
        "closed error channel drift",
    )
    require(
        channels["evidence"]
        == {
            "provider": "openssl-3.5-evp",
            "provider_version": "3.5.7",
            "qualification": "unqualified",
            "target": "linux-x86_64",
        },
        "closed evidence channel drift",
    )


def validate_provider_matrix(fixture: dict[str, Any], snapshot: dict[str, Any]) -> None:
    require(isinstance(fixture, dict), "provider matrix must be an object")
    require(
        set(fixture) == {"schema_version", "algorithm_provider", "targets", "qualification"},
        "provider matrix fields drift",
    )
    require(
        fixture.get("schema_version") == "axiom.runtime_crypto_provider_matrix.v1",
        "provider matrix schema drift",
    )
    require(
        json_equal(
            fixture.get("algorithm_provider"),
            {
            "id": snapshot["algorithm_provider"]["id"],
            "requirements": {
                "load_policy": snapshot["algorithm_provider"]["load_policy"],
                "ambient_host_loading": snapshot["algorithm_provider"]["ambient_host_loading"],
                "provider_name": snapshot["algorithm_provider"]["provider_name"],
                "fips_claim": snapshot["algorithm_provider"]["fips_claim"],
                "qualification_requirements": snapshot["algorithm_provider"]["qualification_requirements"],
            },
            },
        ),
        "provider matrix policy drift",
    )
    require(json_equal(fixture.get("targets"), snapshot["targets"]), "provider matrix target drift")
    require(json_equal(fixture.get("qualification"), snapshot["qualification"]), "provider qualification matrix drift")


def validate_fixtures(reader: RepositoryReader, snapshot: dict[str, Any]) -> None:
    specs = snapshot["fixtures"]
    paths = [spec["path"] for spec in specs]
    require(paths == sorted(EXPECTED_FIXTURES), "fixture set or ordering drift")
    require(len(paths) == len(set(paths)), "fixture paths must be unique")
    for spec in specs:
        expected_id, expected_kind, expected_asserts = EXPECTED_FIXTURES[spec["path"]]
        require(spec["id"] == expected_id, f"{spec['path']}: fixture id drift")
        require(spec["kind"] == expected_kind, f"{spec['path']}: fixture kind drift")
        require(spec["asserts"] == expected_asserts, f"{spec['path']}: fixture assertions drift")
        fixture = reader.read_json(f"{FIXTURES}/{spec['path']}")
        if spec["path"] == "algorithm-vectors.json":
            validate_algorithm_vectors(fixture)
        elif spec["path"] == "failure-matrix.json":
            validate_failure_matrix(fixture)
        elif spec["path"] == "inspection-redaction.json":
            validate_inspection_fixture(fixture, snapshot)
        elif spec["path"] == "provider-matrix.json":
            validate_provider_matrix(fixture, snapshot)
        else:
            raise ContractError(f"unknown fixture {spec['path']}")


def validate_contract(root: Path) -> dict[str, Any]:
    with RepositoryReader(root) as reader:
        schema_bytes = reader.read_bytes(SCHEMA)
        require(
            hashlib.sha256(schema_bytes).hexdigest() == EXPECTED_SCHEMA_SHA256,
            "published runtime crypto policy schema bytes drift",
        )
        schema = decode_json(schema_bytes, SCHEMA)
        snapshot = reader.read_json(SNAPSHOT)
        require(
            schema.get("$id", "").endswith("axiom.runtime_crypto_provider_policy.v1.schema.json"),
            "schema id drift",
        )
        validate_schema(snapshot, schema)
        require(snapshot["schema_version"] == "axiom.runtime_crypto_provider_policy.v1", "schema version drift")
        require(snapshot["policy_id"] == "runtime.crypto.provider", "policy id drift")
        require(snapshot["policy_version"] == "1.0.0", "policy version drift")
        require(snapshot["issue"] == 1481, "governing issue drift")
        require(snapshot["status"] == "review_candidate", "policy must remain review-gated")
        require(
            snapshot["activation"]
            == {
                "mechanism": "separate_reviewed_activation_commit",
                "activation_artifact": ACTIVATION_ARTIFACT,
                "artifact_present": False,
                "trusted_branch": "main",
                "merge_effect": "none",
                "required_roles": ["security_review", "verification_review"],
                "resulting_status": "review_candidate",
                "implementation_claim": "none",
            },
            "activation gate drift",
        )
        require(
            not reader.exists(ACTIVATION_ARTIFACT),
            "activation artifact is present while the checked-in state is review_candidate",
        )
        require(snapshot["capability"] == "crypto", "capability drift")
        validate_algorithms(snapshot)
        validate_provider(snapshot)
        validate_entropy(snapshot)
        validate_targets(snapshot)
        validate_qualification(snapshot)
        validate_execution_and_failures(snapshot)
        validate_secret_handling(snapshot)
        validate_readiness(snapshot)
        validate_fixtures(reader, snapshot)
    providers = [snapshot["algorithm_provider"]["id"]] + [
        source["id"] for source in snapshot["entropy_sources"]
    ]
    return {
        "algorithms": len(snapshot["algorithms"]),
        "fixtures": len(snapshot["fixtures"]),
        "ok": True,
        "providers": sorted(providers),
        "schema": snapshot["schema_version"],
        "targets": [target["id"] for target in snapshot["targets"]],
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
    except (ContractError, KeyError, TypeError) as error:
        if args.json:
            print(json.dumps({"error": str(error), "ok": False}, sort_keys=True))
        else:
            print(f"runtime-crypto-provider-policy-v1: {error}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(result, sort_keys=True))
    else:
        print("runtime-crypto-provider-policy-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
