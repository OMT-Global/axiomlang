#!/usr/bin/env python3
"""Validate the fail-closed Filesystem v1 evidence contract for issue #1443."""

from __future__ import annotations

import argparse
import json
import os
import re
import stat
import sys
from pathlib import Path
from typing import Any


DEFAULT_ROOT = Path(__file__).resolve().parents[2]
ROOT = DEFAULT_ROOT
SCHEMA_REL = Path("stage1/compiler-contracts/schemas/axiom.filesystem.v1.schema.json")
SNAPSHOT_REL = Path("stage1/compiler-contracts/snapshots/filesystem-v1.json")
FIXTURE_DIR_REL = Path("stage1/compiler-contracts/fixtures/filesystem-v1")
READINESS_REL = Path("docs/production-language-readiness.json")
CAPABILITY_LEDGER_REL = Path("stage1/compiler-contracts/snapshots/capability-ledger.json")
FILESYSTEM_DOC_REL = Path("docs/filesystem-v1.md")
BEHAVIORAL_RUNNER_REL = Path("scripts/ci/run-filesystem-v1-behavioral-tests.sh")
MAX_CHECKED_FILE_BYTES = 1024 * 1024
MAX_REQUEST_BYTES = 1024 * 1024

PATH_OPERATIONS = {
    "canonicalize",
    "exists",
    "extension",
    "file_type",
    "is_absolute",
    "join",
    "metadata",
    "name",
    "normalize",
    "parent",
    "read_dir",
    "relative_to",
}
AUTHORITIES = {"metadata", "read", "temporary", "traversal", "write"}
OPERATION_AUTHORITIES = {
    "append_file": "write",
    "atomic_replace": "write",
    "canonicalize": "traversal",
    "create_file": "write",
    "create_temporary_directory": "temporary",
    "create_temporary_file": "temporary",
    "create_temporary_resource": "temporary",
    "exists": "metadata",
    "extension": "metadata",
    "file_exists": "metadata",
    "file_size": "metadata",
    "file_type": "metadata",
    "is_absolute": "metadata",
    "join": "metadata",
    "metadata": "metadata",
    "mkdir": "write",
    "mkdir_all": "write",
    "name": "metadata",
    "normalize": "traversal",
    "open_read": "read",
    "open_write": "write",
    "parent": "metadata",
    "read": "read",
    "read_dir": "traversal",
    "read_file": "read",
    "relative_to": "metadata",
    "remove_dir": "write",
    "remove_file": "write",
    "replace_file": "write",
    "write": "write",
    "write_file": "write",
}
HANDLE_AUTHORITY_OPERATIONS = {"close", "flush", "fsync", "seek"}
AUTHORITY_DENIAL_CASES = [
    {
        "operation": operation,
        "required_authority": authority,
        "provided_authorities": provided,
        "expected": "denied",
        "allocation_performed": False,
        "host_io_performed": False,
    }
    for authority, operation in (
        ("metadata", "metadata"),
        ("read", "open_read"),
        ("temporary", "create_temporary_file"),
        ("traversal", "read_dir"),
        ("write", "open_write"),
    )
    for provided in ([], [next(item for item in sorted(AUTHORITIES) if item != authority)])
]
RESOURCE_OPERATIONS = {
    "close",
    "flush",
    "fsync",
    "open_read",
    "open_write",
    "read",
    "seek",
    "write",
}
SECURITY_RULES = {
    "cleanup_on_drop",
    "exclusive_create",
    "nofollow",
    "restrictive_permissions",
    "runtime_revalidate_parent",
    "same_directory_replace",
    "sync_directory_after_replace",
    "sync_file_before_replace",
    "unpredictable_name",
}
INSPECTION_FIELDS = {
    "authority",
    "bytes_completed",
    "bytes_requested",
    "denial_reason",
    "generation",
    "handle_id",
    "lifetime",
    "normalized_path",
    "operation",
    "runtime_path_origin",
    "scoped_root",
}
OUTCOMES = {
    "accepted",
    "committed_durability_uncertain",
    "denied",
    "end_of_file",
    "io_error",
    "not_found",
    "partial",
    "stale_handle",
    "unsupported",
}
FIXTURE_SPECS = {
    "authority-denials": {
        "kind": "negative",
        "evidence": "target",
        "authorities": [],
        "operation": "authority_matrix",
        "expected": "denied",
        "assertions": [
            "missing authority denies before allocation or host I/O",
            "wrong authority denies before allocation or host I/O",
            "every authority partition has negative evidence",
        ],
    },
    "atomic-replace": {
        "kind": "positive",
        "evidence": "target",
        "authorities": ["write"],
        "operation": "atomic_replace",
        "expected": "accepted",
        "assertions": [
            "directory sync failure after rename reports committed_durability_uncertain",
            "file synced before rename",
            "pre-rename failure preserves old destination",
            "rename is the commit point",
            "temporary file created exclusively in destination directory",
        ],
    },
    "deterministic-directory-order": {
        "kind": "positive",
        "evidence": "target",
        "authorities": ["traversal"],
        "operation": "read_dir",
        "expected": "accepted",
        "assertions": [
            "case is preserved without folding",
            "dot components resolve within the scoped root",
            "metadata does not change ordering",
            "separators normalize to slash",
            "unicode is compared without normalization",
        ],
    },
    "insecure-temporary-name": {
        "kind": "negative",
        "evidence": "target",
        "authorities": ["temporary"],
        "operation": "create_temporary_file",
        "expected": "denied",
        "assertions": [
            "exclusive create required",
            "restrictive permissions required",
            "unpredictable name required",
        ],
    },
    "oversize-io": {
        "kind": "negative",
        "evidence": "target",
        "authorities": ["read"],
        "operation": "read",
        "expected": "denied",
        "assertions": [
            "finite request above the published maximum is denied",
            "handle remains valid",
            "no allocation or I/O performed",
        ],
    },
    "partial-binary-read": {
        "kind": "positive",
        "evidence": "target",
        "authorities": ["read"],
        "operation": "read",
        "expected": "partial",
        "assertions": [
            "bytes completed reported",
            "bytes requested bounded",
            "handle generation remains valid",
            "zero progress is not success",
        ],
    },
    "partial-binary-write": {
        "kind": "positive",
        "evidence": "target",
        "authorities": ["write"],
        "operation": "write",
        "expected": "partial",
        "assertions": [
            "bytes completed reported",
            "bytes requested bounded",
            "handle generation remains valid",
            "zero progress is not success",
        ],
    },
    "scoped-text-floor": {
        "kind": "positive",
        "evidence": "current",
        "authorities": ["read"],
        "operation": "read_file",
        "expected": "accepted",
        "assertions": [
            "content size bounded",
            "runtime root revalidated",
            "static symlink escape denied",
            "TOCTOU-safe pathname operation not claimed",
        ],
    },
    "secure-temporary-resource": {
        "kind": "positive",
        "evidence": "target",
        "authorities": ["temporary"],
        "operation": "create_temporary_resource",
        "expected": "accepted",
        "assertions": [
            "cleanup registered with lifecycle",
            "exclusive create used",
            "nofollow enforced",
            "permissions restrictive",
            "unpredictable name used",
        ],
    },
    "symlink-swap": {
        "kind": "negative",
        "evidence": "target",
        "authorities": ["write"],
        "operation": "open_write",
        "expected": "denied",
        "assertions": [
            "descriptor-anchored parent identity retained",
            "nofollow enforced at operation",
            "root escape denied without host write",
        ],
    },
    "traversal-escape": {
        "kind": "negative",
        "evidence": "current",
        "authorities": ["traversal"],
        "operation": "normalize",
        "expected": "denied",
        "assertions": [
            "denial reason reported",
            "no host operation performed",
            "root escape denied",
        ],
    },
    "typed-path": {
        "kind": "positive",
        "evidence": "target",
        "authorities": ["metadata"],
        "operation": "join",
        "expected": "accepted",
        "assertions": [
            "absolute policy explicit",
            "path origin retained",
            "stable path exposed",
        ],
    },
    "unbounded-io": {
        "kind": "negative",
        "evidence": "target",
        "authorities": ["read"],
        "operation": "read",
        "expected": "denied",
        "assertions": [
            "denial reason reported",
            "handle remains valid",
            "no allocation or I/O performed",
        ],
    },
}
FIXTURES = set(FIXTURE_SPECS)
CURRENT_IMPLEMENTATION = {
    "tier": "static_spike",
    "status": "partial",
    "blockers": [1425, 1426, 1434, 1438],
    "scoped_text_io": True,
    "root_scoped_metadata": True,
    "root_scoped_write": True,
    "typed_paths": False,
    "binary_handles": False,
    "deterministic_traversal": False,
    "atomic_replace": False,
    "secure_temporary_resources": False,
    "runtime_effects_only": False,
    "descriptor_anchored_replace": False,
    "pathname_operations_toctou_safe": False,
}
CURRENT_STDLIB_FUNCTIONS = {
    "append_file",
    "create_file",
    "file_exists",
    "file_size",
    "mkdir",
    "mkdir_all",
    "read_file",
    "remove_dir",
    "remove_file",
    "replace_file",
    "write_file",
}
UNIMPLEMENTED_STDLIB_FUNCTIONS = {
    "canonicalize",
    "create_temporary_directory",
    "create_temporary_file",
    "open_binary",
    "read_dir",
}


