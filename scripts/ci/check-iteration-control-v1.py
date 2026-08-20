#!/usr/bin/env python3
"""Validate Iteration and Loop Control v1 without executing checkout code."""

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


ROOT = Path(__file__).resolve().parents[2]
MAX_READ_BYTES = 1024 * 1024
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.iteration_control.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/iteration-control-v1.json")
PRODUCTION_READINESS = Path("docs/production-language-readiness.json")
SYNTAX_SOURCE = Path("stage1/crates/axiomc/src/syntax.rs")
HIR_SOURCE = Path("stage1/crates/axiomc/src/hir.rs")
MIR_SOURCE = Path("stage1/crates/axiomc/src/mir.rs")
LIB_TESTS = Path("stage1/crates/axiomc/tests/support/lib_unit.rs")
FOR_FAILURE = Path("stage1/conformance/fail/for_loop_requires_iteration_protocol/expected-error.json")
RUNTIME_LOOPS = Path("stage1/conformance/pass/runtime_loop_bodies/src/main_test.ax")

DEPENDENCIES = [1425, 1437, 1440, 1441, 1476]
COLLECTION_KINDS = ["array", "map", "mutable_slice", "runtime_sequence", "slice", "text_scalars", "user_static_iterator"]
ITERATION_MODES = ["borrow_exclusive", "borrow_shared", "move"]
OPERATION_IDS = ["drop", "into-iter", "next"]
PROTOCOL_OPERATIONS = [
    {"id": "axiom://language/iteration/operation/drop", "input": "iterator state", "output": "unit", "semantics": "releases iterator state and remaining ownership obligations exactly once", "failure": "replaces non-error control transfer or attaches once as ordered secondary to propagated error or cancellation"},
    {"id": "axiom://language/iteration/operation/into-iter", "input": "one evaluated iterable source and iteration mode", "output": "iterator state", "semantics": "creates one iterator with collection kind ownership mode and deterministic order", "failure": "rejects unsupported source or mode before the loop body executes or the source changes"},
    {"id": "axiom://language/iteration/operation/next", "input": "live iterator state", "output": "option item", "semantics": "performs one pull and returns one item or permanent exhaustion without prefetch", "failure": "fallible iteration uses an explicit result-bearing item type"},
]
ORDER_RULES = ["array_slice_and_sequence_use_ascending_index_order", "map_uses_declared_deterministic_order_not_storage_order", "static_iterator_order_is_part_of_implementation_contract", "text_uses_unicode_scalar_order"]
OWNERSHIP_RULES = ["borrow_exclusive_yields_exclusive_element_borrow", "borrow_shared_yields_shared_element_borrow", "break_and_normal_end_drop_iterator_once", "continue_drops_iteration_binding_before_next", "move_consumes_source_and_yields_owned_elements", "non_copy_elements_are_never_silently_copied"]
MUTATION_RULES = ["exclusive_current_element_write_does_not_invalidate_iterator", "map_structural_mutation_is_rejected_while_iterator_live", "rejected_mutation_preserves_elements_length_capacity_order_and_generation", "relocating_or_resizing_source_is_rejected_while_borrowed", "shared_iteration_rejects_element_or_structure_mutation", "source_move_or_drop_while_borrowed_is_rejected_before_invalidation"]
LOOP_CONTROL_RULES = ["break_exits_nearest_enclosing_loop", "continue_advances_nearest_enclosing_iterator", "defer_cleanup_runs_on_every_exit_edge", "iteration_binding_scope_is_one_body_execution", "nested_loops_keep_independent_iterator_state", "source_expression_is_evaluated_once"]
TERMINAL_EDGE_RULES = ["break_drops_current_binding_then_iterator_once_and_exits_nearest_loop", "cancellation_remains_primary_and_attaches_cleanup_failures_in_observation_order", "cleanup_failure_replaces_normal_break_continue_or_return_outcome_with_error", "continue_drops_current_binding_before_advancing_same_iterator", "function_return_drops_current_binding_then_iterator_once_before_value_leaves_scope", "normal_end_drops_each_binding_and_iterator_once", "propagated_error_remains_primary_and_attaches_cleanup_failures_in_observation_order"]
SEMANTIC_NODES = ["For", "IteratorBegin", "IteratorNext", "LoopBreak", "LoopContinue", "LoopExit"]
DIAGNOSTICS = ["backend.unsupported_iteration_control", "iteration.cleanup_failure", "iteration.concurrent_mutation", "iteration.dynamic_dispatch_unsupported", "iteration.order_contract_missing", "iteration.source_not_iterable", "ownership.borrow_escape", "ownership.iterator_moved", "parse.for_iteration_protocol_unavailable"]
INSPECTION_FIELDS = ["collection_kind", "deterministic_order", "element_ownership", "iterator_id", "loop_control", "protocol_operation", "runtime_origin", "semantic_node_id", "source_provenance", "target_support"]
PROHIBITED_FALLBACKS = ["compile_time_collection_projection", "dynamic_dispatch_vtable", "generated_host_source", "hash_bucket_order", "host_iterator_layout", "public_while_desugaring", "silent_copy_of_non_copy_element", "static_fixture_substitution"]
RUNTIME_ORIGINS = ["environment", "file", "http", "prior_function_result", "stdin"]
TARGET_GAPS = ["array_iteration", "borrow_move_iteration", "build_once_run_many", "direct_native_for_runtime", "map_iteration", "mutation_rules", "protocol_types", "runtime_sequence_iteration", "semantic_ir_inspection", "slice_iteration", "static_user_iterator", "text_scalar_iteration"]
REQUIRED_PROOFS = ["all_collection_matrix", "borrow_move_and_drop_edges", "build_once_run_many_runtime_sensitivity", "deterministic_map_order", "explicit_semantic_ir_nodes", "invalid_mutation_move_borrow_and_dispatch_diagnostics", "nested_break_continue_and_cleanup", "runtime_origin_matrix", "static_user_iterator", "supported_target_matrix", "text_scalar_iteration"]
COMPLETION_FIELDS = ["for_syntax_proven", "protocol_types_proven", "array_iteration_proven", "slice_iteration_proven", "runtime_sequence_iteration_proven", "map_iteration_proven", "text_scalar_iteration_proven", "static_user_iterator_proven", "borrow_move_iteration_proven", "mutation_rules_proven", "semantic_ir_inspection_proven", "direct_native_for_runtime_proven", "build_once_run_many_proven"]
FIXTURE_NAMES = ["array-borrow-move", "bootstrap-break-continue", "bootstrap-for-fail-closed", "bootstrap-loop-control-negative", "bootstrap-runtime-while", "build-once-run-many", "direct-native-nested-for", "map-deterministic-order", "mutation-during-iteration", "protocol-types", "runtime-sequence", "semantic-ir-nodes", "slice-borrow-write-through", "static-user-iterator", "text-scalar-order", "unsupported-dynamic-dispatch"]
BOOTSTRAP_FIXTURES = {"bootstrap-break-continue", "bootstrap-for-fail-closed", "bootstrap-loop-control-negative", "bootstrap-runtime-while"}
RUNTIME_FIXTURES = {"array-borrow-move", "bootstrap-runtime-while", "build-once-run-many", "direct-native-nested-for", "map-deterministic-order", "mutation-during-iteration", "runtime-sequence", "slice-borrow-write-through", "text-scalar-order"}
TARGET_GAP_FIXTURES = [
    Path("stage1/compiler-contracts/fixtures/iteration-control-v1/control-exits.json"),
    Path("stage1/compiler-contracts/fixtures/iteration-control-v1/mutation-ownership.json"),
    Path("stage1/compiler-contracts/fixtures/iteration-control-v1/order-backpressure.json"),
    Path("stage1/compiler-contracts/fixtures/iteration-control-v1/runtime-receipt.json"),
]
TARGET_GAP_FIXTURE_SHA256 = {
    "stage1/compiler-contracts/fixtures/iteration-control-v1/control-exits.json": "6361e08b9e5963be7fddab589faa9713d574d032435883dbdbb5a2c34e6db192",
    "stage1/compiler-contracts/fixtures/iteration-control-v1/mutation-ownership.json": "27c7c18738e2b57d40c004e7e4b3522fdee8fd6696c3f362cdf74172fb6faf00",
    "stage1/compiler-contracts/fixtures/iteration-control-v1/order-backpressure.json": "fd373b3d03008b9a01369ce297a4674ccaa5415ecfc1ebb7f700d77eebe3cf84",
    "stage1/compiler-contracts/fixtures/iteration-control-v1/runtime-receipt.json": "cce1526b41d887297da4b359dc1530c7bbd7938bc8aa789294e7c4be85805262",
}
FLOW_CONTROL_BOUNDS = {
    "advance_requires_prior_item_release": True,
    "backpressure": "producer_does_not_advance_while_the_current_item_is_outstanding",
    "max_outstanding_items_per_iterator": 1,
    "max_prefetched_items_per_iterator": 0,
    "pull_model": "consumer_requests_exactly_one_next_operation_per_iteration_step",
}
RUNTIME_RECEIPT_CONTRACT = {
    "artifact_required_fields": ["artifact_id", "artifact_path", "compiler_identity", "sha256", "source_digest", "target"],
    "build_required_fields": ["artifact_sha256", "command", "exit_status", "finished_at", "sequence", "started_at"],
    "declarations_are_proof": False,
    "markers_are_proof": False,
    "minimum_post_build_runs": 2,
    "proof_executed": False,
    "proof_kind": "direct_native_runtime_receipt",
    "required_invariants": ["all_runs_reference_identical_artifact_hash", "artifact_hash_matches_built_bytes", "build_count_equals_one", "each_required_runtime_origin_is_observed_post_build", "every_run_records_inputs_outputs_and_exit_status", "minimum_two_post_build_runs", "no_build_event_occurs_after_first_run_starts", "runtime_inputs_differ_across_at_least_two_runs", "run_sequence_is_strictly_increasing"],
    "required_runtime_origins": RUNTIME_ORIGINS,
    "rejected_substitutions": ["compiler_known_value_substitution", "declaration_only_evidence", "marker_only_evidence", "per_run_rebuild", "static_fixture_substitution", "static_projection_of_runtime_collection"],
    "run_required_fields": ["artifact_sha256", "exit_status", "finished_at", "input_digest", "inputs", "rebuild_observed", "run_id", "runtime_origin", "sequence", "started_at", "stderr_digest", "stdout_digest"],
    "status": "required_unimplemented",
}
SOURCE_MARKERS = {
    SYNTAX_SOURCE: ["pub enum Stmt", "While {", "Break {", "Continue {", "stage1 bootstrap does not support `for` loops yet"],
    HIR_SOURCE: ["loop_ctx.loop_depth += 1", "break is only valid inside a while loop", "continue is only valid inside a while loop"],
    MIR_SOURCE: ["hir::Stmt::While", "hir::Stmt::Break", "hir::Stmt::Continue"],
    LIB_TESTS: ["build_project_supports_break_and_continue_in_while_loops", "check_project_rejects_loop_control_outside_while"],
    FOR_FAILURE: ["does not support `for` loops yet"],
    RUNTIME_LOOPS: ["fn edit_distance", "let view: &mut [int]"],
}
HOST_CAPTURE_TERMS = ("cargo", "cranelift", "rust", "rustc", "std::", "vec<")


