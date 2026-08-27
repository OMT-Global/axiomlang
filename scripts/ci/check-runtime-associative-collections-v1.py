#!/usr/bin/env python3
"""Validate the target-neutral Runtime Associative Collections v1 contract."""
import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "stage1/compiler-contracts/schemas/axiom.runtime_associative_collections.v1.schema.json"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json"
CAPTURE = {"rust", "cargo", "cranelift", "hashmap", "hashset", "host address", "instruction stream"}

COLLECTIONS = {
    "map": ["construct", "lookup", "insert_or_replace", "remove", "clear", "len", "capacity", "iterate"],
    "set": ["construct", "contains", "insert", "remove", "clear", "len", "capacity", "iterate"],
    "replacement": "one_value_per_equal_key",
}
KEY_EQUALITY = {
    "mode": "total_deterministic_axiom_semantics",
    "cross_type": "never_equal",
    "primitive": "same_type_and_value",
    "text": "same_unicode_scalar_sequence",
    "tuple": "same_arity_and_recursive_component_equality",
    "enum": "same_declared_type_variant_and_recursive_payload_equality",
    "user_defined": "same_declared_immutable_type_and_declaration_order_fields",
    "hash_compatibility": "equal_keys_same_hash",
}
KEYS = {
    "equality": KEY_EQUALITY,
    "accepted": ["primitive", "text", "tuple_recursive", "enum_recursive", "accepted_immutable_value_shape"],
    "rejected": ["float", "mutable", "resource", "function", "borrowed", "unaccepted_user_shape"],
}
HASHING = {
    "equal_keys": "same_stable_hash",
    "algorithm": "runtime_defined_versioned_stable",
    "host_seed": "forbidden_for_default_or_reproducible_output",
    "randomized_hardening": "explicit_opt_in_preserves_observable_order",
}
ITERATION = {
    "order": "insertion_order",
    "replace": "does_not_move_key",
    "remove_reinsert": "appends",
    "clear": "resets_order",
    "mutation": "generation_checked_fail_closed",
}
RESOURCES = {
    "collision": "separate_chaining_bounded_probes",
    "limits": ["entries", "bytes", "load_factor", "collision_chain"],
    "growth": "checked_before_mutation",
    "failure": "structured_error_no_partial_mutation",
}
OWNERSHIP = {
    "lookup": "borrowed_key_no_transfer",
    "insert": "owned_key_value_transfer",
    "iterator": "scoped_generation_checked",
    "nested": "clone_drop_exactly_once",
    "aliasing": "ownership_and_borrow_contract_enforced",
}
ERRORS = ["invalid_key_type", "allocation_failed", "collection_limit", "collision_limit", "concurrent_collection_mutation", "use_after_move", "borrow_violation"]
FIXTURES = [
    {"id": "map-runtime-origin-growth-replace-remove", "kind": "positive"},
    {"id": "set-runtime-origin-contains-remove-clear", "kind": "positive"},
    {"id": "deterministic-insertion-order", "kind": "positive"},
    {"id": "equal-keys-stable-hash", "kind": "positive"},
    {"id": "tuple-enum-user-value-key", "kind": "positive"},
    {"id": "invalid-float-resource-function-borrowed-key", "kind": "negative"},
    {"id": "mutation-during-iteration", "kind": "negative"},
    {"id": "use-after-move-and-alias-borrow", "kind": "negative"},
    {"id": "allocation-and-limit-failure-no-partial-mutation", "kind": "resource"},
    {"id": "adversarial-collision-bounded", "kind": "adversarial"},
    {"id": "nested-clone-drop-cleanup", "kind": "lifecycle"},
    {"id": "compiler-symbol-table-determinism", "kind": "compiler-proof"},
]
MIGRATION = {
    "semantic_input": "Axiom semantic collection operations and lifecycle provenance",
    "dependencies": [1425, 1437, 1438, 1440],
}


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def main() -> None:
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    snapshot: dict[str, Any] = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
    required = {"schema_version", "contract", "issue", "status", "collections", "keys", "hashing", "iteration", "resources", "ownership", "errors", "fixtures", "migration"}
    require(schema["type"] == "object" and schema["additionalProperties"] is False, "schema envelope drift")
    require(set(schema["required"]) == required, "schema required surface drift")
    require((snapshot["schema_version"], snapshot["contract"], snapshot["issue"], snapshot["status"]) == ("axiom.runtime-associative-collections.v1", "runtime.associative_collections", 1476, "contract_only"), "contract identity drifted")
    require(snapshot["collections"] == COLLECTIONS, "collection operation contract drifted")
    require(snapshot["keys"] == KEYS, "key shape or equality contract drifted")
    require(snapshot["hashing"] == HASHING, "hashing contract drifted")
    require(snapshot["iteration"] == ITERATION, "iteration contract drifted")
    require(snapshot["resources"] == RESOURCES, "collision or resource-bound contract drifted")
    require(snapshot["ownership"] == OWNERSHIP, "ownership contract drifted")
    require(snapshot["errors"] == ERRORS, "error coverage drifted")
    require(snapshot["fixtures"] == FIXTURES, "fixture coverage or classification drifted")
    require(snapshot["migration"]["semantic_input"] == MIGRATION["semantic_input"], "migration input drifted")
    require(snapshot["migration"]["dependencies"] == MIGRATION["dependencies"], "dependency boundary drifted")
    surface_value = dict(snapshot)
    surface_value["migration"] = dict(snapshot["migration"])
    surface_value["migration"]["forbidden_terms"] = []
    surface = json.dumps(surface_value, sort_keys=True).lower()
    require(not any(term in surface for term in CAPTURE), "contract leaks a host implementation term")
    print(json.dumps({"schema": snapshot["schema_version"], "ok": True, "operations": 16, "fixtures": len(FIXTURES)}))


if __name__ == "__main__":
    main()
