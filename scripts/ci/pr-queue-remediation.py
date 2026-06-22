#!/usr/bin/env python3
"""Classify the open pull request queue for repeatable remediation."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "axiom.pr_queue_remediation.v0"
PR_FIELDS = [
    "number",
    "title",
    "url",
    "headRefName",
    "headRefOid",
    "baseRefName",
    "isDraft",
    "mergeable",
    "reviewDecision",
    "statusCheckRollup",
]
PASSING_CONCLUSIONS = {"SUCCESS", "SKIPPED", "NEUTRAL"}
FAILING_CONCLUSIONS = {"FAILURE", "CANCELLED", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE"}


PRIORITIES = {
    "needs_rebase": 10,
    "ci_failing": 20,
    "review_blocked": 30,
    "ci_pending": 40,
    "awaiting_review": 50,
    "draft": 60,
    "needs_recheck": 70,
    "merge_ready": 90,
}

NEXT_ACTION = {
    "needs_rebase": "Rebase or merge current main, push the branch, then re-fetch CI and review state.",
    "ci_failing": "Inspect the failed check logs, repair the branch, rerun evidence, then re-fetch live state.",
    "review_blocked": "Address requested changes, push an update, comment with validation, then request re-review.",
    "ci_pending": "Wait for runner capacity or rerun the queued workflow, then re-fetch terminal check state.",
    "awaiting_review": "Request or wait for non-author approval after required checks reach a terminal state.",
    "draft": "Confirm scope, mark ready only when the branch is ready for normal review gates.",
    "needs_recheck": "Fetch live PR state again; GitHub did not return enough terminal signal to classify safely.",
    "merge_ready": "Maintainer can merge or enable auto-merge if policy and branch protection allow it.",
}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def load_snapshot(path: Path) -> list[dict[str, Any]]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(payload, list):
        return payload
    prs = payload.get("pull_requests")
    if isinstance(prs, list):
        return prs
    raise ValueError("input JSON must be a PR list or an object with pull_requests")


def fetch_live_prs(repo: str, limit: int) -> list[dict[str, Any]]:
    result = subprocess.run(
        [
            "gh",
            "pr",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--limit",
            str(limit),
            "--json",
            ",".join(PR_FIELDS),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "gh pr list failed")
    payload = json.loads(result.stdout)
    if not isinstance(payload, list):
        raise ValueError("gh pr list returned a non-list payload")
    return payload


def check_name(check: dict[str, Any]) -> str:
    return str(check.get("name") or check.get("context") or "unknown")


def check_state(check: dict[str, Any]) -> str:
    status = str(check.get("status") or "").upper()
    conclusion = str(check.get("conclusion") or "").upper()
    if status and status != "COMPLETED":
        return "pending"
    if conclusion in FAILING_CONCLUSIONS:
        return "failing"
    if conclusion in PASSING_CONCLUSIONS:
        return "passing"
    if status == "COMPLETED" and not conclusion:
        return "pending"
    return "pending"


def summarize_checks(checks: list[dict[str, Any]]) -> dict[str, Any]:
    failed: list[str] = []
    pending: list[str] = []
    passed: list[str] = []

    for check in checks:
        name = check_name(check)
        state = check_state(check)
        if state == "failing":
            failed.append(name)
        elif state == "pending":
            pending.append(name)
        else:
            passed.append(name)

    if failed:
        state = "failing"
    elif pending:
        state = "pending"
    elif checks:
        state = "passing"
    else:
        state = "missing"

    return {
        "state": state,
        "failed": sorted(failed),
        "pending": sorted(pending),
        "passed": sorted(passed),
        "total": len(checks),
    }


def classify_pr(pr: dict[str, Any]) -> dict[str, Any]:
    checks = summarize_checks(pr.get("statusCheckRollup") or [])
    mergeable = str(pr.get("mergeable") or "UNKNOWN").upper()
    review_decision = str(pr.get("reviewDecision") or "REVIEW_REQUIRED").upper()
    is_draft = bool(pr.get("isDraft"))

    if is_draft:
        state = "draft"
    elif mergeable == "CONFLICTING":
        state = "needs_rebase"
    elif checks["state"] == "failing":
        state = "ci_failing"
    elif review_decision == "CHANGES_REQUESTED":
        state = "review_blocked"
    elif checks["state"] == "pending":
        state = "ci_pending"
    elif checks["state"] == "missing" or mergeable == "UNKNOWN":
        state = "needs_recheck"
    elif review_decision in {"REVIEW_REQUIRED", ""}:
        state = "awaiting_review"
    elif review_decision == "APPROVED" and checks["state"] == "passing":
        state = "merge_ready"
    else:
        state = "needs_recheck"

    return {
        "number": int(pr["number"]),
        "title": pr.get("title"),
        "url": pr.get("url"),
        "base_ref": pr.get("baseRefName"),
        "head_ref": pr.get("headRefName"),
        "head_sha": pr.get("headRefOid"),
        "mergeable": mergeable,
        "review_decision": review_decision,
        "checks": checks,
        "classification": state,
        "priority": PRIORITIES[state],
        "next_action": NEXT_ACTION[state],
    }


def build_report(
    *,
    repo: str,
    prs: list[dict[str, Any]],
    rechecked_at: str,
    source: str,
) -> dict[str, Any]:
    classified = sorted((classify_pr(pr) for pr in prs), key=lambda item: item["number"])
    worklist = sorted(classified, key=lambda item: (item["priority"], item["number"]))
    counts = Counter(item["classification"] for item in classified)

    return {
        "schema_version": SCHEMA_VERSION,
        "repository": repo,
        "source": source,
        "rechecked_at": rechecked_at,
        "operator_guards": {
            "auto_merge": False,
            "force_push": False,
            "mutates_branches": False,
            "requires_fresh_recheck_after_remediation": True,
        },
        "summary": {
            "open_pull_requests": len(classified),
            "actionable": sum(1 for item in classified if item["classification"] != "merge_ready"),
            "by_classification": dict(sorted(counts.items())),
        },
        "pull_requests": classified,
        "worklist": worklist,
    }


def render_text(report: dict[str, Any]) -> str:
    summary = report["summary"]
    lines = [
        f"pr-queue-remediation: {summary['open_pull_requests']} open PRs, {summary['actionable']} actionable",
        f"repository: {report['repository']}",
        f"rechecked_at: {report['rechecked_at']}",
    ]
    for item in report["worklist"]:
        lines.append(
            "#{number} p{priority} {classification}: {title}".format(
                number=item["number"],
                priority=item["priority"],
                classification=item["classification"],
                title=item["title"] or "",
            )
        )
        lines.append(f"  next: {item['next_action']}")
    return "\n".join(lines)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="OMT-Global/axiomlang")
    parser.add_argument("--input", type=Path, help="Fixture JSON from gh pr list")
    parser.add_argument("--limit", type=int, default=100)
    parser.add_argument("--rechecked-at", default=None)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    rechecked_at = args.rechecked_at or utc_now()
    try:
        if args.input:
            prs = load_snapshot(args.input)
            source = "input_fixture"
        else:
            prs = fetch_live_prs(args.repo, args.limit)
            source = "live_gh_pr_list"
        report = build_report(
            repo=args.repo,
            prs=prs,
            rechecked_at=rechecked_at,
            source=source,
        )
    except (RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"pr-queue-remediation: {error}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(render_text(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
