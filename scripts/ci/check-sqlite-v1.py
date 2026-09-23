#!/usr/bin/env python3
"""Validate the bounded SQLite v1 authority and lifecycle contract."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any

from json_schema_v1 import validate_draft_2020_12


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_SCHEMA = (
    REPO_ROOT
    / "stage1/compiler-contracts/schemas/axiom.sqlite-v1.schema.json"
)
DEFAULT_CONTRACT = (
    REPO_ROOT / "stage1/compiler-contracts/snapshots/sqlite-v1.json"
)
SCHEMA_SEMANTIC_SHA256 = "eeced40276711f3d4e2e196c18ed7914475372a3c17f32111a1f0a7d4f832237"

OUTER_FIELDS = {
    "schema_version",
    "contract",
    "issue",
    "scope",
    "authority",
    "statements",
    "transactions",
    "migrations",
    "concurrency",
    "lifecycle",
    "errors",
    "audit",
    "fixtures",
}

SECTION_FIELDS = {
    "scope": {"database", "provider_boundary", "qualification", "raw_surface"},
    "authority": {
        "capability",
        "identity",
        "path_policy",
        "uri_policy",
        "modes",
        "mode_authority",
        "open_strategy",
        "checks",
    },
    "statements": {
        "lifecycle",
        "sql_source",
        "batching",
        "parameters",
        "rows",
        "injection",
    },
    "transactions": {
        "top_level",
        "begin_modes",
        "nested",
        "savepoint_names",
        "savepoints",
        "drop",
        "failure",
        "parent_failure",
    },
    "migrations": {
        "artifact",
        "dialect",
        "identity",
        "ordering",
        "apply",
        "history",
        "divergence",
    },
    "concurrency": {
        "busy",
        "timeout",
        "cancellation",
        "writer_policy",
        "crash_reopen",
        "corruption",
    },
    "lifecycle": {
        "handles",
        "representation",
        "close",
        "drop",
        "validation",
        "leaks",
    },
    "errors": {"shape", "identity_policy", "kinds", "message_policy"},
    "audit": {"fields", "operation_classes", "forbidden"},
}

EXPECTED = {
    "scope": {
        "database": "sqlite",
        "provider_boundary": "provider_abi_v1_or_builtin_equivalent",
        "qualification": "contract_only",
        "raw_surface": "forbidden",
    },
    "authority": {
        "capability": "database.sqlite",
        "identity": "sha256_of_canonical_authority_and_database_file_identity",
        "path_policy": "project_relative_anchored_no_symlink_escape",
        "uri_policy": {
            "allowed": ["file"],
            "path": "canonical_path_only",
            "query": "forbidden",
            "authority": "forbidden",
            "fragment": "forbidden",
            "percent_decoding": "decode_once_before_normalize_and_anchor",
            "mode_source": "explicit_open_argument_only",
            "ambient": "denied",
            "network": "denied",
        },
        "modes": ["read_only", "read_write", "create"],
        "mode_authority": {
            "read_only": "requires_explicit_read_authority",
            "read_write": "requires_explicit_read_and_write_authority",
            "create": "requires_explicit_read_write_and_create_authority",
        },
        "open_strategy": {
            "existing": (
                "handle_relative_or_revalidate_opened_file_identity_against_pre_authorized_identity"
            ),
            "create": "handle_relative_prevalidated_parent_no_follow_exclusive_leaf_create",
        },
        "checks": [
            "decode_uri_path",
            "normalize",
            "anchor",
            "no_symlink_escape",
            "regular_file_or_new",
            "mode",
            "mode_specific_open_strategy",
        ],
    },
    "statements": {
        "lifecycle": ["prepare", "bind", "step", "reset", "finalize"],
        "sql_source": "prepared_statement_text_only",
        "batching": "one_statement_per_prepare",
        "parameters": {
            "addressing": ["index", "name"],
            "types": ["null", "bool", "int64", "float64", "text_utf8", "bytes"],
            "limits": "validated_before_copy_or_provider_dispatch",
            "unknown": "reject_before_dispatch_with_bind_address",
            "missing_binding": (
                "reject_step_with_bind_missing_unless_every_placeholder_is_bound_including_explicit_null"
            ),
            "reset_binding": "clear_all_bindings_and_row_state",
        },
        "rows": {
            "iteration": "forward_cursor_bounded",
            "lifetime": "valid_until_next_step_reset_or_finalize",
            "column_lookup": ["index", "name"],
            "types": ["null", "bool", "int64", "float64", "text_utf8", "bytes"],
            "coercion": "explicit_only",
            "limits": "validated_before_allocation_or_copy",
        },
        "injection": "bound_values_never_reparsed_as_sql",
    },
    "transactions": {
        "top_level": ["begin", "commit", "rollback"],
        "begin_modes": ["deferred", "immediate", "exclusive"],
        "nested": "savepoint_only",
        "savepoint_names": "generated_or_validated_identifier_never_sql",
        "savepoints": {
            "create": "opens_active_child_scope",
            "release": "terminal_commit_child_into_parent_only_when_unpoisoned",
            "rollback_to": "rollback_child_changes_clear_poison_keep_child_active",
            "rollback_to_and_release": "terminal_rollback_child",
            "drop": "terminal_rollback_child",
        },
        "drop": "rollback_active_scope",
        "failure": "poison_current_scope_until_rollback",
        "parent_failure": (
            "child_failure_does_not_poison_parent_after_terminal_child_rollback"
        ),
    },
    "migrations": {
        "artifact": "sql_migration",
        "dialect": "sqlite",
        "identity": "artifact_digest_plus_provenance_digest",
        "ordering": "strict_monotonic_sequence",
        "apply": "exclusive_transaction_all_or_rollback",
        "history": "immutable_id_digest_and_applied_version",
        "divergence": "fail_closed_before_new_migration",
    },
    "concurrency": {
        "busy": "bounded_retry_with_stable_busy_timeout",
        "timeout": "monotonic_operation_deadline",
        "cancellation": "interrupt_then_drain_before_reuse",
        "writer_policy": "explicit_single_writer_contention",
        "crash_reopen": "committed_state_or_recovery_error_never_partial_success",
        "corruption": "stable_corruption_error_without_automatic_repair",
    },
    "lifecycle": {
        "handles": [
            "database",
            "statement",
            "row_cursor",
            "transaction",
            "savepoint",
        ],
        "representation": "nonzero_provider_scoped_generation_tagged_u64",
        "close": "idempotent_invalidates_children",
        "drop": "finalize_children_rollback_active_then_close",
        "validation": ["provider", "kind", "generation", "open", "parent"],
        "leaks": "fail_qualification_on_live_handle_or_unfinalized_statement",
    },
    "errors": {
        "shape": [
            "kind",
            "operation",
            "request_identity",
            "authority_identity",
            "database_identity",
            "message",
            "retryable",
        ],
        "identity_policy": {
            "request_identity": "opaque_runtime_issued_before_authority_resolution",
            "authority_identity": "opaque_runtime_issued_after_authority_resolution",
            "database_identity": (
                "nullable_until_file_identity_established_required_after_establishment"
            ),
            "null_authority_identity_kinds": ["capability_denied", "invalid_authority"],
            "nullable_database_identity_kinds": [
                "capability_denied",
                "invalid_authority",
                "open_failed",
            ],
        },
        "kinds": [
            "capability_denied",
            "invalid_authority",
            "open_failed",
            "prepare_failed",
            "bind_address",
            "bind_missing",
            "bind_type",
            "execute_failed",
            "busy_timeout",
            "cancelled",
            "row_type",
            "transaction_state",
            "migration_diverged",
            "corruption",
            "closed_handle",
            "provider_fault",
        ],
        "message_policy": "redacted_bounded_and_stable_kind_authoritative",
    },
    "audit": {
        "fields": [
            "request_identity",
            "authority_identity",
            "database_identity",
            "operation_class",
            "decision",
            "transaction_depth",
            "migration_id",
            "error_kind",
        ],
        "operation_classes": [
            "open",
            "prepare",
            "read",
            "write",
            "transaction",
            "migration",
            "close",
            "recovery",
        ],
        "forbidden": [
            "sql_text",
            "parameter_names",
            "parameter_values",
            "row_values",
            "credentials",
            "raw_path",
            "raw_uri",
            "raw_handle",
        ],
    },
}

FIXTURES = {
    "prepared-bind-round-trip": "positive",
    "unknown-bind-address": "negative",
    "missing-parameter-binding": "negative",
    "unsupported-bind-type": "negative",
    "reset-clears-bindings": "negative",
    "capability-path-uri-denial": "negative",
    "bound-value-injection": "negative",
    "typed-row-mismatch": "negative",
    "rollback-and-savepoint": "positive",
    "savepoint-state-transitions": "positive",
    "crash-reopen-committed-state": "recovery",
    "concurrent-busy-cancel": "concurrency",
    "corrupt-database": "negative",
    "migration-divergence": "negative",
    "statement-transaction-handle-leak": "leak",
}


class ContractError(ValueError):
    """A stable checker failure suitable for CI output."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def exact(actual: Any, expected: Any, message: str) -> None:
    require(actual == expected, message)