class ContractError(ValueError):
    pass


def fail(message: str) -> None:
    raise ContractError(message)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def read_checked_bytes(root: Path, relative: Path) -> bytes:
    require(
        bool(relative.parts)
        and not relative.is_absolute()
        and all(component not in {"", ".", ".."} for component in relative.parts),
        f"unsafe checkout path: {relative}",
    )
    nofollow = getattr(os, "O_NOFOLLOW", 0)
    directory = getattr(os, "O_DIRECTORY", 0)
    nonblock = getattr(os, "O_NONBLOCK", 0)
    cloexec = getattr(os, "O_CLOEXEC", 0)
    require(nofollow != 0 and directory != 0, "descriptor-safe checkout reads are unavailable")
    descriptors: list[int] = []
    try:
        current = os.open(os.fspath(root), os.O_RDONLY | directory | nofollow | cloexec)
        descriptors.append(current)
        for component in relative.parts[:-1]:
            current = os.open(
                component,
                os.O_RDONLY | directory | nofollow | cloexec,
                dir_fd=current,
            )
            descriptors.append(current)
        file_descriptor = os.open(
            relative.parts[-1],
            os.O_RDONLY | nofollow | nonblock | cloexec,
            dir_fd=current,
        )
        descriptors.append(file_descriptor)
        metadata = os.fstat(file_descriptor)
        require(stat.S_ISREG(metadata.st_mode), f"checkout path is not a regular file: {relative}")
        require(
            metadata.st_size <= MAX_CHECKED_FILE_BYTES,
            f"checkout file exceeds {MAX_CHECKED_FILE_BYTES} bytes: {relative}",
        )
        chunks: list[bytes] = []
        total = 0
        while True:
            chunk = os.read(
                file_descriptor,
                min(64 * 1024, MAX_CHECKED_FILE_BYTES + 1 - total),
            )
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            require(
                total <= MAX_CHECKED_FILE_BYTES,
                f"checkout file exceeds {MAX_CHECKED_FILE_BYTES} bytes: {relative}",
            )
        return b"".join(chunks)
    except ContractError:
        raise
    except OSError as error:
        fail(f"cannot safely read {relative}: {error.strerror or error}")
    finally:
        for descriptor in reversed(descriptors):
            try:
                os.close(descriptor)
            except OSError:
                pass


