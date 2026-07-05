#!/usr/bin/env python3
import argparse
import json
import subprocess
import sys
from pathlib import Path

SCHEMA = "axiom.self_hosting.language_readiness.v0"
VALID_STATUSES = {"implemented", "partial", "blocked"}
VALID_DIRECT_NATIVE_STATUSES = {"implemented", "partial", "blocked", "not_applicable"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Check self-hosting language readiness.")
    parser.add_argument("--json", action="store_true", help="emit JSON output")
    parser.add_argument(
        "--manifest",
        default="docs/self-hosting-language-readiness.json",
        help="path to the readiness manifest",
    )
    parser.add_argument(
        "--doc",
        default="docs/self-hosting-language-readiness.md",
        help="path to the readiness documentation",
    )
    parser.add_argument("--issue-state-file", help="file containing '<issue> <state>' rows")
    parser.add_argument(
        "--require-issue-states",
        action="store_true",
        help="fail when blocker issue state cannot be read",
    )
    return parser.parse_args()


def check(name: str, status: str, detail: str) -> dict:
    return {"name": name, "status": status, "detail": detail}


def load_json(path: Path):
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def read_issue_states(path: str | None) -> dict[int, str]:
    if not path:
        return {}
    issue_path = Path(path)
    if not issue_path.is_file():
        return {}
    states: dict[int, str] = {}
    for line in issue_path.read_text(encoding="utf-8").splitlines():
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
    except json.JSONDecodeError:
        return None
    return state or None


def blocker_issue_states(
    issues: set[int], issue_state_file: str | None, require_issue_states: bool
) -> tuple[list[dict], bool]:
    checks: list[dict] = []
    file_states = read_issue_states(issue_state_file)
    source = None
    if issue_state_file:
        issue_state_path = Path(issue_state_file)
        source = issue_state_file
        checks.append(
            check(
                "language_readiness_issue_state_source",
                "pass" if issue_state_path.is_file() else "fail",
                f"issue states loaded from {issue_state_file}"
                if issue_state_path.is_file()
                else f"issue state file does not exist: {issue_state_file}",
            )
        )
    elif require_issue_states:
        source = "GitHub"
        checks.append(
            check(
                "language_readiness_issue_state_source",
                "pass",
                "issue states loaded from GitHub",
            )
        )
    else:
        checks.append(
            check(
                "language_readiness_issue_state_source",
                "pass",
                "issue states not required; pass --require-issue-states for rewrite decision PRs",
            )
        )

    issue_states_available = True
    for issue in sorted(issues):
        state = file_states.get(issue)
        if state is None and (require_issue_states or issue_state_file):
            state = issue_state_from_github(issue) if not issue_state_file else None
        if state is None:
            if require_issue_states or issue_state_file:
                issue_states_available = False
                checks.append(
                    check(
                        f"language_readiness_issue_{issue}_closed",
                        "fail",
                        f"issue #{issue} state is unavailable from {source}",
                    )
                )
            continue
        checks.append(
            check(
                f"language_readiness_issue_{issue}_closed",
                "pass" if state == "CLOSED" else "fail",
                f"issue #{issue} is {state}",
            )
        )
    return checks, issue_states_available


def row_summary(row: dict) -> dict:
    return {
        "id": row.get("id"),
        "group": row.get("group"),
        "status": row.get("status"),
        "direct_native_status": row.get("directNativeStatus"),
        "governing_issue": row.get("governingIssue"),
        "blocker_issues": row.get("blockerIssues", []),
        "validating_command": row.get("validatingCommand"),
    }


def main() -> int:
    args = parse_args()
    manifest_path = Path(args.manifest)
    doc_path = Path(args.doc)
    checks: list[dict] = []
    rows: list[dict] = []
    blocker_issues: set[int] = set()

    checks.append(
        check(
            "language_readiness_doc_present",
            "pass" if doc_path.is_file() else "fail",
            f"{doc_path} exists" if doc_path.is_file() else f"{doc_path} is missing",
        )
    )

    if not manifest_path.is_file():
        checks.append(
            check(
                "language_readiness_manifest_present",
                "fail",
                f"{manifest_path} is missing",
            )
        )
        payload = {}
    else:
        checks.append(
            check(
                "language_readiness_manifest_present",
                "pass",
                f"{manifest_path} exists",
            )
        )
        try:
            payload = load_json(manifest_path)
            checks.append(
                check(
                    "language_readiness_manifest_json",
                    "pass",
                    "manifest is valid JSON",
                )
            )
        except json.JSONDecodeError as error:
            checks.append(
                check(
                    "language_readiness_manifest_json",
                    "fail",
                    f"manifest JSON is invalid: {error}",
                )
            )
            payload = {}

    if payload.get("schema") != SCHEMA:
        checks.append(
            check(
                "language_readiness_schema",
                "fail",
                f"manifest schema must be {SCHEMA}",
            )
        )
    else:
        checks.append(
            check("language_readiness_schema", "pass", f"manifest schema is {SCHEMA}")
        )

    manifest_rows = payload.get("rows", [])
    if not isinstance(manifest_rows, list) or not manifest_rows:
        checks.append(
            check(
                "language_readiness_rows_present",
                "fail",
                "manifest must contain at least one readiness row",
            )
        )
        manifest_rows = []
    else:
        checks.append(
            check(
                "language_readiness_rows_present",
                "pass",
                f"manifest contains {len(manifest_rows)} readiness rows",
            )
        )

    seen_ids: set[str] = set()
    implemented_rows = 0
    blocked_rows = 0
    row_failures = 0
    for index, row in enumerate(manifest_rows):
        row_id = str(row.get("id", f"row_{index}"))
        rows.append(row_summary(row))
        row_checks: list[str] = []

        if row_id in seen_ids:
            row_checks.append("duplicate row id")
        seen_ids.add(row_id)

        status = row.get("status")
        direct_native_status = row.get("directNativeStatus")
        if status not in VALID_STATUSES:
            row_checks.append(f"invalid status {status!r}")
        if direct_native_status not in VALID_DIRECT_NATIVE_STATUSES:
            row_checks.append(f"invalid directNativeStatus {direct_native_status!r}")
        if not row.get("group"):
            row_checks.append("missing group")
        if not row.get("requirement"):
            row_checks.append("missing requirement")
        if not isinstance(row.get("governingIssue"), int):
            row_checks.append("missing numeric governingIssue")
        if not row.get("validatingCommand"):
            row_checks.append("missing validatingCommand")

        evidence = row.get("evidence", [])
        if not isinstance(evidence, list) or not evidence:
            row_checks.append("missing evidence")
        else:
            missing_evidence = [path for path in evidence if not Path(path).exists()]
            if missing_evidence:
                row_checks.append("missing evidence: " + ", ".join(missing_evidence))

        blockers = row.get("blockerIssues", [])
        if blockers:
            if not all(isinstance(issue, int) for issue in blockers):
                row_checks.append("blockerIssues must be numeric issue ids")
            else:
                blocker_issues.update(blockers)
        if status != "implemented":
            blocked_rows += 1
            if not blockers:
                row_checks.append("non-implemented row must name blockerIssues")
        else:
            implemented_rows += 1

        if row_checks:
            row_failures += 1
            checks.append(
                check(
                    f"language_readiness_row_{row_id}",
                    "fail",
                    "; ".join(row_checks),
                )
            )
        else:
            checks.append(
                check(
                    f"language_readiness_row_{row_id}",
                    "pass",
                    f"{status} row is well formed",
                )
            )

    if blocked_rows:
        checks.append(
            check(
                "language_readiness_rows_implemented",
                "fail",
                f"{blocked_rows} row(s) are not implemented",
            )
        )
    else:
        checks.append(
            check(
                "language_readiness_rows_implemented",
                "pass",
                f"all {implemented_rows} row(s) are implemented",
            )
        )

    issue_checks, _ = blocker_issue_states(
        blocker_issues, args.issue_state_file, args.require_issue_states
    )
    checks.extend(issue_checks)

    ready = all(item["status"] == "pass" for item in checks)
    report = {
        "schema": SCHEMA,
        "ready": ready,
        "manifest": str(manifest_path),
        "document": str(doc_path),
        "rows": rows,
        "summary": {
            "total_rows": len(manifest_rows),
            "implemented_rows": implemented_rows,
            "blocked_rows": blocked_rows,
            "row_failures": row_failures,
            "blocker_issues": sorted(blocker_issues),
        },
        "checks": checks,
    }

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        if ready:
            print("Self-hosting language readiness: ready")
        else:
            print("Self-hosting language readiness: blocked", file=sys.stderr)
        for item in checks:
            print(f"{item['status']} {item['name']}: {item['detail']}")

    return 0 if ready else 1


if __name__ == "__main__":
    raise SystemExit(main())
