#!/usr/bin/env python3
"""Negative regression coverage for every SQLite v1 contract boundary."""

from __future__ import annotations

import copy
import json
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, Callable


REPO_ROOT = Path(__file__).resolve().parents[2]
CHECKER = REPO_ROOT / "scripts/ci/check-sqlite-v1.py"
SCHEMA = REPO_ROOT / "stage1/compiler-contracts/schemas/axiom.sqlite-v1.schema.json"
CONTRACT = REPO_ROOT / "stage1/compiler-contracts/snapshots/sqlite-v1.json"

Change = Callable[[dict[str, Any], dict[str, Any]], None]


def set_path(path: tuple[Any, ...], value: Any) -> Change:
    def change(contract: dict[str, Any], _schema: dict[str, Any]) -> None:
        cursor: Any = contract
        for key in path[:-1]:
            cursor = cursor[key]
        cursor[path[-1]] = value

    return change


def remove_path(path: tuple[Any, ...]) -> Change:
    def change(contract: dict[str, Any], _schema: dict[str, Any]) -> None:
        cursor: Any = contract
        for key in path[:-1]:
            cursor = cursor[key]
        if isinstance(cursor, list):
            if isinstance(path[-1], int):
                cursor.pop(path[-1])
            else:
                cursor.remove(path[-1])
        else:
            cursor.pop(path[-1])

    return change


