#!/usr/bin/env python3
"""Validate Dynamic Aggregate ABI v1 and optionally execute its compiler evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.dynamic_aggregate_abi.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/dynamic-aggregate-abi-v1.json")
FIXTURES = Path("stage1/compiler-contracts/fixtures/dynamic-aggregate-abi-v1")
PROGRAMS = FIXTURES / "programs"
RUNTIME_LEDGER = Path("stage1/runtime-abi/direct-native-v0.json")
READINESS = Path("docs/production-language-readiness.json")
CONTRACT_DOC = Path("docs/dynamic-aggregate-abi-v1.md")
MAX_CHECKED_FILE_BYTES = 1024 * 1024
MAX_MATERIALIZED_LAYOUT_FIELDS = 4096
SCHEMA_SHA256 = "6f314a4abd1c1d4dd735a4e05524dd68c217558974aa0016fe7f9f9fc32b374b"
SNAPSHOT_SHA256 = "bfe4f6b86bab48535d8531d30fd7854360fe98d5d71f95f18c02f070e4c4c44b"

TARGET_PROFILES = {
    ("aarch64-apple-darwin", "aapcs64-v1"): {
        "byte_order": "little",
        "pointer_width_bits": 64,
    },
    ("x86_64-unknown-linux-gnu", "sysv64-v1"): {
        "byte_order": "little",
        "pointer_width_bits": 64,
    },
}
PROGRAM_DIGESTS = {
    "aggregate-forwarding": "5ddf74889e1e52fc8f9b5b7ea52c879ac5511b3b2ad6243dbf0fa99d327f30c7",
    "borrow-conflict": "ff4a93aa7a0aaaa06a816ec764107dcd74d705a9087792e42b5bde4b9e69668d",
    "move-after-move": "5c3614434f4e891b3dcb24a5f437a52e5a59ffcc0aa81af81d99992dc00066c0",
    "owned-string-runtime": "73e2d00e7301410063a742631befaa97c8095030344a2d0823f6d44d5fc24b30",
}
FIXTURE_DIGESTS = {
    "aggregate-helper-return.json": "f4242d4a603e4ddfedfea0314dfaee8ab678d2765e2b8af4681428f7df4a5831",
    "alignment-overflow.json": "e7308d330cfd1cb9b7c6d3e7c33a72292d2df84d23ac979b33f3c787830fd163",
    "borrow-conflict.json": "ca4ec41e60b4fe6784900061a25521ef1437bbc94418fee4c856587a9d0a0e98",
    "clone-independence.json": "69459593c28cfe3d0d157dbed43b4c80f961cadea0822eda0c231c591100a02b",
    "double-drop-rejected.json": "d53f1d00e8284ccef1971d84ceff3bb6535bd86a727742d8d2c00c648dcb5e20",
    "early-return-owned-cleanup.json": "d099a28949665228a054a6288c1493c274bc6b12871cb44c36c0c731d3ab4728",
    "enum-discriminant-payload.json": "95e642ee0d1af3e1cd64f822aa7768385444dd08364439c0e0b4d6dc083eb583",
    "exactly-once-drop.json": "c31f497dd384dc05c87e7eb4014782084eec8223e6076b03d8190536a06970ee",
    "move-after-move.json": "a26ac86ca79cc617b3ff3d26f5dac5f1048f8fa3b168c7606bdb1923d820be25",
    "option-layout-inspection.json": "821a1993c56724819489e94188e01e10beae6e884c45fd1fefc6388916c89cec",
    "owned-string-three-boundary.json": "bdc51e8b79d24686c5cc86647c8ee180fdf8aac13caccaf2b5b580c0dd802531",
    "recursive-owned-drop.json": "d2128b718a0f7b09ba3daeb83a8b0ea5aaf402ac0736a601e53453cb65e76181",
    "result-layout-inspection.json": "7f0dd1fc3ad116b5120d9228ba9963bb65649c6b6a79182d85f3d1e2cf7da65b",
    "static-projection-production-claim.json": "3e189e2a3cf85d2d9c0fe19dae156ce22466d68ac5227655395f8d1f924fc1c2",
    "struct-layout-inspection.json": "5b9c1b27c6094b924d4388358aef56bc37f39fb5f72ea72e29de6384e660be99",
    "target-layout-record.json": "d31169bffa2ae722d57f2741bbd72ebdf57a137c7a80a9368a84716da58c2bdb",
    "three-boundary-nested-scalar.json": "e214a9208a17a188df9c640ed0f6097de9e263d711f3be15956b919ffde64af2",
    "tuple-layout-inspection.json": "e594dccd18366b51e2d1b00625628e086b32abcfe6bc0f3ab194ffbd7fe2dfd7",
}

KINDS = ["array", "enum", "option", "result", "struct", "tuple"]
OPERATIONS = ["argument", "borrow", "clone", "drop", "move", "mutation", "return", "storage"]
SUPPORTED = [
    "fixed_array_scalar_bool",
    "nested_option_result_scalar",
    "scalar_bool_custom_enum",
    "scalar_bool_struct",
    "scalar_bool_tuple",
    "three_boundary_aggregate_forwarding",
]
UNSUPPORTED = [
    "dynamic_non_copy_storage",
    "owned_field_projection",
    "recursive_owned_cleanup",
    "runtime_origin_string_aggregate",
    "static_projection_retirement",
]
TARGET_GAPS = [
    "cancellation_cleanup",
    "double_drop_runtime_diagnostic",
    "early_return_owned_cleanup",
    "panic_unwind_cleanup",
    "recursive_owned_cleanup",
]
INSPECTION_FIELDS = [
    "abi_profile",
    "alignment_bytes",
    "argument_passing",
    "byte_order",
    "cleanup_obligations",
    "discriminant_offset_bytes",
    "discriminant_width_bytes",
    "drop_order",
    "field_offsets_bytes",
    "layout_id",
    "move_state",
    "payload_offset_bytes",
    "pointer_width_bits",
    "return_passing",
    "size_bytes",
    "source_provenance",
    "target_triple",
    "variant_field_offsets_bytes",
]
FIXTURE_NAMES = [
    "aggregate-helper-return",
    "alignment-overflow",
    "borrow-conflict",
    "clone-independence",
    "double-drop-rejected",
    "early-return-owned-cleanup",
    "enum-discriminant-payload",
    "exactly-once-drop",
    "move-after-move",
    "owned-string-three-boundary",
    "option-layout-inspection",
    "recursive-owned-drop",
    "result-layout-inspection",
    "static-projection-production-claim",
    "struct-layout-inspection",
    "target-layout-record",
    "three-boundary-nested-scalar",
    "tuple-layout-inspection",
]
EVIDENCE_MODES = {
    "compiler_build_diagnostic",
    "compiler_check",
    "contract_rejection",
    "layout_model",
    "lifecycle_model",
    "native_execution",
    "target_gap",
}
CAPTURE_WORDS = {"box", "cargo", "cranelift", "repr", "rust", "serde", "vec"}


class ContractError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def load(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"unable to load {path}: {error}") from error


def resolve_checked_path(root: Path, relative: Path, *, directory: bool = False) -> Path:
    resolved_root = root.resolve(strict=True)
    require(
        bool(relative.parts) and not relative.is_absolute() and ".." not in relative.parts,
        f"unsafe checkout path: {relative}",
    )
    candidate = resolved_root
    for component in relative.parts:
        candidate /= component
        require(
            not candidate.is_symlink(),
            f"checkout path uses a symlink component: {relative}",
        )
    resolved = candidate.resolve(strict=True)
    require(resolved.is_relative_to(resolved_root), f"checkout path escapes root: {relative}")
    if directory:
        require(resolved.is_dir(), f"checkout path is not a directory: {relative}")
    else:
        require(resolved.is_file(), f"checkout path is not a regular file: {relative}")
        require(
            resolved.stat().st_size <= MAX_CHECKED_FILE_BYTES,
            f"checkout file exceeds {MAX_CHECKED_FILE_BYTES} bytes: {relative}",
        )
    return resolved


def load_checked(root: Path, relative: Path) -> Any:
    return load(resolve_checked_path(root, relative))


def read_checked_text(root: Path, relative: Path) -> str:
    return resolve_checked_path(root, relative).read_text(encoding="utf-8")


def value_kind(value: Any) -> str:
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
    return "object"


def validate_schema(value: Any, schema: dict[str, Any], path: str, root: dict[str, Any]) -> None:
    if "$ref" in schema:
        prefix = "#/$defs/"
        reference = schema["$ref"]
        require(reference.startswith(prefix), f"{path}: unsupported schema reference")
        definition = reference[len(prefix):]
        require(definition in root.get("$defs", {}), f"{path}: unknown schema reference")
        validate_schema(value, root["$defs"][definition], path, root)
        return
    if "const" in schema:
        require(value == schema["const"], f"{path}: const mismatch")
    if "enum" in schema:
        require(value in schema["enum"], f"{path}: enum mismatch")
    expected = schema.get("type")
    if expected:
        require(value_kind(value) == expected, f"{path}: expected {expected}")
    if isinstance(value, dict):
        properties = schema.get("properties", {})
        for field in schema.get("required", []):
            require(field in value, f"{path}: missing {field}")
        if schema.get("additionalProperties") is False:
            require(not (set(value) - set(properties)), f"{path}: unknown fields")
        for field, nested in value.items():
            if field in properties:
                validate_schema(nested, properties[field], f"{path}.{field}", root)
    if isinstance(value, list):
        require(len(value) >= schema.get("minItems", 0), f"{path}: too few items")
        if schema.get("uniqueItems"):
            require(len({json.dumps(item, sort_keys=True) for item in value}) == len(value), f"{path}: duplicate items")
        if "items" in schema:
            for index, item in enumerate(value):
                validate_schema(item, schema["items"], f"{path}[{index}]", root)
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: empty string")
        if "pattern" in schema:
            require(re.search(schema["pattern"], value) is not None, f"{path}: pattern mismatch")
    if isinstance(value, int) and not isinstance(value, bool) and "minimum" in schema:
        require(value >= schema["minimum"], f"{path}: below minimum")


def require_sorted_unique(values: list[str], label: str) -> None:
    require(values == sorted(set(values)), f"{label} must be sorted and unique")


def align_up(value: int, alignment: int, maximum: int) -> int:
    require(alignment > 0 and alignment & (alignment - 1) == 0, "layout alignment must be a positive power of two")
    require(value >= 0, "layout value must be non-negative")
    padding = (-value) % alignment
    require(value <= maximum - padding, "backend.runtime_lowering_required")
    return value + padding


def scalar_layout(name: str, pointer_bytes: int) -> dict[str, Any]:
    scalars = {
        "bool": (1, 1),
        "u8": (1, 1),
        "u16": (2, 2),
        "u32": (4, 4),
        "u64": (8, 8),
        "i64": (8, 8),
        "pointer": (pointer_bytes, pointer_bytes),
    }
    require(name in scalars, f"unsupported layout scalar: {name}")
    size, alignment = scalars[name]
    return {"size_bytes": size, "alignment_bytes": alignment, "field_offsets_bytes": []}


def compute_type_layout(model: dict[str, Any], pointer_bytes: int, maximum: int) -> dict[str, Any]:
    kind = model.get("kind")
    if kind == "scalar":
        return scalar_layout(model.get("name", ""), pointer_bytes)
    if kind == "array":
        element = compute_type_layout(model.get("element", {}), pointer_bytes, maximum)
        count = model.get("count")
        require(isinstance(count, int) and not isinstance(count, bool) and count >= 0, "array count must be non-negative")
        stride = align_up(element["size_bytes"], element["alignment_bytes"], maximum)
        require(count == 0 or stride <= maximum // count, "backend.runtime_lowering_required")
        require(
            count <= MAX_MATERIALIZED_LAYOUT_FIELDS,
            "array layout exceeds the bounded inspection field limit",
        )
        return {
            "size_bytes": stride * count,
            "alignment_bytes": element["alignment_bytes"],
            "field_offsets_bytes": [stride * index for index in range(count)],
        }
    if kind in {"struct", "tuple"}:
        field_name = "fields" if kind == "struct" else "elements"
        fields = model.get(field_name)
        require(isinstance(fields, list) and fields, f"{kind} model requires {field_name}")
        require(
            len(fields) <= MAX_MATERIALIZED_LAYOUT_FIELDS,
            f"{kind} layout exceeds the bounded inspection field limit",
        )
        offset = 0
        alignment = 1
        offsets: list[int] = []
        for field in fields:
            field_layout = compute_type_layout(field, pointer_bytes, maximum)
            offset = align_up(offset, field_layout["alignment_bytes"], maximum)
            offsets.append(offset)
            require(offset <= maximum - field_layout["size_bytes"], "backend.runtime_lowering_required")
            offset += field_layout["size_bytes"]
            alignment = max(alignment, field_layout["alignment_bytes"])
        return {
            "size_bytes": align_up(offset, alignment, maximum),
            "alignment_bytes": alignment,
            "field_offsets_bytes": offsets,
        }
    if kind == "option":
        payload = model.get("payload")
        require(isinstance(payload, dict), "option model requires a payload")
        return compute_type_layout(
            {"kind": "enum", "variants": [None, payload]},
            pointer_bytes,
            maximum,
        )
    if kind == "result":
        ok = model.get("ok")
        error = model.get("error")
        require(isinstance(ok, dict) and isinstance(error, dict), "result model requires ok and error payloads")
        return compute_type_layout(
            {"kind": "enum", "variants": [ok, error]},
            pointer_bytes,
            maximum,
        )
    if kind == "enum":
        variants = model.get("variants")
        require(isinstance(variants, list) and variants, "enum model requires variants")
        require(
            len(variants) <= MAX_MATERIALIZED_LAYOUT_FIELDS,
            "enum layout exceeds the bounded inspection field limit",
        )
        max_ordinal = len(variants) - 1
        width = next((candidate for candidate in (1, 2, 4, 8) if max_ordinal < 1 << (candidate * 8)), None)
        require(width is not None, "backend.runtime_lowering_required")
        payload_layouts = [
            compute_type_layout(variant, pointer_bytes, maximum)
            if variant is not None
            else {"size_bytes": 0, "alignment_bytes": 1, "field_offsets_bytes": []}
            for variant in variants
        ]
        payload_size = max(item["size_bytes"] for item in payload_layouts)
        payload_alignment = max(item["alignment_bytes"] for item in payload_layouts)
        payload_offset = align_up(width, payload_alignment, maximum)
        require(payload_offset <= maximum - payload_size, "backend.runtime_lowering_required")
        alignment = max(width, payload_alignment)
        size = align_up(payload_offset + payload_size, alignment, maximum)
        widest = max(payload_layouts, key=lambda item: (item["size_bytes"], item["alignment_bytes"]))
        return {
            "size_bytes": size,
            "alignment_bytes": alignment,
            "field_offsets_bytes": [
                payload_offset + field_offset
                for field_offset in widest["field_offsets_bytes"]
            ],
            "discriminant_width_bytes": width,
            "discriminant_offset_bytes": 0,
            "payload_offset_bytes": payload_offset,
            "variant_field_offsets_bytes": [
                {
                    "variant_ordinal": ordinal,
                    "field_offsets_bytes": [
                        payload_offset + field_offset
                        for field_offset in variant_layout["field_offsets_bytes"]
                    ],
                }
                for ordinal, variant_layout in enumerate(payload_layouts)
            ],
        }
    raise ContractError(f"unsupported layout kind: {kind}")


def compute_layout_record(case: dict[str, Any]) -> dict[str, Any]:
    target = case.get("target")
    require(isinstance(target, dict), "layout case requires a target")
    require(set(target) == {"abi_profile", "byte_order", "pointer_width_bits", "target_triple"}, "layout target fields drifted")
    profile_key = (target["target_triple"], target["abi_profile"])
    require(profile_key in TARGET_PROFILES, "unsupported target ABI profile")
    profile = TARGET_PROFILES[profile_key]
    require(
        target["pointer_width_bits"] == profile["pointer_width_bits"]
        and target["byte_order"] == profile["byte_order"],
        "target ABI profile inputs are inconsistent",
    )
    pointer_width = profile["pointer_width_bits"]
    pointer_bytes = pointer_width // 8
    maximum = (1 << pointer_width) - 1
    layout = compute_type_layout(case.get("type_model", {}), pointer_bytes, maximum)
    direct = layout["size_bytes"] <= pointer_bytes * 2 and layout["alignment_bytes"] <= pointer_bytes
    record = {
        **target,
        "layout_id": case.get("layout_id"),
        "size_bytes": layout["size_bytes"],
        "alignment_bytes": layout["alignment_bytes"],
        "discriminant_width_bytes": layout.get("discriminant_width_bytes"),
        "discriminant_offset_bytes": layout.get("discriminant_offset_bytes"),
        "payload_offset_bytes": layout.get("payload_offset_bytes"),
        "field_offsets_bytes": layout["field_offsets_bytes"],
        "argument_passing": "direct_value" if direct else "indirect_pointer",
        "return_passing": "direct_value" if direct else "caller_provided_storage",
    }
    if "variant_field_offsets_bytes" in layout:
        record["variant_field_offsets_bytes"] = layout["variant_field_offsets_bytes"]
    return record


def execute_lifecycle(events: list[dict[str, Any]]) -> dict[str, Any]:
    values: dict[str, dict[str, Any]] = {}
    creation_order: list[str] = []
    drop_order: list[str] = []

    def live(name: str) -> dict[str, Any]:
        require(name in values and values[name]["state"] == "live", "use_after_move")
        return values[name]

    try:
        for event_index, event in enumerate(events):
            require(isinstance(event, dict) and isinstance(event.get("op"), str), "lifecycle event is invalid")
            op = event["op"]
            name = event.get("value")
            if op == "create":
                require(isinstance(name, str) and name not in values, "ownership.duplicate_value")
                values[name] = {"state": "live", "shared": set(), "mutable": None}
                creation_order.append(name)
            elif op == "clone":
                source = live(event.get("source", ""))
                require(source["mutable"] is None, "ownership.clone_while_mutably_borrowed")
                target = event.get("target")
                require(isinstance(target, str) and target not in values, "ownership.duplicate_value")
                values[target] = {"state": "live", "shared": set(), "mutable": None}
                creation_order.append(target)
            elif op == "move":
                source_name = event.get("source", "")
                source = live(source_name)
                require(not source["shared"] and source["mutable"] is None, "ownership.move_while_borrowed")
                target = event.get("target")
                require(isinstance(target, str) and target not in values, "ownership.duplicate_value")
                source["state"] = "moved"
                values[target] = {"state": "live", "shared": set(), "mutable": None}
                creation_order.append(target)
            elif op in {"use", "mutate"}:
                value = live(name or "")
                if op == "use":
                    require(value["mutable"] is None, "ownership.use_while_mutably_borrowed")
                else:
                    require(not value["shared"] and value["mutable"] is None, "ownership.mutation_while_borrowed")
            elif op in {"borrow_shared", "borrow_mut"}:
                value = live(name or "")
                borrow_id = event.get("borrow")
                require(isinstance(borrow_id, str) and borrow_id, "ownership.borrow_id_required")
                if op == "borrow_shared":
                    require(value["mutable"] is None, "ownership.borrow_conflict")
                    value["shared"].add(borrow_id)
                else:
                    require(value["mutable"] is None and not value["shared"], "ownership.borrow_conflict")
                    value["mutable"] = borrow_id
            elif op == "end_borrow":
                value = live(name or "")
                borrow_id = event.get("borrow")
                if value["mutable"] == borrow_id:
                    value["mutable"] = None
                else:
                    require(borrow_id in value["shared"], "ownership.unknown_borrow")
                    value["shared"].remove(borrow_id)
            elif op == "drop":
                require(isinstance(name, str) and name in values, "use_after_move")
                require(values[name]["state"] != "dropped", "ownership.double_drop")
                value = live(name)
                require(not value["shared"] and value["mutable"] is None, "ownership.drop_while_borrowed")
                value["state"] = "dropped"
                drop_order.append(name)
            elif op == "early_exit":
                require(event_index == len(events) - 1, "ownership.events_after_exit")
                for candidate in reversed(creation_order):
                    value = values[candidate]
                    if value["state"] == "live":
                        require(not value["shared"] and value["mutable"] is None, "ownership.exit_with_live_borrow")
                        value["state"] = "dropped"
                        drop_order.append(candidate)
            else:
                raise ContractError(f"unsupported lifecycle operation: {op}")
    except ContractError as error:
        return {"outcome": "rejected", "diagnostic": str(error), "drop_order": drop_order}
    undischarged = [name for name, value in values.items() if value["state"] == "live"]
    require(not undischarged, f"undischarged cleanup obligations: {','.join(undischarged)}")
    return {"outcome": "accepted", "diagnostic": "", "drop_order": drop_order}


def safe_program(root: Path, relative: Any) -> Path:
    require(isinstance(relative, str) and re.fullmatch(r"[a-z0-9-]+", relative) is not None, "fixture program path is invalid")
    program_relative = PROGRAMS / relative
    path = resolve_checked_path(root, program_relative, directory=True)
    required_files = {"axiom.lock", "axiom.toml", "src/main.ax"}
    actual_files: set[str] = set()
    for candidate in path.rglob("*"):
        nested = candidate.relative_to(path)
        require(not candidate.is_symlink(), f"fixture program {relative} uses a symlink: {nested}")
        if candidate.is_dir():
            continue
        require(candidate.is_file(), f"fixture program {relative} has a non-regular entry: {nested}")
        actual_files.add(nested.as_posix())
    require(actual_files == required_files, f"fixture program {relative} file set drifted")
    for required in required_files:
        resolve_checked_path(root, program_relative / required)
    require(relative in PROGRAM_DIGESTS, f"fixture program {relative} is not governed")
    digest = hashlib.sha256()
    for required in sorted(required_files):
        digest.update(required.encode("utf-8"))
        digest.update(b"\0")
        digest.update(resolve_checked_path(root, program_relative / required).read_bytes())
        digest.update(b"\0")
    require(
        digest.hexdigest() == PROGRAM_DIGESTS[relative],
        f"fixture program {relative} source digest drifted",
    )
    return path


def validate_fixture(root: Path, name: str, fixture: dict[str, Any], expected_kind: str) -> None:
    required = {"schema_version", "id", "kind", "scenario", "shape", "runtime_origin", "function_boundaries", "evidence"}
    require(set(fixture) == required, f"fixture {name} fields drifted")
    require(fixture["schema_version"] == "axiom.dynamic_aggregate_abi.fixture.v1", f"fixture {name} schema drifted")
    require(fixture["id"] == f"dynamic-aggregate-abi-v1/{name}", f"fixture {name} id drifted")
    require(fixture["kind"] == expected_kind, f"fixture {name} kind drifted")
    require(isinstance(fixture["scenario"], str) and fixture["scenario"], f"fixture {name} scenario is empty")
    require(isinstance(fixture["runtime_origin"], bool), f"fixture {name} runtime origin must be boolean")
    require(isinstance(fixture["function_boundaries"], int) and fixture["function_boundaries"] >= 0, f"fixture {name} boundary count is invalid")
    require(isinstance(fixture["shape"], list) and fixture["shape"], f"fixture {name} shape is empty")
    require_sorted_unique(fixture["shape"], f"fixture {name} shape")
    evidence = fixture["evidence"]
    require(isinstance(evidence, dict) and evidence.get("mode") in EVIDENCE_MODES, f"fixture {name} evidence mode is invalid")
    mode = evidence["mode"]
    if mode in {"native_execution", "compiler_check", "compiler_build_diagnostic"}:
        require(set(evidence) == {"mode", "program", "program_sha256", "expected"}, f"fixture {name} compiler evidence fields drifted")
        safe_program(root, evidence["program"])
        require(
            evidence["program_sha256"] == PROGRAM_DIGESTS[evidence["program"]],
            f"fixture {name} source provenance drifted",
        )
        require(isinstance(evidence["expected"], dict), f"fixture {name} expected compiler evidence is invalid")
    elif mode == "layout_model":
        require(set(evidence) == {"mode", "case", "expected"}, f"fixture {name} layout evidence fields drifted")
        expected = evidence["expected"]
        require(isinstance(expected, dict) and expected.get("outcome") in {"accepted", "rejected"}, f"fixture {name} layout outcome is invalid")
        try:
            record = compute_layout_record(evidence["case"])
        except ContractError as error:
            require(expected == {"outcome": "rejected", "diagnostic": str(error)}, f"fixture {name} layout rejection drifted")
        else:
            require(expected == {"outcome": "accepted", "record": record}, f"fixture {name} layout record drifted")
    elif mode == "lifecycle_model":
        require(set(evidence) == {"mode", "events", "expected"}, f"fixture {name} lifecycle evidence fields drifted")
        result = execute_lifecycle(evidence["events"])
        require(evidence["expected"] == result, f"fixture {name} lifecycle result drifted")
    elif mode == "target_gap":
        require(set(evidence) == {"mode", "gap", "reason"}, f"fixture {name} target-gap fields drifted")
        require(evidence["gap"] in TARGET_GAPS and isinstance(evidence["reason"], str) and evidence["reason"], f"fixture {name} target gap is invalid")
    elif mode == "contract_rejection":
        require(set(evidence) == {"mode", "diagnostic", "claim"}, f"fixture {name} contract rejection fields drifted")
        require(evidence["diagnostic"] == "evidence.tier_mismatch", f"fixture {name} contract diagnostic drifted")


def validate_contract(root: Path) -> dict[str, Any]:
    root = root.resolve(strict=True)
    schema_path = resolve_checked_path(root, SCHEMA)
    schema_bytes = schema_path.read_bytes()
    require(hashlib.sha256(schema_bytes).hexdigest() == SCHEMA_SHA256, "trusted schema digest drifted")
    snapshot_path = resolve_checked_path(root, SNAPSHOT)
    snapshot_bytes = snapshot_path.read_bytes()
    require(hashlib.sha256(snapshot_bytes).hexdigest() == SNAPSHOT_SHA256, "trusted snapshot digest drifted")
    schema = load(schema_path)
    snapshot = load(snapshot_path)
    require(schema.get("$id", "").endswith("axiom.dynamic_aggregate_abi.v1.schema.json"), "schema id drift")
    validate_schema(snapshot, schema, "$", schema)
    require((snapshot["schema_version"], snapshot["contract"], snapshot["issue"]) == ("axiom.dynamic_aggregate_abi.v1", "runtime.dynamic_aggregate_abi", 1439), "contract identity drift")
    layout = snapshot["logical_layout"]
    require(layout["kinds"] == KINDS, "aggregate kinds drifted")
    require(layout["target_inputs"] == ["abi_profile", "byte_order", "pointer_width_bits", "target_triple"], "target layout inputs drifted")
    require(layout["alignment_rule"] == "align_up(current_offset, field_alignment); final_size=align_up(end, aggregate_alignment)", "alignment algorithm drifted")
    require(layout["enum_rule"] == "smallest_unsigned_1_2_4_8_byte_source_ordinal_tag_then_aligned_max_variant_payload", "enum rule drifted")
    require(layout["passing"]["direct_limit_pointer_words"] == 2, "passing threshold drifted")
    require(layout["passing"]["selection"] == "size_bytes<=2*pointer_bytes_and_alignment_bytes<=pointer_bytes", "passing selection drifted")
    require_sorted_unique(layout["prohibited_capture"], "prohibited capture")
    require(snapshot["boundary"]["operations"] == OPERATIONS, "boundary operations drifted")
    require(snapshot["boundary"]["minimum_function_boundaries"] == 3, "three-boundary requirement drifted")
    require(snapshot["ownership"]["transition_model"] == "axiom.dynamic_aggregate_ownership.v1", "ownership transition model drifted")
    require(snapshot["ownership"]["exactly_once"] is True, "ownership cleanup must be exactly once")
    require(snapshot["ownership"]["runtime_target_gaps"] == TARGET_GAPS, "runtime ownership target gaps drifted")
    require(snapshot["backend"]["fail_closed"] is True, "backend must fail closed")
    require(snapshot["backend"]["unsupported_diagnostic"] == "backend.runtime_lowering_required", "unsupported diagnostic drifted")
    floor = snapshot["current_floor"]
    require(floor["tier"] == "static_spike" and floor["status"] == "partial", "current evidence tier drifted")
    require(floor["supported"] == SUPPORTED and floor["unsupported"] == UNSUPPORTED, "current floor drifted")
    require(floor["direct_native"] is True and floor["generated_host_source"] is False, "current native evidence drifted")
    require(not any(floor[key] for key in ("runtime_origin_non_copy", "dynamic_storage", "recursive_cleanup", "static_projection_retired")), "current floor overclaims dynamic aggregate completion")
    require(snapshot["inspection_fields"] == INSPECTION_FIELDS, "inspection fields drifted")

    target_surface = {key: snapshot[key] for key in ("logical_layout", "boundary", "ownership", "backend", "inspection_fields")}
    target_words = set(re.findall(r"[a-z]+", json.dumps(target_surface).lower()))
    require(not (target_words & CAPTURE_WORDS), "target contract captured host implementation vocabulary")

    fixture_specs = snapshot["fixtures"]
    names = [spec["id"].rsplit("/", 1)[-1] for spec in fixture_specs]
    require(names == FIXTURE_NAMES, "fixture inventory must be complete and deterministically ordered")
    require(len({spec["path"] for spec in fixture_specs}) == len(fixture_specs), "fixture paths must be unique")
    for spec, name in zip(fixture_specs, names, strict=True):
        require(spec["path"] == f"{name}.json", f"fixture path drifted for {name}")
        fixture_path = resolve_checked_path(root, FIXTURES / spec["path"])
        fixture_bytes = fixture_path.read_bytes()
        require(
            hashlib.sha256(fixture_bytes).hexdigest() == FIXTURE_DIGESTS[spec["path"]],
            f"fixture {name} trusted digest drifted",
        )
        validate_fixture(root, name, load(fixture_path), spec["kind"])

    move_rows = [row for row in load_checked(root, RUNTIME_LEDGER).get("value_features", []) if row.get("id") == "owned.move_state"]
    require(len(move_rows) == 1 and move_rows[0].get("status") == "partial", "owned move-state floor is not partial")
    require("backend.runtime_lowering_required" in move_rows[0].get("notes", ""), "owned move-state row lost fail-closed evidence")
    readiness = load_checked(root, READINESS)
    readiness_row = next((row for row in readiness.get("rows", []) if row.get("governingIssue") == 1439), None)
    require(readiness_row is not None and readiness_row.get("currentTier") == "static_spike", "readiness tier for #1439 drifted")
    contract_doc = read_checked_text(root, CONTRACT_DOC)
    for statement in ("This tranche is partial", "not executable runtime proof", "caller_provided_storage", "target gap"):
        require(statement in contract_doc, f"contract documentation lost required qualification: {statement}")

    return {"schema": snapshot["schema_version"], "ok": True, "fixtures": len(fixture_specs), "inspection_fields": len(snapshot["inspection_fields"]), "supported_floor": len(floor["supported"])}


def parse_json_output(output: subprocess.CompletedProcess[str], label: str) -> dict[str, Any]:
    try:
        payload = json.loads(output.stdout)
    except json.JSONDecodeError as error:
        raise ContractError(f"{label} did not emit JSON: stdout={output.stdout!r} stderr={output.stderr!r}") from error
    require(isinstance(payload, dict), f"{label} JSON must be an object")
    return payload


def run_command(command: list[str], *, cwd: Path, env: dict[str, str], label: str, timeout: int = 180) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ContractError(f"{label} could not execute: {error}") from error


def execute_evidence(root: Path, *, evidence_root: Path | None = None, cargo_target_dir: Path | None = None) -> int:
    root = root.resolve()
    evidence_root = (evidence_root or root).resolve()
    cargo = shutil.which("cargo")
    require(cargo is not None, "cargo is required for executable ABI evidence")
    target = (cargo_target_dir or Path(os.environ.get("CARGO_TARGET_DIR", root / "target/dynamic-aggregate-abi-v1"))).resolve()
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target)
    build_compiler = run_command(
        [cargo, "build", "--locked", "--manifest-path", str(root / "stage1/Cargo.toml"), "-p", "axiomc", "--bin", "axiomc"],
        cwd=root,
        env=env,
        label="build axiomc executable evidence compiler",
        timeout=300,
    )
    require(build_compiler.returncode == 0, f"axiomc evidence compiler build failed: {build_compiler.stderr}")
    axiomc = target / "debug" / ("axiomc.exe" if os.name == "nt" else "axiomc")
    require(axiomc.is_file(), "built axiomc executable is missing")

    snapshot = load_checked(evidence_root, SNAPSHOT)
    executed = 0
    with tempfile.TemporaryDirectory(prefix="axiom-dynamic-aggregate-abi-") as temporary:
        temp = Path(temporary)
        for spec in snapshot["fixtures"]:
            name = spec["id"].rsplit("/", 1)[-1]
            fixture = load_checked(evidence_root, FIXTURES / spec["path"])
            evidence = fixture["evidence"]
            mode = evidence["mode"]
            if mode not in {"native_execution", "compiler_check", "compiler_build_diagnostic"}:
                continue
            source = safe_program(evidence_root, evidence["program"])
            project = temp / name
            shutil.copytree(source, project)
            if mode == "compiler_check":
                output = run_command([str(axiomc), "check", str(project), "--json"], cwd=root, env=env, label=name)
                payload = parse_json_output(output, name)
                expected = evidence["expected"]
                require(set(expected) == {"ok", "schema_version"}, f"fixture {name} check expectation fields drifted")
                require(output.returncode == 0 and payload.get("ok") is True, f"fixture {name} check failed: {output.stdout}{output.stderr}")
                require(payload.get("schema_version") == expected["schema_version"], f"fixture {name} check schema drifted")
            elif mode == "compiler_build_diagnostic":
                output = run_command([str(axiomc), "build", str(project), "--backend", "cranelift", "--json"], cwd=root, env=env, label=name)
                payload = parse_json_output(output, name)
                expected = evidence["expected"]
                require(set(expected) == {"code", "generated_rust", "kind", "ok"}, f"fixture {name} diagnostic expectation fields drifted")
                require(output.returncode != 0 and payload.get("ok") is False, f"fixture {name} unexpectedly built")
                error = payload.get("error", {})
                require(error.get("code") == expected["code"] and error.get("kind") == expected["kind"], f"fixture {name} diagnostic drifted")
                require(payload.get("generated_rust") == expected["generated_rust"], f"fixture {name} generated host-source drifted")
                require(payload.get("binary") is None, f"fixture {name} diagnostic unexpectedly emitted a binary")
            else:
                output = run_command([str(axiomc), "build", str(project), "--backend", "cranelift", "--json"], cwd=root, env=env, label=name)
                payload = parse_json_output(output, name)
                expected = evidence["expected"]
                require(set(expected) == {"backend", "exit_code", "generated_rust", "stderr", "stdout"}, f"fixture {name} native expectation fields drifted")
                require(output.returncode == 0 and payload.get("ok") is True, f"fixture {name} native build failed: {output.stdout}{output.stderr}")
                require(payload.get("backend") == expected["backend"] and payload.get("generated_rust") == expected["generated_rust"], f"fixture {name} native build envelope drifted")
                binary = payload.get("binary")
                require(isinstance(binary, str) and Path(binary).is_file(), f"fixture {name} native binary is missing")
                native = run_command([binary], cwd=project, env=env, label=f"{name} binary")
                require(native.returncode == expected["exit_code"], f"fixture {name} exit code drifted: {native.returncode}")
                require(native.stdout == expected["stdout"] and native.stderr == expected["stderr"], f"fixture {name} native output drifted")
            executed += 1
    require(executed >= 4, "executable fixture coverage is incomplete")
    return executed


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", "--checkout-root", dest="root", type=Path, default=ROOT)
    parser.add_argument("--execute", action="store_true", help="compile and run the governed executable evidence")
    parser.add_argument("--cargo-target-dir", type=Path)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
        if args.execute:
            result["executed_fixtures"] = execute_evidence(args.root, cargo_target_dir=args.cargo_target_dir)
    except (ContractError, KeyError, OSError, TypeError, ValueError) as error:
        if args.json:
            print(json.dumps({"error": str(error), "ok": False}, sort_keys=True))
        else:
            print(f"dynamic-aggregate-abi-v1: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "dynamic-aggregate-abi-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
