#!/usr/bin/env python3
"""Build a deterministic, non-mutating remote branch prune plan."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "axiom.remote_branch_prune_plan.v1"
TERMINAL_PR_STATES = {"CLOSED", "MERGED"}
DEFAULT_PRESERVE_PREFIXES = ("archive/", "historical/", "hotfix/", "release/")
FULL_SHA = re.compile(r"^[0-9a-f]{40}$")

QUERY = """
query RemoteBranchPrunePlan(
  $owner: String!
  $name: String!
  $cursor: String
) {
  repository(owner: $owner, name: $name) {
    nameWithOwner
    deleteBranchOnMerge
    defaultBranchRef {
      name
    }
    refs(
      refPrefix: "refs/heads/"
      first: 100
      after: $cursor
      orderBy: {field: ALPHABETICAL, direction: ASC}
    ) {
      nodes {
        name
        target {
          oid
        }
        branchProtectionRule {
          id
        }
        associatedPullRequests(first: 100) {
          nodes {
            number
            state
            mergedAt
            closedAt
            headRefOid
            title
            url
          }
          pageInfo {
            hasNextPage
          }
        }
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
""".strip()


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def parse_repo(value: str) -> tuple[str, str]:
    parts = value.split("/")
    if len(parts) != 2 or not all(parts):
        raise ValueError("repository must use owner/name form")
    return parts[0], parts[1]


def graphql_page(repo: str, cursor: str | None) -> dict[str, Any]:
    owner, name = parse_repo(repo)
    command = [
        "gh",
        "api",
        "graphql",
        "-f",
        f"query={QUERY}",
        "-F",
        f"owner={owner}",
        "-F",
        f"name={name}",
    ]
    if cursor is not None:
        command.extend(["-F", f"cursor={cursor}"])
    result = subprocess.run(command, check=False, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "gh api graphql failed")
    payload = json.loads(result.stdout)
    errors = payload.get("errors")
    if errors:
        raise RuntimeError(f"GitHub GraphQL returned errors: {errors}")
    repository = payload.get("data", {}).get("repository")
    if not isinstance(repository, dict):
        raise ValueError("GitHub GraphQL response did not contain a repository")
    return repository


def fetch_live_snapshot(repo: str) -> dict[str, Any]:
    nodes: list[dict[str, Any]] = []
    cursor: str | None = None
    repository_metadata: dict[str, Any] | None = None
    while True:
        repository = graphql_page(repo, cursor)
        refs = repository.get("refs")
        if not isinstance(refs, dict) or not isinstance(refs.get("nodes"), list):
            raise ValueError("GitHub GraphQL response did not contain branch refs")
        if repository_metadata is None:
            repository_metadata = {
                "nameWithOwner": repository.get("nameWithOwner"),
                "deleteBranchOnMerge": repository.get("deleteBranchOnMerge"),
                "defaultBranchRef": repository.get("defaultBranchRef"),
            }
        nodes.extend(refs["nodes"])
        page_info = refs.get("pageInfo")
        if not isinstance(page_info, dict):
            raise ValueError("GitHub GraphQL response did not contain ref pagination")
        if not page_info.get("hasNextPage"):
            break
        next_cursor = page_info.get("endCursor")
        if not isinstance(next_cursor, str) or not next_cursor:
            raise ValueError("GitHub GraphQL pagination cursor was missing")
        cursor = next_cursor
    assert repository_metadata is not None
    repository_metadata["refs"] = {"nodes": nodes}
    return repository_metadata


def load_snapshot(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError("input JSON must be an object")
    if isinstance(payload.get("data"), dict):
        payload = payload["data"]
    if isinstance(payload.get("repository"), dict):
        payload = payload["repository"]
    return payload


def require_string(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{field} must be a non-empty string")
    return value


def require_sha(value: Any, field: str) -> str:
    sha = require_string(value, field).lower()
    if not FULL_SHA.fullmatch(sha):
        raise ValueError(f"{field} must be a full 40-character hexadecimal SHA")
    return sha


def normalize_pr(raw: Any, branch_name: str) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise ValueError(f"branch {branch_name!r} has a non-object pull request")
    number = raw.get("number")
    if not isinstance(number, int) or isinstance(number, bool) or number < 1:
        raise ValueError(f"branch {branch_name!r} has a pull request without a valid number")
    state = require_string(raw.get("state"), f"PR #{number} state").upper()
    head_sha = raw.get("headRefOid")
    if head_sha is not None:
        head_sha = require_sha(head_sha, f"PR #{number} headRefOid")
    return {
        "number": number,
        "state": state,
        "head_sha": head_sha,
        "merged_at": raw.get("mergedAt"),
        "closed_at": raw.get("closedAt"),
        "title": raw.get("title"),
        "url": raw.get("url"),
    }


def classify_branch(
    raw: Any,
    *,
    default_branch: str,
    preserve_prefixes: tuple[str, ...],
) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise ValueError("branch entry must be an object")
    name = require_string(raw.get("name"), "branch name")
    target = raw.get("target")
    if not isinstance(target, dict):
        raise ValueError(f"branch {name!r} target must be an object")
    sha = require_sha(target.get("oid"), f"branch {name!r} target oid")
    protected = raw.get("branchProtectionRule") is not None
    associated = raw.get("associatedPullRequests")
    if not isinstance(associated, dict) or not isinstance(associated.get("nodes"), list):
        raise ValueError(f"branch {name!r} associatedPullRequests must contain nodes")
    prs = sorted(
        (normalize_pr(item, name) for item in associated["nodes"]),
        key=lambda item: item["number"],
    )
    if len({pull_request["number"] for pull_request in prs}) != len(prs):
        raise ValueError(f"branch {name!r} contains duplicate pull request numbers")
    pull_requests_truncated = bool(
        isinstance(associated.get("pageInfo"), dict)
        and associated["pageInfo"].get("hasNextPage")
    )
    matched_prefix = next((prefix for prefix in preserve_prefixes if name.startswith(prefix)), None)

    if name == default_branch:
        disposition, reason = "preserve", "default_branch"
    elif protected:
        disposition, reason = "preserve", "protected_branch"
    elif matched_prefix is not None:
        disposition, reason = "preserve", f"preserved_prefix:{matched_prefix}"
    elif pull_requests_truncated:
        disposition, reason = "manual_review", "associated_pull_requests_truncated"
    elif not prs:
        disposition, reason = "manual_review", "no_associated_pull_request"
    elif any(pr["state"] == "OPEN" for pr in prs):
        disposition, reason = "preserve", "open_pull_request"
    elif any(pr["state"] not in TERMINAL_PR_STATES for pr in prs):
        disposition, reason = "manual_review", "unknown_pull_request_state"
    elif not any(pr["head_sha"] == sha for pr in prs):
        disposition, reason = "manual_review", "branch_sha_differs_from_terminal_pr_head"
    else:
        disposition, reason = "candidate", "terminal_pull_request_exact_head"

    return {
        "name": name,
        "sha": sha,
        "protected": protected,
        "is_default": name == default_branch,
        "preserved_prefix": matched_prefix,
        "pull_requests_truncated": pull_requests_truncated,
        "disposition": disposition,
        "reason": reason,
        "pull_requests": prs,
    }


def build_report(
    *,
    snapshot: dict[str, Any],
    repository: str,
    snapshot_at: str,
    source: str,
    preserve_prefixes: tuple[str, ...],
) -> dict[str, Any]:
    live_name = require_string(snapshot.get("nameWithOwner"), "repository nameWithOwner")
    if live_name.casefold() != repository.casefold():
        raise ValueError(
            f"snapshot repository {live_name!r} does not match requested {repository!r}"
        )
    default_ref = snapshot.get("defaultBranchRef")
    if not isinstance(default_ref, dict):
        raise ValueError("repository defaultBranchRef must be an object")
    default_branch = require_string(default_ref.get("name"), "default branch name")
    delete_branch_on_merge = snapshot.get("deleteBranchOnMerge")
    if not isinstance(delete_branch_on_merge, bool):
        raise ValueError("repository deleteBranchOnMerge must be a boolean")
    refs = snapshot.get("refs")
    if not isinstance(refs, dict) or not isinstance(refs.get("nodes"), list):
        raise ValueError("repository refs must contain nodes")
    branches = sorted(
        (
            classify_branch(
                item,
                default_branch=default_branch,
                preserve_prefixes=preserve_prefixes,
            )
            for item in refs["nodes"]
        ),
        key=lambda item: item["name"],
    )
    names = [branch["name"] for branch in branches]
    if len(names) != len(set(names)):
        raise ValueError("snapshot contains duplicate branch names")
    counts = Counter(branch["disposition"] for branch in branches)
    reason_counts = Counter(branch["reason"] for branch in branches)
    candidates = [
        {"name": branch["name"], "sha": branch["sha"]}
        for branch in branches
        if branch["disposition"] == "candidate"
    ]
    review_manifest = [
        {
            "branch": branch["name"],
            "sha": branch["sha"],
            "associated_pull_requests": branch["pull_requests"],
        }
        for branch in branches
        if branch["disposition"] == "candidate"
    ]
    return {
        "schema_version": SCHEMA_VERSION,
        "repository": repository,
        "source": source,
        "snapshot_at": snapshot_at,
        "settings": {
            "default_branch": default_branch,
            "delete_branch_on_merge": delete_branch_on_merge,
            "preserve_prefixes": list(preserve_prefixes),
        },
        "operator_guards": {
            "mutates_branches": False,
            "candidate_means_approved_to_delete": False,
            "requires_explicit_maintainer_approval": True,
            "requires_fresh_identical_sha_recheck": True,
            "requires_open_pr_recheck": True,
        },
        "summary": {
            "remote_branches": len(branches),
            "by_disposition": dict(sorted(counts.items())),
            "by_reason": dict(sorted(reason_counts.items())),
            "candidate_count": len(candidates),
        },
        "candidates": candidates,
        "review_manifest": review_manifest,
        "branches": branches,
    }


def render_text(report: dict[str, Any]) -> str:
    summary = report["summary"]
    lines = [
        (
            "remote-branch-prune-plan: "
            f"{summary['remote_branches']} branches, "
            f"{summary['candidate_count']} review candidates"
        ),
        f"repository: {report['repository']}",
        f"snapshot_at: {report['snapshot_at']}",
        "dispositions: "
        + ", ".join(
            f"{name}={count}"
            for name, count in sorted(summary["by_disposition"].items())
        ),
        "No branch was deleted. Candidates require explicit approval and a fresh SHA recheck.",
    ]
    for candidate in report["candidates"]:
        manifest = next(
            item
            for item in report["review_manifest"]
            if item["branch"] == candidate["name"]
        )
        pull_requests = ",".join(
            f"#{pull_request['number']}:{pull_request['state']}"
            for pull_request in manifest["associated_pull_requests"]
        )
        lines.append(
            f"candidate {candidate['name']} {candidate['sha']} "
            f"pull_requests={pull_requests}"
        )
    return "\n".join(lines)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="OMT-Global/axiomlang")
    parser.add_argument("--input", type=Path, help="Fixture or captured GraphQL repository JSON")
    parser.add_argument("--snapshot-at")
    parser.add_argument(
        "--preserve-prefix",
        action="append",
        default=[],
        help="Additional branch-name prefix that must never be proposed",
    )
    parser.add_argument("--json", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    try:
        parse_repo(args.repo)
        if args.input:
            snapshot = load_snapshot(args.input)
            source = "input_fixture"
        else:
            snapshot = fetch_live_snapshot(args.repo)
            source = "live_github_graphql"
        prefixes = tuple(
            sorted(set(DEFAULT_PRESERVE_PREFIXES).union(args.preserve_prefix))
        )
        report = build_report(
            snapshot=snapshot,
            repository=args.repo,
            snapshot_at=args.snapshot_at or utc_now(),
            source=source,
            preserve_prefixes=prefixes,
        )
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"remote-branch-prune-plan: {error}", file=sys.stderr)
        return 2
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(render_text(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