def read_checked_text(root: Path, relative: Path) -> str:
    try:
        return read_checked_bytes(root, relative).decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        fail(f"checkout file is not valid UTF-8: {relative}")


def load_object(path: Path, *, root: Path | None = None) -> dict[str, Any]:
    try:
        content = read_checked_text(root, path) if root is not None else path.read_text(encoding="utf-8")
        value = json.loads(content)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")
    require(isinstance(value, dict), f"{path} must contain an object")
    return value


def validate_schema_node(
    value: Any,
    schema: dict[str, Any],
    path: str,
    definitions: dict[str, Any],
) -> None:
    if "$ref" in schema:
        prefix = "#/$defs/"
        reference = schema["$ref"]
        require(reference.startswith(prefix), f"{path} uses unsupported schema ref {reference}")
        name = reference[len(prefix) :]
        require(name in definitions, f"{path} references unknown schema def {name}")
        validate_schema_node(value, definitions[name], path, definitions)
        return

    for index, nested in enumerate(schema.get("allOf", [])):
        validate_schema_node(value, nested, f"{path}.allOf[{index}]", definitions)
    if "if" in schema:
        try:
            validate_schema_node(value, schema["if"], path, definitions)
        except ContractError:
            branch = schema.get("else")
        else:
            branch = schema.get("then")
        if branch is not None:
            validate_schema_node(value, branch, path, definitions)

    if "const" in schema:
        require(value == schema["const"], f"{path} must equal {schema['const']!r}")
    if "enum" in schema:
        require(value in schema["enum"], f"{path} must be one of {schema['enum']!r}")

    expected_type = schema.get("type")
    if expected_type == "object":
        require(isinstance(value, dict), f"{path} must be an object")
    elif expected_type == "array":
        require(isinstance(value, list), f"{path} must be an array")
    elif expected_type == "string":
        require(isinstance(value, str), f"{path} must be a string")
    elif expected_type == "integer":
        require(
            isinstance(value, int) and not isinstance(value, bool),
            f"{path} must be an integer",
        )
    elif expected_type == "boolean":
        require(isinstance(value, bool), f"{path} must be a boolean")
    elif expected_type is not None:
        fail(f"{path} uses unsupported schema type {expected_type}")

    if isinstance(value, dict):
        required = set(schema.get("required", []))
        missing = sorted(required - set(value))
        require(not missing, f"{path} is missing required fields: {', '.join(missing)}")
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            unexpected = sorted(set(value) - set(properties))
            require(not unexpected, f"{path} has unexpected fields: {', '.join(unexpected)}")
        for key, nested in value.items():
            if key in properties:
                validate_schema_node(nested, properties[key], f"{path}.{key}", definitions)
    elif isinstance(value, list):
        minimum = schema.get("minItems")
        if minimum is not None:
            require(len(value) >= minimum, f"{path} must have at least {minimum} items")
        maximum = schema.get("maxItems")
        if maximum is not None:
            require(len(value) <= maximum, f"{path} must have at most {maximum} items")
        item_schema = schema.get("items")
        if item_schema:
            for index, item in enumerate(value):
                validate_schema_node(item, item_schema, f"{path}[{index}]", definitions)
    elif isinstance(value, str):
        if "minLength" in schema:
            require(len(value) >= schema["minLength"], f"{path} must not be empty")
        if "pattern" in schema:
            require(re.search(schema["pattern"], value) is not None, f"{path} has an invalid form")