def load_object(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot read {label} {path}: {error}") from error
    require(isinstance(value, dict), f"{label} must be a JSON object")
    return value


def validate_schema(schema: dict[str, Any]) -> None:
    semantic_digest = hashlib.sha256(
        json.dumps(schema, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    exact(
        semantic_digest,
        SCHEMA_SEMANTIC_SHA256,
        "schema semantic constraints drift",
    )
    exact(schema.get("type"), "object", "schema envelope must be an object")
    exact(
        schema.get("additionalProperties"),
        False,
        "schema envelope permits unknown fields",
    )
    exact(set(schema.get("required", [])), OUTER_FIELDS, "schema required fields drift")
    properties = schema.get("properties")
    require(isinstance(properties, dict), "schema properties must be an object")
    exact(set(properties), OUTER_FIELDS, "schema property fields drift")
    exact(
        properties.get("schema_version", {}).get("const"),
        "axiom.sqlite-v1",
        "schema version drift",
    )
    exact(properties.get("contract", {}).get("const"), "runtime.sqlite", "contract drift")
    exact(properties.get("issue", {}).get("const"), 1452, "governing issue drift")
    for section, fields in SECTION_FIELDS.items():
        shape = properties.get(section)
        require(isinstance(shape, dict), f"schema section {section} must be an object")
        exact(shape.get("type"), "object", f"schema section {section} type drift")
        exact(
            shape.get("additionalProperties"),
            False,
            f"schema section {section} permits unknown fields",
        )
        exact(
            set(shape.get("required", [])),
            fields,
            f"schema section {section} required fields drift",
        )
        section_properties = shape.get("properties")
        require(
            isinstance(section_properties, dict),
            f"schema section {section} properties must be an object",
        )
        exact(
            set(section_properties),
            fields,
            f"schema section {section} property fields drift",
        )
    for section, fields in {
        "authority.uri_policy": {
            "allowed",
            "path",
            "query",
            "authority",
            "fragment",
            "percent_decoding",
            "mode_source",
            "ambient",
            "network",
        },
        "authority.mode_authority": {"read_only", "read_write", "create"},
        "authority.open_strategy": {"existing", "create"},
        "statements.parameters": {
            "addressing",
            "types",
            "limits",
            "unknown",
            "missing_binding",
            "reset_binding",
        },
        "statements.rows": {
            "iteration",
            "lifetime",
            "column_lookup",
            "types",
            "coercion",
            "limits",
        },
        "transactions.savepoints": {
            "create",
            "release",
            "rollback_to",
            "rollback_to_and_release",
            "drop",
        },
        "errors.identity_policy": {
            "request_identity",
            "authority_identity",
            "database_identity",
            "null_authority_identity_kinds",
            "nullable_database_identity_kinds",
        },
    }.items():
        outer, inner = section.split(".")
        shape = properties[outer]["properties"][inner]
        exact(shape.get("type"), "object", f"schema section {section} type drift")
        exact(
            shape.get("additionalProperties"),
            False,
            f"schema section {section} permits unknown fields",
        )
        exact(set(shape.get("required", [])), fields, f"schema section {section} required fields drift")
        exact(set(shape.get("properties", {})), fields, f"schema section {section} property fields drift")
    fixture_items = properties.get("fixtures", {}).get("items", {})
    exact(
        properties.get("fixtures", {}).get("uniqueItems"),
        True,
        "fixture schema permits duplicate entries",
    )
    exact(fixture_items.get("type"), "object", "fixture schema type drift")
    exact(
        fixture_items.get("additionalProperties"),
        False,
        "fixture schema permits unknown fields",
    )
    exact(set(fixture_items.get("required", [])), {"id", "kind"}, "fixture fields drift")
    exact(set(fixture_items.get("properties", {})), {"id", "kind"}, "fixture properties drift")


def validate_contract(contract: dict[str, Any]) -> None:
    exact(set(contract), OUTER_FIELDS, "contract fields drift")
    exact(
        (
            contract.get("schema_version"),
            contract.get("contract"),
            contract.get("issue"),
        ),
        ("axiom.sqlite-v1", "runtime.sqlite", 1452),
        "contract identity drift",
    )
    for section, expected in EXPECTED.items():
        exact(contract.get(section), expected, f"{section} contract drift")
    fixtures = contract.get("fixtures")
    require(isinstance(fixtures, list), "fixtures must be an array")
    observed: dict[str, str] = {}
    for fixture in fixtures:
        require(isinstance(fixture, dict), "each fixture must be an object")
        exact(set(fixture), {"id", "kind"}, "fixture fields drift")
        fixture_id = fixture.get("id")
        kind = fixture.get("kind")
        require(isinstance(fixture_id, str), "fixture id must be a string")
        require(isinstance(kind, str), "fixture kind must be a string")
        require(fixture_id not in observed, f"duplicate fixture id {fixture_id}")
        observed[fixture_id] = kind
    exact(observed, FIXTURES, "required SQLite fixture coverage drift")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA)
    parser.add_argument("--contract", type=Path, default=DEFAULT_CONTRACT)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        schema = load_object(args.schema, "schema")
        contract = load_object(args.contract, "contract")
        validate_schema(schema)
        validate_contract(contract)
        try:
            validate_draft_2020_12(contract, schema)
        except ValueError as error:
            raise ContractError(f"contract violates published schema: {error}") from error
    except ContractError as error:
        if args.json:
            print(json.dumps({"schema": "axiom.sqlite-v1", "ok": False, "errors": [str(error)]}))
        else:
            print(f"SQLite v1 contract: fail\n- {error}", file=sys.stderr)
        return 1
    report = {
        "schema": contract["schema_version"],
        "ok": True,
        "issue": contract["issue"],
        "qualification": contract["scope"]["qualification"],
        "fixtures": len(contract["fixtures"]),
    }
    if args.json:
        print(json.dumps(report, sort_keys=True))
    else:
        print(
            "SQLite v1 contract: pass "
            f"({report['fixtures']} fixtures; {report['qualification']})"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
