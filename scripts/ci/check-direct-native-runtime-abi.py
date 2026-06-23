#!/usr/bin/env python3
"""Validate the direct native runtime ABI contract."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CONTRACT = REPO_ROOT / "stage1/runtime-abi/direct-native-v0.json"
DEFAULT_EVIDENCE_TEST_MANIFEST = (
    REPO_ROOT / "stage1/runtime-abi/direct-native-v0-evidence-tests.json"
)
VALID_STATUSES = {"implemented", "partial", "blocked"}
RUST_TEST_FN_RE = re.compile(r"(?m)^fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")
REQUIRED_VALUE_FEATURES = {
    "numeric.scalars",
    "boolean",
    "string",
    "array.fixed",
    "slice.borrowed",
    "map.lookup",
    "tuple",
    "option",
    "result",
    "enum.payload",
    "struct.field",
    "owned.move_state",
}
REQUIRED_CAPABILITY_SHIMS = {
    "fs.read",
    "fs.write",
    "network.dns.resolve",
    "network.tcp",
    "network.udp",
    "network.http.client",
    "network.http.server",
    "network.http.async_server",
    "process.status",
    "cli.args",
    "env.read",
    "clock.now_sleep",
    "crypto.hash",
    "crypto.mac",
    "crypto.random",
    "crypto.signature",
    "crypto.aead",
    "ffi.call",
    "async.runtime",
    "json.serdes",
    "regex.match_replace",
    "sync.primitives",
    "io.logging_stdio",
}


def load_contract(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("contract root must be an object")
    return payload


def load_optional_manifest(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    with path.open(encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("evidence test manifest root must be an object")
    return payload


def validate_rows(
    rows: object,
    required_ids: set[str],
    group_name: str,
    errors: list[str],
    contract_root: Path,
) -> tuple[set[str], list[str], list[str], dict[str, int], set[int]]:
    if not isinstance(rows, list):
        errors.append(f"{group_name} must be an array")
        return set(), [], [], {status: 0 for status in sorted(VALID_STATUSES)}, set()

    seen: set[str] = set()
    incomplete_rows: list[str] = []
    blocked_rows: list[str] = []
    status_counts = {status: 0 for status in sorted(VALID_STATUSES)}
    blocker_issues: set[int] = set()
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            errors.append(f"{group_name}[{index}] must be an object")
            continue
        row_id = row.get("id")
        if not isinstance(row_id, str) or not row_id:
            errors.append(f"{group_name}[{index}].id must be a non-empty string")
            continue
        if row_id in seen:
            errors.append(f"{group_name} contains duplicate row {row_id!r}")
        seen.add(row_id)

        status = row.get("status")
        if status not in VALID_STATUSES:
            errors.append(
                f"{group_name} row {row_id!r} has invalid status {status!r}; "
                f"expected one of {sorted(VALID_STATUSES)}"
            )
            continue
        status_counts[status] += 1

        validate_evidence_paths(row, row_id, group_name, "evidence", errors, contract_root)
        validate_evidence_paths(
            row, row_id, group_name, "denial_evidence", errors, contract_root
        )
        validate_evidence_paths(
            row, row_id, group_name, "runtime_evidence", errors, contract_root
        )
        if status in {"implemented", "partial"} and "evidence" not in row:
            errors.append(
                f"{group_name} row {row_id!r} must name evidence for status {status!r}"
            )

        row_blockers = row.get("blockers", [])
        if status == "implemented":
            if "blockers" in row and row_blockers:
                errors.append(f"{group_name} row {row_id!r} must not name blockers")
            if "runtime_evidence" not in row:
                errors.append(
                    f"{group_name} row {row_id!r} must name runtime_evidence "
                    "for status 'implemented'"
                )
        else:
            incomplete_rows.append(row_id)
            if status == "blocked":
                blocked_rows.append(row_id)
            if not isinstance(row_blockers, list) or not row_blockers:
                errors.append(f"{group_name} row {row_id!r} must name blockers")
            elif not all(isinstance(issue, int) and issue > 0 for issue in row_blockers):
                errors.append(
                    f"{group_name} row {row_id!r} blockers must be positive issue numbers"
                )
            else:
                blocker_issues.update(row_blockers)

    missing = sorted(required_ids - seen)
    if missing:
        errors.append(f"{group_name} missing required rows: {', '.join(missing)}")
    extra = sorted(seen - required_ids)
    if extra:
        errors.append(f"{group_name} has unknown rows: {', '.join(extra)}")
    return seen, incomplete_rows, blocked_rows, status_counts, blocker_issues


def validate_evidence_paths(
    row: dict[str, Any],
    row_id: str,
    group_name: str,
    field_name: str,
    errors: list[str],
    contract_root: Path,
) -> None:
    if field_name not in row:
        return

    evidence = row[field_name]
    if not isinstance(evidence, list) or not evidence:
        errors.append(f"{group_name} row {row_id!r} {field_name} must be a non-empty array")
        return

    for index, evidence_path in enumerate(evidence):
        if not isinstance(evidence_path, str) or not evidence_path:
            errors.append(
                f"{group_name} row {row_id!r} {field_name}[{index}] "
                "must be a non-empty string"
            )
            continue
        path = Path(evidence_path)
        if path.is_absolute() or ".." in path.parts:
            errors.append(
                f"{group_name} row {row_id!r} {field_name}[{index}] "
                "must be a repository-relative path"
            )
            continue
        if not (contract_root / path).is_file():
            errors.append(
                f"{group_name} row {row_id!r} {field_name}[{index}] "
                f"does not exist: {evidence_path}"
            )


def validate_evidence_test_manifest(
    manifest: dict[str, Any] | None,
    errors: list[str],
    contract_root: Path,
) -> dict[str, dict[str, int]] | None:
    if manifest is None:
        return None

    if (
        manifest.get("schema_version")
        != "axiom.direct_native.runtime_abi.evidence_tests.v0"
    ):
        errors.append(
            "evidence test manifest schema_version must be "
            "axiom.direct_native.runtime_abi.evidence_tests.v0"
        )
    if manifest.get("target_id") != "axiom://target/stage1-direct-native":
        errors.append(
            "evidence test manifest target_id must be "
            "axiom://target/stage1-direct-native"
        )

    test_source_value = manifest.get("test_source")
    test_names: set[str] = set()
    if not isinstance(test_source_value, str) or not test_source_value:
        errors.append("evidence test manifest test_source must be a non-empty string")
    else:
        test_source = Path(test_source_value)
        if test_source.is_absolute() or ".." in test_source.parts:
            errors.append(
                "evidence test manifest test_source must be a repository-relative path"
            )
        else:
            test_source_path = contract_root / test_source
            if not test_source_path.is_file():
                errors.append(
                    "evidence test manifest test_source does not exist: "
                    f"{test_source_value}"
                )
            else:
                test_names = set(
                    RUST_TEST_FN_RE.findall(test_source_path.read_text(encoding="utf-8"))
                )

    counts: dict[str, dict[str, int]] = {
        "value_features": {},
        "capability_shims": {},
    }
    validate_evidence_test_group(
        manifest.get("value_features"),
        REQUIRED_VALUE_FEATURES,
        test_names,
        "value_features",
        errors,
        counts["value_features"],
    )
    validate_evidence_test_group(
        manifest.get("capability_shims"),
        REQUIRED_CAPABILITY_SHIMS,
        test_names,
        "capability_shims",
        errors,
        counts["capability_shims"],
    )
    return counts


def validate_evidence_test_group(
    group: object,
    required_ids: set[str],
    test_names: set[str],
    group_name: str,
    errors: list[str],
    counts: dict[str, int],
) -> None:
    if not isinstance(group, dict):
        errors.append(f"evidence test manifest {group_name} must be an object")
        return

    seen = set(group)
    missing = sorted(required_ids - seen)
    if missing:
        errors.append(
            f"evidence test manifest {group_name} missing required rows: "
            f"{', '.join(missing)}"
        )
    extra = sorted(seen - required_ids)
    if extra:
        errors.append(
            f"evidence test manifest {group_name} has unknown rows: "
            f"{', '.join(extra)}"
        )

    for row_id, tests in sorted(group.items()):
        if not isinstance(row_id, str) or not row_id:
            errors.append(
                f"evidence test manifest {group_name} row ids must be non-empty strings"
            )
            continue
        if not isinstance(tests, list) or not tests:
            errors.append(
                f"evidence test manifest {group_name} row {row_id!r} "
                "must name at least one focused test"
            )
            continue
        row_tests: set[str] = set()
        for index, test_name in enumerate(tests):
            if not isinstance(test_name, str) or not test_name:
                errors.append(
                    f"evidence test manifest {group_name} row {row_id!r} "
                    f"test[{index}] must be a non-empty string"
                )
                continue
            if test_name in row_tests:
                errors.append(
                    f"evidence test manifest {group_name} row {row_id!r} "
                    f"contains duplicate test {test_name!r}"
                )
            row_tests.add(test_name)
            if test_names and test_name not in test_names:
                errors.append(
                    f"evidence test manifest {group_name} row {row_id!r} "
                    f"names missing test {test_name!r}"
                )
        counts[row_id] = len(row_tests)


def resolve_evidence_row(
    manifest: dict[str, Any] | None,
    contract: dict[str, Any],
    row_id: str,
) -> tuple[dict[str, Any], int]:
    contract_row, contract_group = find_contract_row(contract, row_id)
    row_report = {
        "schema": "axiom.direct_native.runtime_abi.evidence_row.v1",
        "target_id": None,
        "row_id": row_id,
        "group": contract_group,
        "status": None,
        "blockers": [],
        "evidence": [],
        "denial_evidence": [],
        "runtime_evidence": [],
        "notes": None,
        "test_source": None,
        "tests": [],
        "errors": [],
    }
    if contract_row is None:
        row_report["errors"].append(f"unknown direct native runtime ABI contract row: {row_id}")
    else:
        row_report["status"] = contract_row.get("status")
        row_report["blockers"] = contract_row.get("blockers", [])
        row_report["evidence"] = contract_row.get("evidence", [])
        row_report["denial_evidence"] = contract_row.get("denial_evidence", [])
        row_report["runtime_evidence"] = contract_row.get("runtime_evidence", [])
        row_report["notes"] = contract_row.get("notes")

    if manifest is None:
        row_report["errors"].append("evidence test manifest is not available")
        return row_report, 1

    row_report["target_id"] = manifest.get("target_id")
    row_report["test_source"] = manifest.get("test_source")
    for group_name in ("value_features", "capability_shims"):
        group = manifest.get(group_name)
        if isinstance(group, dict) and row_id in group:
            tests = group[row_id]
            row_report["group"] = row_report["group"] or group_name
            row_report["tests"] = tests if isinstance(tests, list) else []
            return row_report, 1 if row_report["errors"] else 0

    row_report["errors"].append(f"unknown direct native runtime ABI evidence row: {row_id}")
    return row_report, 1


def build_evidence_row_list(
    manifest: dict[str, Any] | None,
    contract: dict[str, Any],
    check_report: dict[str, Any],
) -> tuple[dict[str, Any], int]:
    rows: list[dict[str, Any]] = []
    for group_name in ("value_features", "capability_shims"):
        contract_rows = contract.get(group_name)
        if not isinstance(contract_rows, list):
            continue
        for row in contract_rows:
            if not isinstance(row, dict):
                continue
            row_id = row.get("id")
            if not isinstance(row_id, str) or not row_id:
                continue
            tests = evidence_tests_for_row(manifest, group_name, row_id)
            rows.append(
                {
                    "row_id": row_id,
                    "group": group_name,
                    "status": row.get("status"),
                    "blockers": row.get("blockers", []),
                    "evidence": row.get("evidence", []),
                    "denial_evidence": row.get("denial_evidence", []),
                    "runtime_evidence": row.get("runtime_evidence", []),
                    "test_source": manifest.get("test_source")
                    if isinstance(manifest, dict)
                    else None,
                    "tests": tests,
                    "test_count": len(tests),
                    "notes": row.get("notes"),
                }
            )

    row_list_report = {
        "schema": "axiom.direct_native.runtime_abi.evidence_rows.v1",
        "ready": check_report["ready"],
        "target_id": contract.get("target_id"),
        "contract_status": contract.get("status"),
        "status_counts": check_report["status_counts"],
        "value_feature_count": check_report["value_feature_count"],
        "capability_shim_count": check_report["capability_shim_count"],
        "incomplete_rows": check_report["incomplete_rows"],
        "blocked_rows": check_report["blocked_rows"],
        "blocker_issues": check_report["blocker_issues"],
        "rows": rows,
        "errors": check_report["errors"],
    }
    return row_list_report, 1 if check_report["errors"] else 0


def evidence_tests_for_row(
    manifest: dict[str, Any] | None,
    group_name: str,
    row_id: str,
) -> list[str]:
    if manifest is None:
        return []
    group = manifest.get(group_name)
    if not isinstance(group, dict):
        return []
    tests = group.get(row_id)
    if not isinstance(tests, list):
        return []
    return [test for test in tests if isinstance(test, str)]


def find_contract_row(
    contract: dict[str, Any],
    row_id: str,
) -> tuple[dict[str, Any] | None, str | None]:
    for group_name in ("value_features", "capability_shims"):
        rows = contract.get(group_name)
        if not isinstance(rows, list):
            continue
        for row in rows:
            if isinstance(row, dict) and row.get("id") == row_id:
                return row, group_name
    return None, None


def build_report(
    contract: dict[str, Any],
    contract_root: Path = REPO_ROOT,
    evidence_test_manifest: dict[str, Any] | None = None,
) -> tuple[dict[str, Any], int]:
    errors: list[str] = []

    if contract.get("schema_version") != "axiom.direct_native.runtime_abi.v0":
        errors.append("schema_version must be axiom.direct_native.runtime_abi.v0")
    if contract.get("target_id") != "axiom://target/stage1-direct-native":
        errors.append("target_id must be axiom://target/stage1-direct-native")
    if contract.get("status") not in VALID_STATUSES:
        errors.append("status must be implemented, partial, or blocked")

    (
        _,
        value_incomplete_rows,
        value_blocked_rows,
        value_status_counts,
        value_blocker_issues,
    ) = validate_rows(
        contract.get("value_features"),
        REQUIRED_VALUE_FEATURES,
        "value_features",
        errors,
        contract_root,
    )
    (
        _,
        capability_incomplete_rows,
        capability_blocked_rows,
        capability_status_counts,
        capability_blocker_issues,
    ) = validate_rows(
        contract.get("capability_shims"),
        REQUIRED_CAPABILITY_SHIMS,
        "capability_shims",
        errors,
        contract_root,
    )
    evidence_test_counts = validate_evidence_test_manifest(
        evidence_test_manifest,
        errors,
        contract_root,
    )

    incomplete_rows = sorted(value_incomplete_rows + capability_incomplete_rows)
    blocked_rows = sorted(value_blocked_rows + capability_blocked_rows)
    blocker_issues = sorted(value_blocker_issues | capability_blocker_issues)
    ready = not errors and not incomplete_rows and contract.get("status") == "implemented"
    report = {
        "schema": "axiom.direct_native.runtime_abi.check.v1",
        "ready": ready,
        "target_id": contract.get("target_id"),
        "contract_status": contract.get("status"),
        "status_counts": {
            "value_features": value_status_counts,
            "capability_shims": capability_status_counts,
        },
        "value_feature_count": len(contract.get("value_features", []))
        if isinstance(contract.get("value_features"), list)
        else 0,
        "capability_shim_count": len(contract.get("capability_shims", []))
        if isinstance(contract.get("capability_shims"), list)
        else 0,
        "incomplete_rows": incomplete_rows,
        "blocked_rows": blocked_rows,
        "blocker_issues": blocker_issues,
        "evidence_test_manifest": {
            "present": evidence_test_manifest is not None,
            "value_feature_rows": len(
                (evidence_test_counts or {}).get("value_features", {})
            ),
            "value_feature_test_count": sum(
                (evidence_test_counts or {}).get("value_features", {}).values()
            ),
            "capability_shim_rows": len(
                (evidence_test_counts or {}).get("capability_shims", {})
            ),
            "capability_shim_test_count": sum(
                (evidence_test_counts or {}).get("capability_shims", {}).values()
            ),
        },
        "errors": errors,
    }
    return report, 1 if errors else 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate the direct native runtime ABI contract."
    )
    parser.add_argument("--contract", type=Path, default=DEFAULT_CONTRACT)
    parser.add_argument(
        "--evidence-test-manifest",
        type=Path,
        default=DEFAULT_EVIDENCE_TEST_MANIFEST,
        help="focused test manifest to validate with the ABI contract",
    )
    parser.add_argument(
        "--no-evidence-test-manifest",
        action="store_true",
        help="skip focused test manifest validation",
    )
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    parser.add_argument(
        "--evidence-row",
        help="emit focused Cranelift test names for one runtime ABI row",
    )
    parser.add_argument(
        "--list-evidence-rows",
        action="store_true",
        help="emit the runtime ABI row inventory with status, evidence, and tests",
    )
    parser.add_argument(
        "--enforce-ready",
        action="store_true",
        help="fail while any runtime ABI row remains partial or blocked",
    )
    args = parser.parse_args()

    if args.list_evidence_rows and args.evidence_row:
        parser.error("--list-evidence-rows and --evidence-row cannot be combined")

    try:
        contract = load_contract(args.contract)
        manifest_path = (
            None if args.no_evidence_test_manifest else args.evidence_test_manifest
        )
        evidence_test_manifest = load_optional_manifest(manifest_path)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        report = {
            "schema": "axiom.direct_native.runtime_abi.check.v1",
            "ready": False,
            "target_id": None,
            "contract_status": None,
            "status_counts": {
                "value_features": {status: 0 for status in sorted(VALID_STATUSES)},
                "capability_shims": {status: 0 for status in sorted(VALID_STATUSES)},
            },
            "value_feature_count": 0,
            "capability_shim_count": 0,
            "incomplete_rows": [],
            "blocked_rows": [],
            "blocker_issues": [],
            "evidence_test_manifest": {
                "present": False,
                "value_feature_rows": 0,
                "value_feature_test_count": 0,
                "capability_shim_rows": 0,
                "capability_shim_test_count": 0,
            },
            "errors": [str(error)],
        }
        if args.json:
            print(json.dumps(report, indent=2))
        else:
            print(f"direct native runtime ABI: invalid ({error})", file=sys.stderr)
        return 1

    report, validation_status = build_report(
        contract,
        REPO_ROOT,
        evidence_test_manifest,
    )
    if args.list_evidence_rows:
        row_list_report, row_list_status = build_evidence_row_list(
            evidence_test_manifest,
            contract,
            report,
        )
        if args.json:
            print(json.dumps(row_list_report, indent=2))
        elif row_list_report["errors"]:
            for error in row_list_report["errors"]:
                print(error, file=sys.stderr)
        else:
            for row in row_list_report["rows"]:
                blockers = ", ".join(f"#{issue}" for issue in row["blockers"]) or "-"
                print(
                    f"{row['group']} {row['row_id']} {row['status']} "
                    f"tests={row['test_count']} blockers={blockers}"
                )
        if validation_status:
            return validation_status
        return row_list_status

    if args.evidence_row:
        row_report, row_status = resolve_evidence_row(
            evidence_test_manifest,
            contract,
            args.evidence_row,
        )
        row_report["errors"] = [*report["errors"], *row_report["errors"]]
        if args.json:
            print(json.dumps(row_report, indent=2))
        elif row_report["errors"]:
            for error in row_report["errors"]:
                print(error, file=sys.stderr)
        else:
            for test_name in row_report["tests"]:
                print(test_name)
        if validation_status:
            return validation_status
        return row_status

    if args.json:
        print(json.dumps(report, indent=2))
    elif report["ready"]:
        print("direct native runtime ABI: ready")
    else:
        print(
            "direct native runtime ABI: not ready "
            f"({len(report['incomplete_rows'])} incomplete rows, "
            f"{len(report['blocked_rows'])} blocked rows, {len(report['errors'])} errors)"
        )
        if report["incomplete_rows"]:
            print(f"incomplete rows: {', '.join(report['incomplete_rows'])}")
        if report["blocked_rows"]:
            print(f"blocked rows: {', '.join(report['blocked_rows'])}")
        if report["blocker_issues"]:
            issue_list = ", ".join(f"#{issue}" for issue in report["blocker_issues"])
            print(f"blocker issues: {issue_list}")

    if validation_status:
        return validation_status
    if args.enforce_ready and not report["ready"]:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