def require_sorted_exact(values: list[Any], expected: set[Any], label: str) -> None:
    require(set(values) == expected, f"{label} are incomplete")
    require(values == sorted(values), f"{label} must be deterministically ordered")
    require(len(values) == len(set(values)), f"{label} must be unique")


def normalize_fixture_path(value: str, origin: str) -> str:
    components: list[str] = []
    require(origin in {"posix", "windows"}, "directory-order path origin is invalid")
    normalized_separators = value.replace("\\", "/") if origin == "windows" else value
    for component in normalized_separators.split("/"):
        if component in ("", "."):
            continue
        if component == "..":
            require(bool(components), "directory-order vector escapes its scoped root")
            components.pop()
            continue
        components.append(component)
    return "/".join(components)


def fixture_path_order_key(entry: dict[str, str]) -> tuple[str, str, str]:
    return (entry["normalized"], entry["origin"], entry["path"])


def validate_fixture(root: Path, path: Path, reference: dict[str, Any]) -> None:
    fixture = load_object(path, root=root)
    name = reference["id"].rsplit("/", 1)[-1]
    spec = FIXTURE_SPECS[name]
    common_fields = {
        "schema_version",
        "id",
        "kind",
        "evidence",
        "scenario",
        "authorities",
        "operation",
        "expected",
        "assertions",
    }
    special_fields: dict[str, set[str]] = {
        "authority-denials": {"cases"},
        "atomic-replace": {"phase_outcomes"},
        "deterministic-directory-order": {
            "normalization",
            "inputs",
            "expected_order",
        },
        "partial-binary-read": {
            "requested_bytes",
            "completed_bytes",
            "remaining_bytes",
        },
        "partial-binary-write": {
            "requested_bytes",
            "completed_bytes",
            "remaining_bytes",
        },
        "oversize-io": {"requested_bytes", "maximum_request_bytes"},
    }
    require(
        set(fixture) == common_fields | special_fields.get(name, set()),
        f"{path.name} fields drifted",
    )
    require(fixture["schema_version"] == "axiom.filesystem_fixture.v1", f"{path.name} schema drifted")
    require(fixture["id"] == reference["id"], f"{path.name} id disagrees with snapshot")
    for field in ("kind", "evidence"):
        require(
            fixture[field] == reference[field] == spec[field],
            f"{path.name} {field} disagrees with its exact contract",
        )
    require(
        fixture["authorities"] == spec["authorities"],
        f"{path.name} authorities drifted",
    )
    require(
        set(fixture["authorities"]) <= AUTHORITIES,
        f"{path.name} uses an unknown authority",
    )
    require(fixture["operation"] == spec["operation"], f"{path.name} operation drifted")
    require(fixture["expected"] == spec["expected"], f"{path.name} outcome drifted")
    require(fixture["expected"] in OUTCOMES, f"{path.name} uses an unknown outcome")
    require(isinstance(fixture["scenario"], str) and fixture["scenario"], f"{path.name} needs a scenario")
    require(
        fixture["assertions"] == spec["assertions"],
        f"{path.name} assertions drifted",
    )
    if fixture["kind"] == "negative":
        require(fixture["expected"] == "denied", f"{path.name} negative fixture must deny")

    if name in {"partial-binary-read", "partial-binary-write"}:
        requested = fixture["requested_bytes"]
        completed = fixture["completed_bytes"]
        remaining = fixture["remaining_bytes"]
        require(
            all(
                isinstance(value, int)
                and not isinstance(value, bool)
                and value >= 0
                for value in (requested, completed, remaining)
            ),
            f"{path.name} byte counts must be non-negative integers",
        )
        require(
            0 < completed < requested <= MAX_REQUEST_BYTES
            and completed + remaining == requested,
            f"{path.name} partial byte accounting is inconsistent",
        )

    if name == "authority-denials":
        require(
            fixture["cases"] == AUTHORITY_DENIAL_CASES,
            "authority-denial cases do not cover missing and wrong grants for every partition",
        )
        require(
            all(
                OPERATION_AUTHORITIES[case["operation"]] == case["required_authority"]
                for case in fixture["cases"]
            ),
            "authority-denial cases disagree with the canonical operation matrix",
        )

    if name == "oversize-io":
        require(
            fixture["maximum_request_bytes"] == MAX_REQUEST_BYTES
            and fixture["requested_bytes"] == MAX_REQUEST_BYTES + 1,
            "oversize-I/O fixture must exceed the exact published request bound",
        )

    if name == "deterministic-directory-order":
        expected_normalization = {
            "separator": "windows_backslash_to_slash_posix_preserved",
            "dot_components": "remove_dot_and_resolve_dotdot_within_root",
            "unicode": "none",
            "case_folding": "none",
            "comparison_key": "normalized_path_then_origin_then_raw_path_unicode_scalar_sequence",
        }
        require(
            fixture["normalization"] == expected_normalization,
            "directory-order normalization rules drifted",
        )
        inputs = fixture["inputs"]
        require(
            isinstance(inputs, list)
            and all(
                isinstance(value, dict)
                and set(value) == {"origin", "path"}
                and value["origin"] in {"posix", "windows"}
                and isinstance(value["path"], str)
                and value["path"]
                for value in inputs
            ),
            "directory-order inputs must be typed non-empty paths",
        )
        normalized = [
            {
                "origin": value["origin"],
                "path": value["path"],
                "normalized": normalize_fixture_path(value["path"], value["origin"]),
            }
            for value in inputs
        ]
        require(
            fixture["expected_order"] == sorted(normalized, key=fixture_path_order_key),
            "directory-order vectors do not match the injective typed-path ordering key",
        )
        require(
            {entry["normalized"] for entry in normalized} >= {"a/é.txt", "a/é.txt"},
            "directory-order vectors must preserve composed and decomposed Unicode",
        )
        require(
            len({fixture_path_order_key(entry) for entry in normalized}) == len(normalized),
            "directory-order comparison keys must be injective",
        )

    if name == "atomic-replace":
        require(
            fixture["phase_outcomes"]
            == [
                {
                    "phase": "before_rename",
                    "committed": False,
                    "destination": "old",
                    "outcome": "io_error",
                },
                {
                    "phase": "rename",
                    "committed": True,
                    "destination": "new",
                    "outcome": "accepted",
                },
                {
                    "phase": "after_rename_directory_sync",
                    "committed": True,
                    "destination": "new",
                    "outcome": "committed_durability_uncertain",
                },
            ],
            "atomic-replace commit-point outcomes drifted",
        )