class ContractError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def relative_components(relative: os.PathLike[str] | str) -> tuple[str, list[str]]:
    try:
        text = os.fspath(relative)
    except TypeError as error:
        raise ContractError(f"checkout path is not path-like: {relative!r}") from error
    require(isinstance(text, str), "checkout paths must be text")
    require("\x00" not in text, f"unsafe checkout path contains NUL: {text!r}")
    windows = PureWindowsPath(text)
    require(not PurePosixPath(text).is_absolute(), f"absolute checkout path is forbidden: {text!r}")
    require(not windows.is_absolute() and not windows.drive and not windows.root, f"Windows absolute checkout path is forbidden: {text!r}")
    require("\\" not in text, f"Windows checkout path separators are forbidden: {text!r}")
    components = text.split("/")
    require(all(component not in {"", ".", ".."} for component in components), f"unsafe checkout path component: {text!r}")
    return text, components


def safe_read_bytes(root: Path, relative: os.PathLike[str] | str) -> bytes:
    text, components = relative_components(relative)
    nofollow = getattr(os, "O_NOFOLLOW", None)
    nonblock = getattr(os, "O_NONBLOCK", None)
    directory = getattr(os, "O_DIRECTORY", None)
    require(nofollow is not None and nonblock is not None and directory is not None, "secure descriptor-relative reads are unsupported on this platform")
    common = os.O_RDONLY | nofollow | nonblock | getattr(os, "O_CLOEXEC", 0)
    dir_fd = -1
    file_fd = -1
    try:
        dir_fd = os.open(os.fspath(root), common | directory)
        require(stat.S_ISDIR(os.fstat(dir_fd).st_mode), f"checkout root is not a directory: {root}")
        for component in components[:-1]:
            next_fd = os.open(component, common | directory, dir_fd=dir_fd)
            require(stat.S_ISDIR(os.fstat(next_fd).st_mode), f"checkout path component is not a directory: {text!r}")
            os.close(dir_fd)
            dir_fd = next_fd
        file_fd = os.open(components[-1], common, dir_fd=dir_fd)
        metadata = os.fstat(file_fd)
        require(stat.S_ISREG(metadata.st_mode), f"checkout path is not a regular file: {text!r}")
        require(metadata.st_size <= MAX_READ_BYTES, f"checkout file exceeds {MAX_READ_BYTES} bytes: {text!r}")
        chunks: list[bytes] = []
        remaining = MAX_READ_BYTES + 1
        while remaining:
            chunk = os.read(file_fd, min(65536, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        payload = b"".join(chunks)
        require(len(payload) <= MAX_READ_BYTES, f"checkout file exceeds {MAX_READ_BYTES} bytes while reading: {text!r}")
        return payload
    except ContractError:
        raise
    except (OSError, ValueError) as error:
        raise ContractError(f"unable to read checkout file {text!r}: {error}") from error
    finally:
        if file_fd >= 0:
            try:
                os.close(file_fd)
            except OSError:
                pass
        if dir_fd >= 0:
            try:
                os.close(dir_fd)
            except OSError:
                pass


def safe_read_text(root: Path, relative: os.PathLike[str] | str) -> str:
    payload = safe_read_bytes(root, relative)
    try:
        return payload.decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise ContractError(f"checkout file is not valid UTF-8: {os.fspath(relative)!r}") from error


def load(root: Path, relative: os.PathLike[str] | str) -> Any:
    try:
        return json.loads(safe_read_text(root, relative))
    except json.JSONDecodeError as error:
        raise ContractError(f"unable to parse checkout JSON {os.fspath(relative)!r}: {error}") from error


def value_kind(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    return "object"


def validate_schema(value: Any, schema: dict[str, Any], path: str, root: dict[str, Any]) -> None:
    if "$ref" in schema:
        reference = schema["$ref"]
        require(reference.startswith("#/$defs/"), f"{path}: unsupported schema reference")
        name = reference.removeprefix("#/$defs/")
        require(name in root.get("$defs", {}), f"{path}: unknown schema reference")
        validate_schema(value, root["$defs"][name], path, root)
        return
    if "const" in schema:
        require(value == schema["const"], f"{path}: const mismatch")
    if "enum" in schema:
        require(value in schema["enum"], f"{path}: enum mismatch")
    if schema.get("type"):
        require(value_kind(value) == schema["type"], f"{path}: expected {schema['type']}")
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
        for index, item in enumerate(value):
            if "items" in schema:
                validate_schema(item, schema["items"], f"{path}[{index}]", root)
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: empty string")
        if "pattern" in schema:
            require(re.search(schema["pattern"], value) is not None, f"{path}: pattern mismatch")
    if isinstance(value, int) and not isinstance(value, bool) and "minimum" in schema:
        require(value >= schema["minimum"], f"{path}: below minimum")


def require_sorted_unique(values: list[str], label: str) -> None:
    require(values == sorted(set(values)), f"{label} must be sorted and unique")


def evidence_path(value: str) -> str:
    return value.split("::", 1)[0]


def validate_evidence(root: Path, value: str, label: str) -> None:
    path_text, separator, anchor = value.partition("::")
    source = safe_read_text(root, path_text)
    if separator:
        require(bool(anchor) and anchor in source, f"{label} evidence anchor is missing: {value}")


def strings(value: Any) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, list):
        return [item for nested in value for item in strings(nested)]
    if isinstance(value, dict):
        return [item for nested in value.values() for item in strings(nested)]
    return []


def reject_host_capture(value: Any) -> None:
    for text in strings(value):
        lowered = text.lower()
        for term in HOST_CAPTURE_TERMS:
            require(term not in lowered, f"iteration semantic contract captures host term: {term}")


def readiness_row(payload: dict[str, Any], row_id: str) -> dict[str, Any]:
    rows = {item["id"]: item for item in payload["rows"]}
    require(row_id in rows, f"{row_id} readiness row missing")
    return rows[row_id]


def validate_readiness(root: Path) -> None:
    required_evidence = {"docs/iteration-control-v1.md", "stage1/compiler-contracts/snapshots/iteration-control-v1.json"}
    production = readiness_row(load(root, PRODUCTION_READINESS), "iteration_control")
    require(production["governingIssue"] == 1442, "production governing issue drifted")
    require(production["currentTier"] == "syntax_only" and production["status"] == "blocked", "production readiness overclaims iteration control")
    require(production["blockerIssues"] == [1442], "production blocker drifted")
    require(production["dependencies"] == DEPENDENCIES, "production dependencies drifted")
    require(required_evidence <= set(production["evidence"]), "production iteration evidence missing")
    require("make stage1-iteration-control-v1-test" in production["validatingCommand"], "production mutation gate missing")
    require("make stage1-iteration-control-v1" in production["validatingCommand"], "production contract gate missing")


def validate_target_gap_fixtures(root: Path, schema: dict[str, Any], paths: list[str]) -> None:
    expected = [path.as_posix() for path in TARGET_GAP_FIXTURES]
    require(paths == expected, "target-gap fixture inventory drifted")
    identifiers: list[str] = []
    for path in paths:
        digest = hashlib.sha256(safe_read_bytes(root, path)).hexdigest()
        require(digest == TARGET_GAP_FIXTURE_SHA256[path], f"target-gap fixture content drifted: {path}")
        fixture = load(root, path)
        validate_schema(fixture, schema["$defs"]["targetGapFixture"], f"fixture:{path}", schema)
        require(fixture["status"] == "required_unimplemented" and fixture["executable_proof"] is False, f"target-gap fixture overclaims execution: {path}")
        requirement_ids = [item["id"] for item in fixture["requirements"]]
        negative_ids = [item["id"] for item in fixture["negative_cases"]]
        require_sorted_unique(requirement_ids, f"{path} requirements")
        require_sorted_unique(negative_ids, f"{path} negative cases")
        identifiers.append(fixture["id"])
    require(identifiers == sorted(set(identifiers)), "target-gap fixture IDs must be sorted and unique")


def validate_contract(root: Path) -> dict[str, Any]:
    schema, snapshot = load(root, SCHEMA), load(root, SNAPSHOT)
    require(schema.get("$id", "").endswith("axiom.iteration_control.v1.schema.json"), "schema id drifted")
    validate_schema(snapshot, schema, "$", schema)
    require((snapshot["schema_version"], snapshot["contract"], snapshot["issue"]) == ("axiom.iteration_control.v1", "language.iteration_control", 1442), "contract identity drifted")
    require(snapshot["ready"] is False, "iteration contract cannot be globally ready")
    require(snapshot["dependency_issues"] == DEPENDENCIES, "dependency inventory drifted")
    target = snapshot["target_contract"]
    exact = {
        "collection_kinds": COLLECTION_KINDS,
        "iteration_modes": ITERATION_MODES,
        "order_rules": ORDER_RULES,
        "ownership_rules": OWNERSHIP_RULES,
        "mutation_rules": MUTATION_RULES,
        "loop_control_rules": LOOP_CONTROL_RULES,
        "terminal_edge_rules": TERMINAL_EDGE_RULES,
        "semantic_nodes": SEMANTIC_NODES,
        "diagnostics": DIAGNOSTICS,
        "inspection_fields": INSPECTION_FIELDS,
        "prohibited_fallbacks": PROHIBITED_FALLBACKS,
    }
    for label, expected in exact.items():
        require(target[label] == expected, f"target {label} drifted")
        require_sorted_unique(target[label], f"target {label}")
    require(target["flow_control_bounds"] == FLOW_CONTROL_BOUNDS, "pull and backpressure bounds drifted")
    require(target["protocol_operations"] == PROTOCOL_OPERATIONS, "protocol operation semantics drifted")
    operation_ids = [operation["id"].rsplit("/", 1)[-1] for operation in target["protocol_operations"]]
    require(operation_ids == OPERATION_IDS, "protocol operations must be complete and sorted")
    capture = dict(target)
    capture.pop("prohibited_fallbacks")
    reject_host_capture(capture)
    floor = snapshot["current_floor"]
    positive = ["while_runtime_loops_present", "break_present", "continue_present", "loop_control_outside_while_rejected", "for_fails_closed"]
    require(all(floor[field] is True for field in positive), "bootstrap loop-control evidence disappeared")
    require(all(floor[field] is False for field in COMPLETION_FIELDS), "current floor overclaims iteration control")
    require(floor["target_gaps"] == TARGET_GAPS, "target gap inventory drifted")
    require_sorted_unique(floor["bootstrap_evidence"], "bootstrap evidence")
    for value in floor["bootstrap_evidence"]:
        validate_evidence(root, value, "bootstrap floor")
    qualification = snapshot["qualification"]
    require(qualification["minimum_nested_depth"] == 2, "minimum nested depth drifted")
    require(qualification["required_runtime_origins"] == RUNTIME_ORIGINS, "qualification origins drifted")
    require(qualification["required_collection_kinds"] == COLLECTION_KINDS, "qualification collections drifted")
    require(qualification["required_iteration_modes"] == ITERATION_MODES, "qualification modes drifted")
    require(qualification["required_proofs"] == REQUIRED_PROOFS, "qualification proofs drifted")
    require(qualification["readiness_promotable"] is False, "contract slice cannot promote readiness")
    require(snapshot["runtime_receipt_contract"] == RUNTIME_RECEIPT_CONTRACT, "runtime receipt contract drifted")
    validate_target_gap_fixtures(root, schema, snapshot["target_gap_fixture_paths"])
    fixtures = snapshot["fixtures"]
    names = [fixture["id"].rsplit("/", 1)[-1] for fixture in fixtures]
    require(names == FIXTURE_NAMES, "fixture inventory must be complete and sorted")
    for fixture, name in zip(fixtures, names, strict=True):
        bootstrap = name in BOOTSTRAP_FIXTURES
        require(fixture["status"] == ("bootstrap_pass" if bootstrap else "target_gap"), f"fixture status drifted for {name}")
        require(fixture["runtime_origin"] is (name in RUNTIME_FIXTURES), f"fixture runtime origin drifted for {name}")
        require(fixture["blocks_readiness"] is (not bootstrap), f"fixture readiness drifted for {name}")
        for value in fixture["evidence"]:
            validate_evidence(root, value, f"fixture {name}")
    for path, markers in SOURCE_MARKERS.items():
        source = safe_read_text(root, path)
        for marker in markers:
            require(marker in source, f"bootstrap iteration marker missing in {path}: {marker}")
    validate_readiness(root)
    return {"bootstrap_pass": len(BOOTSTRAP_FIXTURES), "fixtures": len(fixtures), "ok": True, "readiness_promotable": False, "ready": False, "schema": snapshot["schema_version"], "target_gap_fixtures": len(TARGET_GAP_FIXTURES), "target_gaps": len(fixtures) - len(BOOTSTRAP_FIXTURES)}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
    except (ContractError, KeyError, TypeError, AttributeError) as error:
        print(json.dumps({"error": str(error), "ok": False}, sort_keys=True) if args.json else f"iteration-control-v1: {error}", file=sys.stdout if args.json else sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "iteration-control-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