CASES: dict[str, Change] = {
    "schema-version": set_path(("schema_version",), "axiom.sqlite-v2"),
    "contract-identity": set_path(("contract",), "runtime.database"),
    "governing-issue": set_path(("issue",), 0),
    "provider-boundary": set_path(("scope", "provider_boundary"), "raw-sqlite-c"),
    "qualification-overclaim": set_path(("scope", "qualification"), "runtime_complete"),
    "raw-surface": set_path(("scope", "raw_surface"), "allowed"),
    "authority-capability": set_path(("authority", "capability"), "fs"),
    "authority-identity": set_path(("authority", "identity"), "caller-label"),
    "authority-path": set_path(("authority", "path_policy"), "ambient-path"),
    "authority-uri-scheme": set_path(("authority", "uri_policy", "allowed"), ["file", "https"]),
    "authority-uri-query": set_path(("authority", "uri_policy", "query"), "allowed"),
    "authority-uri-authority": set_path(("authority", "uri_policy", "authority"), "allowed"),
    "authority-uri-fragment": set_path(("authority", "uri_policy", "fragment"), "allowed"),
    "authority-uri-decoding": set_path(("authority", "uri_policy", "percent_decoding"), "after-anchor"),
    "authority-uri-mode": set_path(("authority", "uri_policy", "mode_source"), "query-parameter"),
    "authority-uri-ambient": set_path(("authority", "uri_policy", "ambient"), "allowed"),
    "authority-uri-network": set_path(("authority", "uri_policy", "network"), "allowed"),
    "authority-mode": set_path(("authority", "modes"), ["read_write"]),
    "authority-read-write-grant": set_path(
        ("authority", "mode_authority", "read_write"), "read-only"
    ),
    "authority-create-grant": set_path(
        ("authority", "mode_authority", "create"), "implicit"
    ),
    "authority-checks": remove_path(("authority", "checks", "no_symlink_escape")),
    "authority-open-identity": remove_path(
        ("authority", "checks", "mode_specific_open_strategy")
    ),
    "authority-existing-open-strategy": set_path(
        ("authority", "open_strategy", "existing"), "pathname-open"
    ),
    "authority-create-open-strategy": set_path(
        ("authority", "open_strategy", "create"), "pathname-create"
    ),
    "statement-lifecycle": remove_path(("statements", "lifecycle", "finalize")),
    "statement-sql-source": set_path(("statements", "sql_source"), "concatenated-command"),
    "statement-batching": set_path(("statements", "batching"), "unbounded-script"),
    "parameter-addressing": set_path(("statements", "parameters", "addressing"), ["string-interpolation"]),
    "parameter-types": remove_path(("statements", "parameters", "types", "bytes")),
    "parameter-limits": set_path(("statements", "parameters", "limits"), "after-copy"),
    "parameter-unknown": set_path(("statements", "parameters", "unknown"), "coerce"),
    "parameter-missing-binding": set_path(
        ("statements", "parameters", "missing_binding"), "implicit-null"
    ),
    "parameter-reset-binding": set_path(
        ("statements", "parameters", "reset_binding"), "retain"
    ),
    "row-iteration": set_path(("statements", "rows", "iteration"), "unbounded-buffer"),
    "row-lifetime": set_path(("statements", "rows", "lifetime"), "forever"),
    "row-lookup": set_path(("statements", "rows", "column_lookup"), ["index"]),
    "row-types": remove_path(("statements", "rows", "types", "null")),
    "row-coercion": set_path(("statements", "rows", "coercion"), "implicit"),
    "row-limits": set_path(("statements", "rows", "limits"), "unchecked"),
    "bound-value-injection": set_path(("statements", "injection"), "reparse-bound-text"),
    "transaction-top-level": remove_path(("transactions", "top_level", "rollback")),
    "transaction-begin-modes": set_path(("transactions", "begin_modes"), ["deferred"]),
    "transaction-nested": set_path(("transactions", "nested"), "nested-begin"),
    "transaction-savepoint-name": set_path(("transactions", "savepoint_names"), "caller-sql"),
    "transaction-savepoint-rollback": set_path(
        ("transactions", "savepoints", "rollback_to"), "terminal"
    ),
    "transaction-savepoint-release": set_path(
        ("transactions", "savepoints", "release"), "commit-poisoned"
    ),
    "transaction-drop": set_path(("transactions", "drop"), "commit"),
    "transaction-failure": set_path(("transactions", "failure"), "continue"),
    "transaction-parent-failure": set_path(
        ("transactions", "parent_failure"), "poison-parent"
    ),
    "migration-artifact": set_path(("migrations", "artifact"), "raw-sql"),
    "migration-dialect": set_path(("migrations", "dialect"), "postgresql"),
    "migration-identity": set_path(("migrations", "identity"), "name-only"),
    "migration-order": set_path(("migrations", "ordering"), "filesystem-order"),
    "migration-apply": set_path(("migrations", "apply"), "best-effort"),
    "migration-history": set_path(("migrations", "history"), "mutable"),
    "migration-divergence": set_path(("migrations", "divergence"), "overwrite"),
    "busy-policy": set_path(("concurrency", "busy"), "retry-forever"),
    "timeout-policy": set_path(("concurrency", "timeout"), "per-attempt-reset"),
    "cancellation-policy": set_path(("concurrency", "cancellation"), "reuse-immediately"),
    "writer-policy": set_path(("concurrency", "writer_policy"), "implicit"),
    "crash-reopen": set_path(("concurrency", "crash_reopen"), "partial-success"),
    "corruption": set_path(("concurrency", "corruption"), "automatic-repair"),
    "handle-kinds": remove_path(("lifecycle", "handles", "statement")),
    "handle-representation": set_path(("lifecycle", "representation"), "raw-pointer"),
    "handle-close": set_path(("lifecycle", "close"), "leave-children"),
    "handle-drop": set_path(("lifecycle", "drop"), "close-database-only"),
    "handle-validation": remove_path(("lifecycle", "validation", "generation")),
    "handle-leaks": set_path(("lifecycle", "leaks"), "warning"),
    "error-shape": remove_path(("errors", "shape", "database_identity")),
    "error-request-identity": set_path(
        ("errors", "identity_policy", "request_identity"), "raw-path-digest"
    ),
    "error-authority-identity": set_path(
        ("errors", "identity_policy", "authority_identity"), "raw-path-digest"
    ),
    "error-pre-resolution-identity": set_path(
        ("errors", "identity_policy", "database_identity"), "always-required"
    ),
    "error-null-authority-kinds": remove_path(
        ("errors", "identity_policy", "null_authority_identity_kinds", "invalid_authority")
    ),
    "error-null-database-kinds": remove_path(
        ("errors", "identity_policy", "nullable_database_identity_kinds", "open_failed")
    ),
    "error-kinds": remove_path(("errors", "kinds", "corruption")),
    "error-message-policy": set_path(("errors", "message_policy"), "raw-provider-message"),
    "audit-fields": remove_path(("audit", "fields", "operation_class")),
    "audit-request-identity": remove_path(("audit", "fields", "request_identity")),
    "audit-operation-classes": remove_path(("audit", "operation_classes", "recovery")),
    "audit-secrets": remove_path(("audit", "forbidden", "parameter_values")),
    "fixture-id": set_path(("fixtures", 0, "id"), "generic-success"),
    "fixture-kind": set_path(("fixtures", 0, "kind"), "negative"),
    "fixture-missing": remove_path(("fixtures", 9)),
    "unknown-contract-field": lambda contract, _schema: contract.update({"extension": {}}),
    "unknown-section-field": lambda contract, _schema: contract["authority"].update({"token": "x"}),
    "schema-envelope-open": lambda _contract, schema: schema.update({"additionalProperties": True}),
    "schema-required-drift": lambda _contract, schema: schema["required"].remove("audit"),
    "schema-section-open": lambda _contract, schema: schema["properties"]["authority"].update({"additionalProperties": True}),
    "schema-nested-open": lambda _contract, schema: schema["properties"]["statements"]["properties"]["rows"].update({"additionalProperties": True}),
    "schema-fixture-open": lambda _contract, schema: schema["properties"]["fixtures"]["items"].update({"additionalProperties": True}),
    "schema-fixture-duplicates": lambda _contract, schema: schema["properties"]["fixtures"].update({"uniqueItems": False}),
    "schema-message-policy-constraint": lambda _contract, schema: schema["properties"]["errors"]["properties"]["message_policy"].pop("const"),
}


def run(schema_path: Path, contract_path: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(CHECKER),
            "--schema",
            str(schema_path),
            "--contract",
            str(contract_path),
            "--json",
        ],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )


def write(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    original_schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    original_contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        schema_path = root / "schema.json"
        contract_path = root / "contract.json"
        write(schema_path, original_schema)
        write(contract_path, original_contract)
        valid = run(schema_path, contract_path)
        if valid.returncode != 0:
            print("valid SQLite v1 contract was rejected", file=sys.stderr)
            print(valid.stdout, file=sys.stderr)
            print(valid.stderr, file=sys.stderr)
            return 1
        for name, change in CASES.items():
            schema = copy.deepcopy(original_schema)
            contract = copy.deepcopy(original_contract)
            change(contract, schema)
            write(schema_path, schema)
            write(contract_path, contract)
            result = run(schema_path, contract_path)
            if result.returncode == 0:
                print(f"SQLite v1 negative case was accepted: {name}", file=sys.stderr)
                return 1
    print(f"SQLite v1 checker tests passed ({len(CASES)} negative cases)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