def validate_current_implementation(root: Path, snapshot: dict[str, Any]) -> None:
    require(
        snapshot["implementation"] == CURRENT_IMPLEMENTATION,
        "current Filesystem v1 implementation evidence drifted or was promoted",
    )

    stdlib = read_checked_text(root, Path("stage1/crates/axiomc/src/stdlib.rs"))
    start = stdlib.index('        "fs.ax",')
    end = stdlib.index('        "net.ax",', start)
    fs_module = stdlib[start:end]
    functions = set(re.findall(r"\bpub fn ([a-z][a-z0-9_]*)", fs_module))
    require(functions == CURRENT_STDLIB_FUNCTIONS, "current std/fs.ax helper floor drifted")
    require(
        functions <= set(OPERATION_AUTHORITIES),
        "current std/fs.ax helpers are missing exact operation-authority assignments",
    )
    require(
        not (functions & UNIMPLEMENTED_STDLIB_FUNCTIONS),
        "Filesystem v1 snapshot must be updated before new std/fs.ax resources are exposed",
    )

    codegen = read_checked_text(root, Path("stage1/crates/axiomc/src/codegen.rs"))
    for marker in (
        "AXIOM_MAX_FS_READ_BYTES",
        "AXIOM_MAX_FS_WRITE_BYTES",
        "axiom_fs_candidate",
        "std::fs::symlink_metadata",
        "std::fs::rename",
        '.axiom-replace-{}-{stamp}.tmp',
    ):
        require(marker in codegen, f"current filesystem runtime evidence lost {marker}")

    evaluator = read_checked_text(
        root, Path("stage1/crates/axiomc/src/cranelift_backend/evaluator.rs")
    )
    for marker in (
        '"fs_write" =>',
        '"fs_replace" =>',
        "std::fs::write(candidate, content)",
        "std::fs::create_dir_all(candidate)",
    ):
        require(
            marker in evaluator,
            f"compile-time filesystem fallback evidence lost {marker}",
        )

    direct_native = read_checked_text(
        root, Path("stage1/crates/axiomc-backend-cranelift/src/lib.rs")
    )
    for marker in (
        "libc::O_NOFOLLOW",
        "libc::O_EXCL",
        "runtime_refs.openat",
        "runtime_refs.renameat",
        "runtime_refs.fsync",
    ):
        require(
            marker in direct_native,
            f"descriptor-anchored replace evidence lost {marker}",
        )

    rfc = read_checked_text(root, Path("docs/rfcs/0002-write-capability-boundary.md"))
    for marker in ("`fs:write`", "parent-directory traversal", "symlink escape", "temporary-directory allocation"):
        require(marker in rfc, f"filesystem capability RFC lost {marker}")

    filesystem_doc = read_checked_text(root, FILESYSTEM_DOC_REL)
    for marker in (
        "not descriptor-anchored",
        "replacement between validation and use",
        "`runtime_effects_only` evidence is therefore false",
        "Issue `#1434`",
        "Rename is the commit point",
        "`committed_durability_uncertain`",
        "Unicode normalization and no case folding",
    ):
        require(marker in filesystem_doc, f"Filesystem v1 documentation lost {marker}")

    readiness = load_object(READINESS_REL, root=root)
    readiness_row = next(
        (
            row
            for row in readiness.get("rows", [])
            if row.get("id") == "filesystem_resources"
        ),
        None,
    )
    require(readiness_row is not None, "production readiness lost filesystem_resources")
    require(
        readiness_row.get("currentTier") == "static_spike"
        and readiness_row.get("status") == "partial"
        and readiness_row.get("governingIssue") == 1443,
        "production readiness promoted Filesystem v1 without executable evidence",
    )

    ledger = load_object(CAPABILITY_LEDGER_REL, root=root)
    schema_id = "https://omt-global.github.io/axiom/schemas/axiom.filesystem.v1.schema.json"
    ledger_row = next(
        (row for row in ledger.get("schemas", []) if row.get("name") == schema_id),
        None,
    )
    require(ledger_row is not None, "capability ledger lost Filesystem v1")
    require(
        ledger_row
        == {
            "evidenceTier": "static_spike",
            "name": schema_id,
            "source": str(SCHEMA_REL),
            "status": "checked",
        },
        "capability ledger promoted or rewired Filesystem v1",
    )

    behavior_runner = read_checked_text(root, BEHAVIORAL_RUNNER_REL)
    for marker in (
        "run_required_tests",
        "required evidence filter executed no non-ignored tests",
        "[1-9][0-9]* passed; 0 failed; 0 ignored",
    ):
        require(marker in behavior_runner, f"behavioral runner lost execution assertion {marker}")
    behavior_sources = {
        "build_project_scopes_fs_": Path("stage1/crates/axiomc/tests/support/lib_unit.rs"),
        "stage1_project_imports_synthetic_stdlib_fs_write_helpers": Path(
            "stage1/crates/axiomc/tests/support/lib_unit.rs"
        ),
        "cranelift_backend_lowers_fs_": Path("stage1/crates/axiomc/tests/cranelift_backend.rs"),
        "cranelift_backend_denies_fs_": Path("stage1/crates/axiomc/tests/cranelift_backend.rs"),
        "cranelift_backend_rejects_fs_": Path("stage1/crates/axiomc/tests/cranelift_backend.rs"),
        "links_i64_exit_program_with_replace_file": Path(
            "stage1/crates/axiomc-backend-cranelift/src/lib.rs"
        ),
    }
    for test_filter, source in behavior_sources.items():
        require(
            test_filter in behavior_runner,
            f"behavioral runner lost {test_filter}",
        )
        require(
            test_filter in read_checked_text(root, source),
            f"behavioral evidence source lost {test_filter}",
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate Filesystem v1 against an explicit checkout root."
    )
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    return parser.parse_args()


