#!/usr/bin/env python3
"""Validate the direct native runtime ABI contract."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CONTRACT = REPO_ROOT / "stage1/runtime-abi/direct-native-v0.json"
VALID_STATUSES = {"implemented", "partial", "blocked"}
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


def build_report(
    contract: dict[str, Any], contract_root: Path = REPO_ROOT
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
        "errors": errors,
    }
    return report, 1 if errors else 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate the direct native runtime ABI contract."
    )
    parser.add_argument("--contract", type=Path, default=DEFAULT_CONTRACT)
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    parser.add_argument(
        "--enforce-ready",
        action="store_true",
        help="fail while any runtime ABI row remains partial or blocked",
    )
    args = parser.parse_args()

    try:
        contract = load_contract(args.contract)
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
            "errors": [str(error)],
        }
        if args.json:
            print(json.dumps(report, indent=2))
        else:
            print(f"direct native runtime ABI: invalid ({error})", file=sys.stderr)
        return 1

    report, validation_status = build_report(contract, REPO_ROOT)
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
