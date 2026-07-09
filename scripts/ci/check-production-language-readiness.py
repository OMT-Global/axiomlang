#!/usr/bin/env python3
"""Validate the production-language readiness ledger and report its gate state."""

import argparse
import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path
from typing import Any


SCHEMA = "axiom.production_language.readiness.v1"
TIERS = {
    "syntax_only": 0,
    "static_spike": 1,
    "runtime_complete": 2,
    "production_qualified": 3,
}
STATUSES = {"implemented", "partial", "blocked"}
RISKS = {"low", "medium", "high", "critical"}
ROW_ID = re.compile(r"^[a-z][a-z0-9_]*$")
REQUIRED_ROW_FIELDS = {
    "id",
    "track",
    "requirement",
    "requiredForProduction",
    "targetTier",
    "currentTier",
    "status",
    "governingIssue",
    "dependencies",
    "evidence",
    "validatingCommand",
    "rustCaptureRisk",
    "agentInspectionImpact",
}
ALLOWED_ROW_FIELDS = REQUIRED_ROW_FIELDS | {"blockerIssues", "notes"}
ALLOWED_MANIFEST_FIELDS = {"schemaVersion", "schema", "umbrellaIssue", "rows"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Check AxiOM production-language readiness."
    )
    parser.add_argument("--json", action="store_true", help="emit JSON output")
    parser.add_argument(
        "--validate-only",
        action="store_true",
        help="validate the checked ledger without requiring roadmap completion",
    )
    parser.add_argument(
        "--manifest",
        default="docs/production-language-readiness.json",
        help="path to the readiness manifest",
    )
    parser.add_argument(
        "--doc",
        default="docs/production-language-roadmap.md",
        help="path to the production-language roadmap",
    )
    parser.add_argument(
        "--schema-file",
        default="stage1/schemas/axiom-production-language-readiness-v1.schema.json",
        help="path to the published JSON schema",
    )
    parser.add_argument(
        "--issue-state-file", help="file containing '<issue> <state>' rows"
    )
    parser.add_argument(
        "--require-issue-states",
        action="store_true",
        help="load blocker issue state from GitHub and fail when unavailable",
    )
    return parser.parse_args()


def check(name: str, status: str, detail: str) -> dict[str, str]:
    return {"name": name, "status": status, "detail": detail}


