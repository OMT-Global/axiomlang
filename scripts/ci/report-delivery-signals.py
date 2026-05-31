#!/usr/bin/env python3
"""Report issue, PR, CI, and review delivery signals.

The report is advisory by default. It is intended for operators and agents that
need a fresh, deterministic queue view without adding a second required GitHub
status check.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib.parse import quote

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_REPO = "OMT-Global/axiom"
REPORT_VERSION = "axiom.delivery_signals.v0"
ISSUE_REF_RE = re.compile(
    r"(?:https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/issues/|"
    r"(?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#)([0-9]+)"
)
ISSUE_EXCEPTION_RE = re.compile(
    r"\b(no issue is linked|no linked issue|without a linked issue|no governing issue)\b",
    re.IGNORECASE,
)
FAILURE_CONCLUSIONS = {"ACTION_REQUIRED", "CANCELLED", "FAILURE", "STARTUP_FAILURE", "TIMED_OUT"}
REBASE_STATES = {"BEHIND", "DIRTY", "UNKNOWN"}


@dataclass(frozen=True)
class IssueState:
    number: int
    state: str
    title: str | None = None
    url: str | None = None


def run_gh(args: list[str], repo: str | None) -> Any:
    cmd = ["gh", *args]
    if repo:
        cmd.extend(["--repo", repo])
    completed = subprocess.run(
        cmd,
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        if completed.stderr:
            sys.stderr.write(completed.stderr)
        raise SystemExit(completed.returncode)
    return json.loads(completed.stdout)


def collect_prs_from_github(args: argparse.Namespace) -> list[dict[str, Any]]:
    fields = ",".join(
        [
            "number",
            "title",
            "url",
            "headRefName",
            "baseRefName",
            "headRefOid",
            "body",
            "mergeStateStatus",
            "reviewDecision",
            "statusCheckRollup",
            "closingIssuesReferences",
            "files",
            "isDraft",
            "updatedAt",
        ]
    )
    if args.pr:
        return [
            run_gh(["pr", "view", str(number), "--json", fields], args.repo)
            for number in sorted(args.pr)
        ]
    return run_gh(
        ["pr", "list", "--state", args.state, "--limit", str(args.limit), "--json", fields],
        args.repo,
    )


def load_fixture(path: Path) -> tuple[list[dict[str, Any]], dict[int, IssueState]]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    prs = payload.get("prs", payload) if isinstance(payload, dict) else payload
    if not isinstance(prs, list):
        raise SystemExit(f"{path}: fixture must be a list of PR objects or an object with prs")

    issues: dict[int, IssueState] = {}
    for issue in payload.get("issues", []) if isinstance(payload, dict) else []:
        number = int(issue["number"])
        issues[number] = IssueState(
            number=number,
            state=str(issue.get("state", "UNKNOWN")).upper(),
            title=issue.get("title"),
            url=issue.get("url"),
        )
    return prs, issues


def issue_numbers_from_body(body: str | None) -> list[int]:
    if not body:
        return []
    return sorted({int(match.group(1)) for match in ISSUE_REF_RE.finditer(body)})


def has_issue_exception(body: str | None) -> bool:
    return bool(body and ISSUE_EXCEPTION_RE.search(body))


def closing_issue_states(pr: dict[str, Any]) -> dict[int, IssueState]:
    issues: dict[int, IssueState] = {}
    for issue in pr.get("closingIssuesReferences") or []:
        number = int(issue["number"])
        issues[number] = IssueState(
            number=number,
            state=str(issue.get("state", "UNKNOWN")).upper(),
            title=issue.get("title"),
            url=issue.get("url"),
        )
    return issues


def lookup_issue_states(numbers: set[int], repo: str | None, cached: dict[int, IssueState]) -> dict[int, IssueState]:
    states = dict(cached)
    needs_refresh = {
        number
        for number in numbers
        if number not in states or states[number].state in {"", "UNKNOWN"}
    }
    for number in sorted(needs_refresh):
        try:
            payload = run_gh(["issue", "view", str(number), "--json", "number,state,title,url"], repo)
        except SystemExit:
            states[number] = IssueState(number=number, state="MISSING")
            continue
        states[number] = IssueState(
            number=int(payload["number"]),
            state=str(payload.get("state", "UNKNOWN")).upper(),
            title=payload.get("title"),
            url=payload.get("url"),
        )
    return states


def file_paths(pr: dict[str, Any]) -> list[str]:
    paths: list[str] = []
    for item in pr.get("files") or []:
        path = item.get("path") if isinstance(item, dict) else None
        if path:
            paths.append(path)
    return sorted(set(paths))


def package_root_for_path(path: str) -> str | None:
    candidate = REPO_ROOT / path
    current = candidate.parent if candidate.suffix else candidate
    while current != REPO_ROOT and REPO_ROOT in current.parents:
        if (current / "axiom.toml").exists():
            return current.relative_to(REPO_ROOT).as_posix()
        current = current.parent
    return None


def axiom_id(*parts: str) -> str:
    encoded = "/".join(quote(part.strip("/"), safe="-._~:@!$&'()*+,;=%") for part in parts if part)
    return f"axiom://package/{encoded}"


def semantic_nodes(paths: list[str]) -> list[dict[str, str]]:
    nodes: dict[str, dict[str, str]] = {}
    for path in paths:
        package = package_root_for_path(path)
        if not package:
            continue
        package_id = axiom_id(package)
        nodes[package_id] = {"id": package_id, "kind": "package", "path": package}
        if path.endswith(".ax"):
            source_rel = Path(path).relative_to(package).as_posix()
            source_id = axiom_id(package, f"source#{source_rel}")
            nodes[source_id] = {"id": source_id, "kind": "source", "path": path}
    return [nodes[key] for key in sorted(nodes)]


def check_items(pr: dict[str, Any]) -> list[dict[str, str | None]]:
    items: list[dict[str, str | None]] = []
    for item in pr.get("statusCheckRollup") or []:
        if not isinstance(item, dict):
            continue
        items.append(
            {
                "name": item.get("name"),
                "workflow": item.get("workflowName"),
                "status": (item.get("status") or "").upper() or None,
                "conclusion": (item.get("conclusion") or "").upper() or None,
            }
        )
    return sorted(items, key=lambda item: (item.get("workflow") or "", item.get("name") or ""))


def ci_signal(pr: dict[str, Any]) -> dict[str, Any]:
    checks = check_items(pr)
    gate = next((item for item in checks if item.get("name") == "CI Gate"), None)
    failing = [item for item in checks if item.get("conclusion") in FAILURE_CONCLUSIONS]
    pending = [
        item
        for item in checks
        if item.get("status") and item.get("status") != "COMPLETED"
    ]
    if gate:
        conclusion = gate.get("conclusion")
        status = gate.get("status")
        if conclusion == "SUCCESS":
            evidence_status = "passing"
        elif conclusion in FAILURE_CONCLUSIONS:
            evidence_status = "failing"
        elif status and status != "COMPLETED":
            evidence_status = "required"
        else:
            evidence_status = "missing"
    elif failing:
        evidence_status = "failing"
    elif pending:
        evidence_status = "required"
    else:
        evidence_status = "missing"

    return {
        "status": evidence_status,
        "ci_gate": gate,
        "checks": checks,
        "failing_checks": failing,
        "pending_checks": pending,
    }


def review_signal(pr: dict[str, Any]) -> dict[str, str | None]:
    decision = (pr.get("reviewDecision") or "UNKNOWN").upper()
    if decision == "APPROVED":
        evidence_status = "passing"
    elif decision == "CHANGES_REQUESTED":
        evidence_status = "failing"
    elif decision in {"REVIEW_REQUIRED", "UNKNOWN"}:
        evidence_status = "required"
    else:
        evidence_status = "provided"
    return {"decision": decision, "status": evidence_status}


def traceability_status(
    issue_numbers: list[int],
    states: dict[int, IssueState],
    body: str | None,
) -> dict[str, Any]:
    if issue_numbers:
        linked = [states.get(number, IssueState(number, "UNKNOWN")) for number in issue_numbers]
        invalid = [issue for issue in linked if issue.state not in {"OPEN"}]
        status = "pass" if not invalid else "closed_or_missing_issue"
        return {
            "status": status,
            "issue_numbers": issue_numbers,
            "issues": [issue.__dict__ for issue in linked],
            "exception": False,
        }
    if has_issue_exception(body):
        return {"status": "exception", "issue_numbers": [], "issues": [], "exception": True}
    return {"status": "missing_issue_link", "issue_numbers": [], "issues": [], "exception": False}


def evidence_entry(
    pr: dict[str, Any],
    evidence_type: str,
    status: str,
    collected_at: str,
    diagnostics: list[str],
    signal: dict[str, Any],
    repository: str,
) -> dict[str, Any]:
    number = int(pr["number"])
    target = axiom_id("github", "pr", str(number))
    return {
        "id": axiom_id("github", "pr", str(number), "evidence", evidence_type),
        "evidence_type": evidence_type,
        "status": status,
        "target": target,
        "path": None,
        "diagnostics": diagnostics,
        "delivery_signal": {
            "provider": "github",
            "repository": repository,
            "pr": number,
            "commit": pr.get("headRefOid"),
            "collected_at": collected_at,
            "fresh_for_commit": True,
            **signal,
        },
    }


def classify_pr(
    pr: dict[str, Any],
    issue_states: dict[int, IssueState],
    collected_at: str,
    repository: str,
) -> dict[str, Any]:
    paths = file_paths(pr)
    numbers = sorted(set(issue_numbers_from_body(pr.get("body"))) | set(closing_issue_states(pr)))
    traceability = traceability_status(numbers, issue_states, pr.get("body"))
    ci = ci_signal(pr)
    review = review_signal(pr)

    classification: list[str] = []
    if pr.get("isDraft"):
        classification.append("draft")
    if (pr.get("mergeStateStatus") or "").upper() in REBASE_STATES:
        classification.append("needs_rebase")
    if traceability["status"] == "missing_issue_link":
        classification.append("missing_issue_link")
    elif traceability["status"] == "closed_or_missing_issue":
        classification.append("closed_or_missing_issue")
    if ci["status"] == "failing":
        classification.append("ci_failing")
    elif ci["status"] == "required":
        classification.append("ci_pending")
    if review["status"] != "passing":
        classification.append("awaiting_review")
    if not classification:
        classification.append("mergeable")

    ci_diagnostics = [
        f"{item.get('workflow') or 'check'} / {item.get('name')}: {item.get('conclusion')}"
        for item in ci["failing_checks"]
    ]
    if ci["status"] == "missing":
        ci_diagnostics.append("CI Gate check was not found in statusCheckRollup")

    review_diagnostics = []
    if review["status"] != "passing":
        review_diagnostics.append(f"reviewDecision is {review['decision']}")

    return {
        "number": int(pr["number"]),
        "title": pr.get("title"),
        "url": pr.get("url"),
        "head_ref": pr.get("headRefName"),
        "base_ref": pr.get("baseRefName"),
        "head_sha": pr.get("headRefOid"),
        "merge_state": pr.get("mergeStateStatus"),
        "classification": classification,
        "traceability": traceability,
        "changed_files": paths,
        "semantic_nodes": semantic_nodes(paths),
        "ci": ci,
        "review": review,
        "evidence": [
            evidence_entry(
                pr,
                "ci_status",
                ci["status"],
                collected_at,
                ci_diagnostics,
                {
                    "signal": "ci_status",
                    "check_name": "CI Gate",
                    "state": ci["status"],
                    "ci_gate": ci["ci_gate"],
                },
                repository,
            ),
            evidence_entry(
                pr,
                "review_state",
                review["status"],
                collected_at,
                review_diagnostics,
                {
                    "signal": "review_state",
                    "state": review["decision"],
                },
                repository,
            ),
        ],
    }


def build_issue_index(prs: list[dict[str, Any]]) -> list[dict[str, Any]]:
    index: dict[int, dict[str, Any]] = {}
    for pr in prs:
        for issue in pr["traceability"]["issues"]:
            number = int(issue["number"])
            entry = index.setdefault(
                number,
                {
                    "number": number,
                    "state": issue.get("state"),
                    "title": issue.get("title"),
                    "url": issue.get("url"),
                    "prs": [],
                    "changed_files": [],
                    "semantic_nodes": [],
                },
            )
            entry["prs"].append(pr["number"])
            entry["changed_files"].extend(pr["changed_files"])
            entry["semantic_nodes"].extend(pr["semantic_nodes"])

    for entry in index.values():
        entry["prs"] = sorted(set(entry["prs"]))
        entry["changed_files"] = sorted(set(entry["changed_files"]))
        unique_nodes = {node["id"]: node for node in entry["semantic_nodes"]}
        entry["semantic_nodes"] = [unique_nodes[key] for key in sorted(unique_nodes)]
    return [index[key] for key in sorted(index)]


def summarize(prs: list[dict[str, Any]]) -> dict[str, int]:
    keys = [
        "mergeable",
        "needs_rebase",
        "ci_failing",
        "ci_pending",
        "awaiting_review",
        "missing_issue_link",
        "closed_or_missing_issue",
        "draft",
    ]
    summary = {"total_prs": len(prs)}
    for key in keys:
        summary[key] = sum(1 for pr in prs if key in pr["classification"])
    return summary


def collected_at() -> str:
    override = os.environ.get("AXIOM_DELIVERY_COLLECTED_AT")
    if override:
        return override
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    if args.fixture:
        raw_prs, cached_issues = load_fixture(args.fixture)
        source = "fixture"
    else:
        raw_prs = collect_prs_from_github(args)
        cached_issues = {}
        source = "github"

    issue_numbers: set[int] = set()
    cached = dict(cached_issues)
    for pr in raw_prs:
        cached.update(closing_issue_states(pr))
        issue_numbers.update(issue_numbers_from_body(pr.get("body")))
        issue_numbers.update(closing_issue_states(pr))

    issue_states = cached if args.fixture else lookup_issue_states(issue_numbers, args.repo, cached)
    timestamp = collected_at()
    repository = args.repo or DEFAULT_REPO
    prs = [classify_pr(pr, issue_states, timestamp, repository) for pr in raw_prs]
    if args.issue:
        wanted = set(args.issue)
        prs = [
            pr
            for pr in prs
            if wanted.intersection({int(issue["number"]) for issue in pr["traceability"]["issues"]})
        ]
    prs = sorted(prs, key=lambda pr: pr["number"])
    return {
        "schema_version": REPORT_VERSION,
        "repository": repository,
        "source": source,
        "collected_at": timestamp,
        "summary": summarize(prs),
        "issues": build_issue_index(prs),
        "prs": prs,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=DEFAULT_REPO)
    parser.add_argument("--state", default="open", choices=["open", "closed", "merged", "all"])
    parser.add_argument("--limit", type=int, default=50)
    parser.add_argument("--pr", type=int, action="append", help="Limit the report to a PR number")
    parser.add_argument("--issue", type=int, action="append", help="Limit the report to PRs linked to an issue")
    parser.add_argument("--fixture", type=Path, help="Read PR data from a fixture instead of GitHub")
    parser.add_argument(
        "--check-traceability",
        action="store_true",
        help="Return non-zero when a PR is missing a governing issue or links a closed/missing issue",
    )
    args = parser.parse_args()

    report = build_report(args)
    json.dump(report, sys.stdout, indent=2, sort_keys=True)
    print()

    if args.check_traceability:
        for pr in report["prs"]:
            if "missing_issue_link" in pr["classification"] or "closed_or_missing_issue" in pr["classification"]:
                return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