def main() -> None:
    root = parse_args().root.resolve()
    fixture_dir = root / FIXTURE_DIR_REL
    schema = load_object(SCHEMA_REL, root=root)
    snapshot = load_object(SNAPSHOT_REL, root=root)
    require(
        schema.get("$id", "").endswith("axiom.filesystem.v1.schema.json"),
        "Filesystem v1 schema id mismatch",
    )
    validate_schema_node(snapshot, schema, "$", schema.get("$defs", {}))
    require(
        (snapshot["schema_version"], snapshot["contract"], snapshot["issue"])
        == ("axiom.filesystem.v1", "runtime.filesystem", 1443),
        "Filesystem v1 snapshot identity mismatch",
    )

    require_sorted_exact(snapshot["path_model"]["required_operations"], PATH_OPERATIONS, "path operations")
    require_sorted_exact(snapshot["path_model"]["metadata_fields"], {"file_type", "length_bytes", "modified_time", "permissions", "stable_path"}, "metadata fields")
    require_sorted_exact(snapshot["authority"]["required_kinds"], AUTHORITIES, "authority kinds")
    require_sorted_exact(snapshot["authority"]["current_grants"], {"fs", "fs:write"}, "current authority grants")
    require(
        snapshot["authority"]["operation_requirements"] == OPERATION_AUTHORITIES,
        "filesystem operation-authority matrix drifted",
    )
    require_sorted_exact(
        snapshot["authority"]["handle_authority_operations"],
        HANDLE_AUTHORITY_OPERATIONS,
        "handle authority operations",
    )
    require_sorted_exact(snapshot["file_resources"]["required_operations"], RESOURCE_OPERATIONS, "resource operations")
    require_sorted_exact(snapshot["file_resources"]["seek_origins"], {"current", "end", "start"}, "seek origins")
    require_sorted_exact(snapshot["file_resources"]["flush_modes"], {"buffer", "durable"}, "flush modes")
    require(
        snapshot["file_resources"]["max_request_bytes"] == MAX_REQUEST_BYTES,
        "filesystem request byte bound drifted",
    )
    require_sorted_exact(snapshot["atomic_and_temporary"]["security_rules"], SECURITY_RULES, "security rules")
    require_sorted_exact(snapshot["inspection_fields"], INSPECTION_FIELDS, "inspection fields")
    require_sorted_exact(snapshot["outcomes"], OUTCOMES, "outcomes")
    require_sorted_exact(snapshot["migration"]["blocker_issues"], {1425, 1426, 1434, 1438}, "blocker issues")
    require(
        snapshot["path_model"]["enumeration"]
        == {
            "order": "stable_normalized_path_order",
            "separator_normalization": "typed_origin_windows_backslash_to_slash_posix_preserved",
            "dot_components": "remove_dot_and_resolve_dotdot_within_root",
            "unicode_normalization": "none",
            "case_folding": "none",
            "comparison_key": "normalized_path_then_origin_then_raw_path_unicode_scalar_sequence",
        },
        "directory enumeration semantics drifted",
    )
    require(
        snapshot["atomic_and_temporary"]["atomic_replace"]
        == {
            "required": True,
            "same_directory": True,
            "file_sync_before_rename": True,
            "rename_is_commit_point": True,
            "directory_sync_after_rename": True,
            "pre_commit_failure_preserves_old": True,
            "post_commit_sync_failure": "committed_durability_uncertain",
        },
        "atomic replace commit-point semantics drifted",
    )

    fixture_refs = snapshot["fixtures"]
    require(
        [reference["id"] for reference in fixture_refs]
        == sorted(reference["id"] for reference in fixture_refs),
        "fixture references must be deterministically ordered",
    )
    fixture_ids = {reference["id"].rsplit("/", 1)[-1] for reference in fixture_refs}
    require(fixture_ids == FIXTURES, "Filesystem v1 fixture coverage is incomplete")
    fixture_files = {path.name for path in fixture_dir.glob("*.json")}
    require(fixture_files == {f"{name}.json" for name in FIXTURES}, "Filesystem v1 fixture files drifted")
    for reference in fixture_refs:
        name = reference["id"].rsplit("/", 1)[-1]
        require(
            reference["kind"] == FIXTURE_SPECS[name]["kind"],
            f"fixture kind drifted for {name}",
        )
        require(
            reference["evidence"] == FIXTURE_SPECS[name]["evidence"],
            f"fixture evidence drifted for {name}",
        )
        require(reference["file"] == f"{name}.json", f"fixture filename drifted for {name}")
        validate_fixture(root, FIXTURE_DIR_REL / reference["file"], reference)

    validate_current_implementation(root, snapshot)
    print(
        json.dumps(
            {
                "schema": snapshot["schema_version"],
                "ok": True,
                "authorities": len(AUTHORITIES),
                "fixtures": len(FIXTURES),
                "path_operations": len(PATH_OPERATIONS),
                "resource_operations": len(RESOURCE_OPERATIONS),
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    try:
        main()
    except (ContractError, OSError, ValueError) as error:
        print(f"filesystem-v1: {error}", file=sys.stderr)
        raise SystemExit(1) from error