def load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def read_issue_states(path: Path) -> dict[int, str]:
    states: dict[int, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        parts = stripped.split()
        if len(parts) < 2:
            continue
        try:
            issue = int(parts[0])
        except ValueError:
            continue
        states[issue] = parts[1].upper()
    return states


def issue_state_from_github(issue: int) -> str | None:
    try:
        result = subprocess.run(
            ["gh", "issue", "view", str(issue), "--json", "state"],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
    except OSError:
        return None
    if result.returncode != 0:
        return None
    try:
        state = json.loads(result.stdout).get("state", "").upper()
    except (AttributeError, json.JSONDecodeError):
        return None
    return state or None


def instance_matches_type(instance: Any, expected: str) -> bool:
    if expected == "object":
        return isinstance(instance, dict)
    if expected == "array":
        return isinstance(instance, list)
    if expected == "string":
        return isinstance(instance, str)
    if expected == "integer":
        return isinstance(instance, int) and not isinstance(instance, bool)
    if expected == "number":
        return isinstance(instance, (int, float)) and not isinstance(instance, bool)
    if expected == "boolean":
        return isinstance(instance, bool)
    if expected == "null":
        return instance is None
    return False


def resolve_local_ref(root: dict[str, Any], reference: str) -> Any:
    if not reference.startswith("#/"):
        raise ValueError(f"unsupported non-local schema reference {reference!r}")
    value: Any = root
    for raw_part in reference[2:].split("/"):
        part = raw_part.replace("~1", "/").replace("~0", "~")
        if not isinstance(value, dict) or part not in value:
            raise ValueError(f"unresolved schema reference {reference!r}")
        value = value[part]
    return value


def json_values_equal(left: Any, right: Any) -> bool:
    if isinstance(left, bool) or isinstance(right, bool):
        return isinstance(left, bool) and isinstance(right, bool) and left == right
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        return left == right
    return type(left) is type(right) and left == right


def schema_errors(
    instance: Any,
    schema: Any,
    root: dict[str, Any],
    path: str = "$",
) -> list[str]:
    """Validate the JSON Schema keywords used by the checked readiness schema."""
    if not isinstance(schema, dict):
        return [f"{path}: schema node must be an object"]
    if "$ref" in schema:
        try:
            target = resolve_local_ref(root, schema["$ref"])
        except (TypeError, ValueError) as error:
            return [f"{path}: {error}"]
        return schema_errors(instance, target, root, path)

    errors: list[str] = []
    expected_type = schema.get("type")
    if expected_type is not None:
        expected_types = (
            expected_type if isinstance(expected_type, list) else [expected_type]
        )
        if not all(isinstance(item, str) for item in expected_types):
            return [f"{path}: schema type declaration is invalid"]
        if not any(instance_matches_type(instance, item) for item in expected_types):
            return [f"{path}: expected type {' or '.join(expected_types)}"]

    if "const" in schema and not json_values_equal(instance, schema["const"]):
        errors.append(f"{path}: expected constant {schema['const']!r}")
    if "enum" in schema:
        choices = schema["enum"]
        if not isinstance(choices, list) or not any(
            json_values_equal(instance, item) for item in choices
        ):
            errors.append(f"{path}: value {instance!r} is not in the declared enum")

    if isinstance(instance, dict):
        required = schema.get("required", [])
        if isinstance(required, list):
            for field in required:
                if field not in instance:
                    errors.append(f"{path}: missing required property {field!r}")
        properties = schema.get("properties", {})
        if not isinstance(properties, dict):
            errors.append(f"{path}: schema properties must be an object")
            properties = {}
        if schema.get("additionalProperties") is False:
            for field in instance:
                if field not in properties:
                    errors.append(f"{path}: unexpected property {field!r}")
        for field, value in instance.items():
            if field in properties:
                errors.extend(
                    schema_errors(value, properties[field], root, f"{path}.{field}")
                )

    if isinstance(instance, list):
        min_items = schema.get("minItems")
        if isinstance(min_items, int) and len(instance) < min_items:
            errors.append(f"{path}: expected at least {min_items} item(s)")
        if schema.get("uniqueItems") is True:
            encoded = [json.dumps(item, sort_keys=True) for item in instance]
            if len(encoded) != len(set(encoded)):
                errors.append(f"{path}: items must be unique")
        item_schema = schema.get("items")
        if item_schema is not None:
            for index, value in enumerate(instance):
                errors.extend(
                    schema_errors(value, item_schema, root, f"{path}[{index}]")
                )

    if isinstance(instance, str):
        min_length = schema.get("minLength")
        if isinstance(min_length, int) and len(instance) < min_length:
            errors.append(f"{path}: expected at least {min_length} character(s)")
        pattern = schema.get("pattern")
        if isinstance(pattern, str) and re.search(pattern, instance) is None:
            errors.append(f"{path}: value does not match {pattern!r}")

    if isinstance(instance, (int, float)) and not isinstance(instance, bool):
        minimum = schema.get("minimum")
        if isinstance(minimum, (int, float)) and instance < minimum:
            errors.append(f"{path}: value must be at least {minimum}")
    return errors


def validate_integer_list(row: dict[str, Any], field: str, errors: list[str]) -> list[int]:
    value = row.get(field)
    if not isinstance(value, list):
        errors.append(f"{field} must be an array")
        return []
    if any(not isinstance(item, int) or isinstance(item, bool) or item < 1 for item in value):
        errors.append(f"{field} must contain positive issue numbers")
        return []
    if len(value) != len(set(value)):
        errors.append(f"{field} must not contain duplicates")
    return value


def dependency_graph_errors(rows: list[Any]) -> tuple[list[str], int]:
    graph: dict[int, set[int]] = {}
    edge_count = 0
    errors: list[str] = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        issue = row.get("governingIssue")
        dependencies = row.get("dependencies")
        if (
            not isinstance(issue, int)
            or isinstance(issue, bool)
            or issue < 1
            or not isinstance(dependencies, list)
        ):
            continue
        valid_dependencies = {
            dependency
            for dependency in dependencies
            if isinstance(dependency, int)
            and not isinstance(dependency, bool)
            and dependency > 0
        }
        if issue in valid_dependencies:
            errors.append(f"issue #{issue} depends on itself")
        graph.setdefault(issue, set()).update(valid_dependencies)
        edge_count += len(valid_dependencies)

    state: dict[int, int] = {}
    stack: list[int] = []

    def visit(issue: int) -> None:
        if state.get(issue) == 2:
            return
        if state.get(issue) == 1:
            try:
                start = stack.index(issue)
            except ValueError:
                start = 0
            cycle = stack[start:] + [issue]
            detail = " -> ".join(f"#{item}" for item in cycle)
            message = f"dependency cycle: {detail}"
            if message not in errors:
                errors.append(message)
            return
        state[issue] = 1
        stack.append(issue)
        for dependency in sorted(graph.get(issue, set())):
            if dependency in graph:
                visit(dependency)
        stack.pop()
        state[issue] = 2

    for issue in sorted(graph):
        visit(issue)
    return errors, edge_count


def row_summary(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": row.get("id"),
        "track": row.get("track"),
        "required_for_production": row.get("requiredForProduction"),
        "status": row.get("status"),
        "current_tier": row.get("currentTier"),
        "target_tier": row.get("targetTier"),
        "governing_issue": row.get("governingIssue"),
        "blocker_issues": row.get("blockerIssues", []),
        "dependencies": row.get("dependencies", []),
        "validating_command": row.get("validatingCommand"),
    }


def validate_row(
    row: Any, index: int, seen_ids: set[str]
) -> tuple[dict[str, Any], list[str], set[int]]:
    if not isinstance(row, dict):
        return {"id": f"row_{index}"}, ["row must be an object"], set()

    errors: list[str] = []
    missing_fields = sorted(REQUIRED_ROW_FIELDS - set(row))
    if missing_fields:
        errors.append("missing fields: " + ", ".join(missing_fields))
    unexpected_fields = sorted(set(row) - ALLOWED_ROW_FIELDS)
    if unexpected_fields:
        errors.append("unexpected fields: " + ", ".join(unexpected_fields))

    row_id = row.get("id")
    if not isinstance(row_id, str) or not ROW_ID.fullmatch(row_id):
        errors.append("id must match ^[a-z][a-z0-9_]*$")
    elif row_id in seen_ids:
        errors.append("duplicate row id")
    else:
        seen_ids.add(row_id)

    for field in ("track", "requirement", "validatingCommand", "agentInspectionImpact"):
        if not isinstance(row.get(field), str) or not row[field].strip():
            errors.append(f"{field} must be a non-empty string")

    if not isinstance(row.get("requiredForProduction"), bool):
        errors.append("requiredForProduction must be boolean")

    governing_issue = row.get("governingIssue")
    if (
        not isinstance(governing_issue, int)
        or isinstance(governing_issue, bool)
        or governing_issue < 1
    ):
        errors.append("governingIssue must be a positive issue number")

    current_tier = row.get("currentTier")
    target_tier = row.get("targetTier")
    if not isinstance(current_tier, str) or current_tier not in TIERS:
        errors.append(f"invalid currentTier {current_tier!r}")
    if not isinstance(target_tier, str) or target_tier not in TIERS:
        errors.append(f"invalid targetTier {target_tier!r}")

    status = row.get("status")
    if not isinstance(status, str) or status not in STATUSES:
        errors.append(f"invalid status {status!r}")
    risk = row.get("rustCaptureRisk")
    if not isinstance(risk, str) or risk not in RISKS:
        errors.append(f"invalid rustCaptureRisk {risk!r}")

    validate_integer_list(row, "dependencies", errors)
    blockers = validate_integer_list(
        {"blockerIssues": row.get("blockerIssues", [])}, "blockerIssues", errors
    )
    if isinstance(status, str) and status in {"partial", "blocked"} and not blockers:
        errors.append(f"{status} rows require blockerIssues")
    if status == "implemented" and blockers:
        errors.append("implemented rows must not retain blockerIssues")
    if (
        status == "implemented"
        and isinstance(current_tier, str)
        and current_tier in TIERS
        and isinstance(target_tier, str)
        and target_tier in TIERS
        and TIERS[current_tier] < TIERS[target_tier]
    ):
        errors.append("implemented row currentTier is below targetTier")

    evidence = row.get("evidence")
    if not isinstance(evidence, list) or not evidence:
        errors.append("evidence must be a non-empty array")
    elif any(not isinstance(item, str) or not item.strip() for item in evidence):
        errors.append("evidence entries must be non-empty paths")
    else:
        if len(evidence) != len(set(evidence)):
            errors.append("evidence must not contain duplicates")
        missing = [path for path in evidence if not Path(path).exists()]
        if missing:
            errors.append("missing evidence: " + ", ".join(missing))

    return row_summary(row), errors, set(blockers)


def main() -> int:
    args = parse_args()
    manifest_path = Path(args.manifest)
    doc_path = Path(args.doc)
    schema_path = Path(args.schema_file)
    checks: list[dict[str, str]] = []
    payload: dict[str, Any] = {}
    schema_payload: dict[str, Any] = {}

    checks.append(
        check(
            "production_readiness_doc_present",
            "pass" if doc_path.is_file() else "fail",
            f"{doc_path} exists" if doc_path.is_file() else f"{doc_path} is missing",
        )
    )

    if schema_path.is_file():
        try:
            loaded_schema = load_json(schema_path)
            if isinstance(loaded_schema, dict):
                schema_payload = loaded_schema
            schema_valid = (
                isinstance(schema_payload, dict)
                and schema_payload.get("properties", {})
                .get("schema", {})
                .get("const")
                == SCHEMA
            )
            checks.append(
                check(
                    "production_readiness_schema_file",
                    "pass" if schema_valid else "fail",
                    f"{schema_path} publishes {SCHEMA}"
                    if schema_valid
                    else f"{schema_path} does not publish {SCHEMA}",
                )
            )
        except json.JSONDecodeError as error:
            checks.append(
                check(
                    "production_readiness_schema_file",
                    "fail",
                    f"schema JSON is invalid: {error}",
                )
            )
    else:
        checks.append(
            check(
                "production_readiness_schema_file",
                "fail",
                f"{schema_path} is missing",
            )
        )

    if manifest_path.is_file():
        checks.append(
            check(
                "production_readiness_manifest_present",
                "pass",
                f"{manifest_path} exists",
            )
        )
        try:
            loaded = load_json(manifest_path)
            if isinstance(loaded, dict):
                payload = loaded
                checks.append(
                    check(
                        "production_readiness_manifest_json",
                        "pass",
                        "manifest is valid JSON",
                    )
                )
            else:
                checks.append(
                    check(
                        "production_readiness_manifest_json",
                        "fail",
                        "manifest root must be an object",
                    )
                )
        except json.JSONDecodeError as error:
            checks.append(
                check(
                    "production_readiness_manifest_json",
                    "fail",
                    f"manifest JSON is invalid: {error}",
                )
            )
    else:
        checks.append(
            check(
                "production_readiness_manifest_present",
                "fail",
                f"{manifest_path} is missing",
            )
        )

    if schema_payload and payload:
        contract_errors = schema_errors(payload, schema_payload, schema_payload)
        checks.append(
            check(
                "production_readiness_json_schema",
                "fail" if contract_errors else "pass",
                "; ".join(contract_errors[:10])
                if contract_errors
                else "manifest validates against the published JSON Schema",
            )
        )
    else:
        checks.append(
            check(
                "production_readiness_json_schema",
                "fail",
                "manifest and schema must both be available for validation",
            )
        )

    manifest_shape_errors: list[str] = []
    unexpected_manifest_fields = sorted(set(payload) - ALLOWED_MANIFEST_FIELDS)
    if unexpected_manifest_fields:
        manifest_shape_errors.append(
            "unexpected fields: " + ", ".join(unexpected_manifest_fields)
        )
    schema_version = payload.get("schemaVersion")
    if (
        not isinstance(schema_version, int)
        or isinstance(schema_version, bool)
        or schema_version != 1
    ):
        manifest_shape_errors.append("schemaVersion must be 1")
    if payload.get("schema") != SCHEMA:
        manifest_shape_errors.append(f"schema must be {SCHEMA}")
    umbrella = payload.get("umbrellaIssue")
    if not isinstance(umbrella, int) or isinstance(umbrella, bool) or umbrella < 1:
        manifest_shape_errors.append("umbrellaIssue must be a positive issue number")
    checks.append(
        check(
            "production_readiness_manifest_contract",
            "fail" if manifest_shape_errors else "pass",
            "; ".join(manifest_shape_errors)
            if manifest_shape_errors
            else f"manifest conforms to {SCHEMA}",
        )
    )

    manifest_rows = payload.get("rows", [])
    if not isinstance(manifest_rows, list) or not manifest_rows:
        checks.append(
            check(
                "production_readiness_rows_present",
                "fail",
                "manifest must contain at least one readiness row",
            )
        )
        manifest_rows = []
    else:
        checks.append(
            check(
                "production_readiness_rows_present",
                "pass",
                f"manifest contains {len(manifest_rows)} readiness rows",
            )
        )

    graph_errors, dependency_edges = dependency_graph_errors(manifest_rows)
    checks.append(
        check(
            "production_readiness_dependency_graph",
            "fail" if graph_errors else "pass",
            "; ".join(graph_errors)
            if graph_errors
            else f"dependency graph has {dependency_edges} acyclic issue edge(s)",
        )
    )

    rows: list[dict[str, Any]] = []
    seen_ids: set[str] = set()
    required_issues: set[int] = set()
    valid_row_ids: set[str] = set()
    row_errors = 0
    for index, row in enumerate(manifest_rows):
        summary, errors, blockers = validate_row(row, index, seen_ids)
        row_id = summary.get("id") or f"row_{index}"
        rows.append(summary)
        if summary.get("required_for_production") is True:
            governing_issue = summary.get("governing_issue")
            if (
                isinstance(governing_issue, int)
                and not isinstance(governing_issue, bool)
                and governing_issue > 0
            ):
                required_issues.add(governing_issue)
            required_issues.update(blockers)
            dependencies = summary.get("dependencies")
            if isinstance(dependencies, list):
                required_issues.update(
                    issue
                    for issue in dependencies
                    if isinstance(issue, int)
                    and not isinstance(issue, bool)
                    and issue > 0
                )
        if errors:
            row_errors += 1
            checks.append(
                check(
                    f"production_readiness_row_{row_id}",
                    "fail",
                    "; ".join(errors),
                )
            )
        else:
            valid_row_ids.add(str(row_id))
            checks.append(
                check(
                    f"production_readiness_row_{row_id}",
                    "pass",
                    f"row is valid at {summary['current_tier']} toward {summary['target_tier']}",
                )
            )

    issue_states: dict[int, str] = {}
    issue_source = "not required"
    if args.issue_state_file:
        issue_path = Path(args.issue_state_file)
        if issue_path.is_file():
            issue_states = read_issue_states(issue_path)
            issue_source = str(issue_path)
            checks.append(
                check(
                    "production_readiness_issue_state_source",
                    "pass",
                    f"issue states loaded from {issue_path}",
                )
            )
        else:
            issue_source = str(issue_path)
            checks.append(
                check(
                    "production_readiness_issue_state_source",
                    "fail",
                    f"issue state file does not exist: {issue_path}",
                )
            )
    elif args.require_issue_states:
        issue_source = "GitHub"
        checks.append(
            check(
                "production_readiness_issue_state_source",
                "pass",
                "blocker issue states requested from GitHub",
            )
        )
    else:
        checks.append(
            check(
                "production_readiness_issue_state_source",
                "pass",
                "issue states are not required for the offline report",
            )
        )

    if args.issue_state_file or args.require_issue_states:
        for issue in sorted(required_issues):
            state = issue_states.get(issue)
            if state is None and args.require_issue_states and not args.issue_state_file:
                state = issue_state_from_github(issue)
            if state is None:
                checks.append(
                    check(
                        f"production_readiness_issue_{issue}_closed",
                        "fail",
                        f"issue #{issue} state is unavailable from {issue_source}",
                    )
                )
            else:
                checks.append(
                    check(
                        f"production_readiness_issue_{issue}_closed",
                        "pass" if state == "CLOSED" else "fail",
                        f"issue #{issue} is {state}",
                    )
                )

    statuses = Counter(
        row.get("status") for row in rows if isinstance(row.get("status"), str)
    )
    tiers = Counter(
        row.get("current_tier")
        for row in rows
        if isinstance(row.get("current_tier"), str)
    )
    required_rows = [row for row in rows if row.get("required_for_production") is True]
    required_ready = [
        row
        for row in required_rows
        if isinstance(row.get("id"), str)
        and row.get("id") in valid_row_ids
        and row.get("status") == "implemented"
        and isinstance(row.get("current_tier"), str)
        and row.get("current_tier") in TIERS
        and isinstance(row.get("target_tier"), str)
        and row.get("target_tier") in TIERS
        and TIERS[row["current_tier"]] >= TIERS[row["target_tier"]]
    ]
    all_required_ready = len(required_rows) > 0 and len(required_ready) == len(required_rows)
    checks.append(
        check(
            "production_readiness_required_rows",
            "pass" if all_required_ready else "fail",
            f"{len(required_ready)} of {len(required_rows)} required rows meet their target tier",
        )
    )

    validation_checks = [
        item
        for item in checks
        if item["name"] != "production_readiness_required_rows"
        and not re.fullmatch(r"production_readiness_issue_\d+_closed", item["name"])
    ]
    valid = all(item["status"] == "pass" for item in validation_checks)
    ready = all(check_item["status"] == "pass" for check_item in checks)
    summary = {
        "total": len(rows),
        "required": len(required_rows),
        "required_ready": len(required_ready),
        "implemented": statuses["implemented"],
        "partial": statuses["partial"],
        "blocked": statuses["blocked"],
        "syntax_only": tiers["syntax_only"],
        "static_spike": tiers["static_spike"],
        "runtime_complete": tiers["runtime_complete"],
        "production_qualified": tiers["production_qualified"],
        "invalid_rows": row_errors,
        "dependency_edges": dependency_edges,
    }
    report = {
        "schema": SCHEMA,
        "valid": valid,
        "ready": ready,
        "summary": summary,
        "rows": rows,
        "checks": checks,
    }

    if args.json:
        json.dump(report, sys.stdout, indent=2)
        sys.stdout.write("\n")
    elif args.validate_only and valid:
        print("Production language readiness ledger: valid")
    elif ready:
        print("Production language readiness: ready")
    else:
        print(
            "Production language readiness: blocked "
            f"({len(required_ready)}/{len(required_rows)} required rows ready)",
            file=sys.stderr,
        )
        for failed in (item for item in checks if item["status"] == "fail"):
            print(f"- {failed['name']}: {failed['detail']}", file=sys.stderr)
    return 0 if (valid if args.validate_only else ready) else 1


if __name__ == "__main__":
    raise SystemExit(main())
