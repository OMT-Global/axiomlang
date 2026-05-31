#!/usr/bin/env python3
"""Emit an advisory issue-to-PR traceability report."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable


ISSUE_REF_RE = re.compile(
    r"(?:(?P<verb>close[sd]?|fix(?:e[sd])?|resolve[sd]?|refs?|part\s+of)\s+)?"
    r"(?P<ref>"
    r"https://github\.com/(?P<url_repo>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/issues/(?P<url_number>[0-9]+)"
    r"|(?:(?P<qual_repo>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+))?#(?P<hash_number>[0-9]+)"
    r")",
    re.IGNORECASE,
)

NO_ISSUE_RE = re.compile(
    r"\b(no issue is linked|no linked issue|without a linked issue|no governing issue)\b",
    re.IGNORECASE,
)


@dataclass(frozen=True)
class IssueLink:
    repo: str
    number: int
    reference: str
    relationship: str


def parse_issue_links(body: str, default_repo: str) -> list[IssueLink]:
    links: list[IssueLink] = []
    seen: set[tuple[str, int, str]] = set()
    for match in ISSUE_REF_RE.finditer(body):
        repo = match.group("url_repo") or match.group("qual_repo") or default_repo
        raw_number = match.group("url_number") or match.group("hash_number")
        if not raw_number:
            continue
        verb = (match.group("verb") or "mentions").lower()
        relationship = "closes" if verb.startswith(("close", "fix", "resolve")) else verb
        key = (repo, int(raw_number), relationship)
        if key in seen:
            continue
        seen.add(key)
        links.append(
            IssueLink(
                repo=repo,
                number=int(raw_number),
                reference=match.group("ref"),
                relationship=relationship,
            )
        )
    return links


def load_event(path: str | None) -> dict[str, Any]:
    if not path:
        return {}
    event_path = Path(path)
    if not event_path.exists():
        return {}
    with event_path.open(encoding="utf-8") as handle:
        return json.load(handle)


def event_pr(event: dict[str, Any]) -> dict[str, Any]:
    pr = event.get("pull_request")
    return pr if isinstance(pr, dict) else {}


def event_repo(event: dict[str, Any]) -> str:
    repo = event.get("repository")
    if isinstance(repo, dict) and isinstance(repo.get("full_name"), str):
        return repo["full_name"]
    return ""


def default_changed_files(base_ref: str | None, head_ref: str | None) -> list[str]:
    if not base_ref or not head_ref:
        return []
    try:
        result = subprocess.run(
            ["git", "diff", "--name-only", f"{base_ref}...{head_ref}"],
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError:
        return []
    if result.returncode != 0:
        return []
    return [line for line in result.stdout.splitlines() if line]


def semantic_hint(path: str) -> str:
    if path.startswith(".github/workflows/") or path.startswith("scripts/ci/"):
        return "delivery_governance"
    if path.startswith("stage1/schemas/") or path.startswith("stage1/compiler-contracts/schemas/"):
        return "schema"
    if path.startswith("stage1/conformance/"):
        return "evidence"
    if path.startswith("stage1/crates/axiomc/src/"):
        return "compiler"
    if path == ".github/PULL_REQUEST_TEMPLATE.md" or path.startswith("docs/bootstrap/"):
        return "governance"
    if path.startswith("docs/"):
        return "documentation"
    return "source"


def issue_api_url(repo: str, number: int) -> str:
    return f"https://api.github.com/repos/{repo}/issues/{number}"


def resolve_issue(repo: str, number: int, token: str | None) -> dict[str, Any]:
    request = urllib.request.Request(
        issue_api_url(repo, number),
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "axiom-issue-pr-traceability",
        },
    )
    if token:
        request.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return {
            "repo": repo,
            "number": number,
            "resolved": False,
            "status": "missing" if error.code == 404 else "unavailable",
            "http_status": error.code,
        }
    except (OSError, json.JSONDecodeError) as error:
        return {
            "repo": repo,
            "number": number,
            "resolved": False,
            "status": "unavailable",
            "error": str(error),
        }
    return {
        "repo": repo,
        "number": number,
        "resolved": True,
        "status": payload.get("state", "unknown"),
        "title": payload.get("title"),
        "url": payload.get("html_url"),
        "is_pull_request": "pull_request" in payload,
    }


IssueResolver = Callable[[str, int], dict[str, Any]]


def build_report(
    *,
    repo: str,
    body: str,
    pr_number: int | None,
    pr_title: str | None,
    head_sha: str | None,
    changed_files: list[str],
    resolver: IssueResolver | None,
) -> dict[str, Any]:
    links = parse_issue_links(body, repo)
    has_no_issue_exception = bool(NO_ISSUE_RE.search(body))
    problems: list[dict[str, str]] = []
    issue_entries: list[dict[str, Any]] = []

    if not links and not has_no_issue_exception:
        problems.append(
            {
                "severity": "error",
                "code": "missing_governing_issue",
                "message": "PR body does not link a governing issue or declare an exception.",
            }
        )

    for link in links:
        resolved = resolver(link.repo, link.number) if resolver else None
        entry: dict[str, Any] = {
            "repo": link.repo,
            "number": link.number,
            "reference": link.reference,
            "relationship": link.relationship,
            "url": f"https://github.com/{link.repo}/issues/{link.number}",
        }
        if resolved is None:
            entry["resolution"] = {"resolved": False, "status": "skipped"}
        else:
            entry["resolution"] = resolved
            if not resolved.get("resolved"):
                problems.append(
                    {
                        "severity": "warning",
                        "code": "issue_resolution_unavailable",
                        "message": f"Issue {link.repo}#{link.number} could not be resolved.",
                    }
                )
            elif resolved.get("status") != "open":
                problems.append(
                    {
                        "severity": "warning",
                        "code": "linked_issue_not_open",
                        "message": f"Issue {link.repo}#{link.number} is {resolved.get('status')}.",
                    }
                )
            elif resolved.get("is_pull_request"):
                problems.append(
                    {
                        "severity": "warning",
                        "code": "linked_target_is_pr",
                        "message": f"Reference {link.repo}#{link.number} resolves to a pull request.",
                    }
                )
        issue_entries.append(entry)

    semantic_hints = [
        {"path": path, "semantic_hint": semantic_hint(path)} for path in sorted(set(changed_files))
    ]

    return {
        "schema_version": "axiom.issue_pr_traceability.v0",
        "ok": not any(problem["severity"] == "error" for problem in problems),
        "advisory": True,
        "repository": repo,
        "pull_request": {
            "number": pr_number,
            "title": pr_title,
            "head_sha": head_sha,
        },
        "issue_links": issue_entries,
        "no_issue_exception": has_no_issue_exception,
        "changed_files": sorted(set(changed_files)),
        "semantic_hints": semantic_hints,
        "problems": problems,
    }


def text_report(report: dict[str, Any]) -> str:
    lines = [
        f"issue-pr-traceability: {'ok' if report['ok'] else 'needs attention'}",
        f"repository: {report['repository']}",
        f"linked issues: {len(report['issue_links'])}",
    ]
    for issue in report["issue_links"]:
        resolution = issue["resolution"]
        lines.append(
            "- {repo}#{number} ({relationship}) => {status}".format(
                repo=issue["repo"],
                number=issue["number"],
                relationship=issue["relationship"],
                status=resolution.get("status", "unknown"),
            )
        )
    for problem in report["problems"]:
        lines.append(f"{problem['severity']}: {problem['code']}: {problem['message']}")
    return "\n".join(lines)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--body-file")
    parser.add_argument("--repo")
    parser.add_argument("--event-path", default=os.environ.get("GITHUB_EVENT_PATH"))
    parser.add_argument("--pr-number", type=int)
    parser.add_argument("--pr-title")
    parser.add_argument("--head-sha")
    parser.add_argument("--base-ref")
    parser.add_argument("--head-ref")
    parser.add_argument("--changed-file", action="append", default=[])
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--enforce", action="store_true")
    parser.add_argument("--json", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    event = load_event(args.event_path)
    pr = event_pr(event)
    repo = args.repo or os.environ.get("GITHUB_REPOSITORY") or event_repo(event)
    if not repo:
        print("issue-pr-traceability: repository is required", file=sys.stderr)
        return 2

    if args.body_file:
        body = Path(args.body_file).read_text(encoding="utf-8")
    else:
        body = os.environ.get("PR_BODY") or pr.get("body") or ""

    base_ref = args.base_ref or (pr.get("base") or {}).get("sha")
    head_ref = args.head_ref or (pr.get("head") or {}).get("sha")
    changed_files = args.changed_file or default_changed_files(base_ref, head_ref)
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    resolver = None if args.offline or not token else lambda repo, number: resolve_issue(repo, number, token)

    report = build_report(
        repo=repo,
        body=body,
        pr_number=args.pr_number or pr.get("number"),
        pr_title=args.pr_title or pr.get("title"),
        head_sha=args.head_sha or (pr.get("head") or {}).get("sha"),
        changed_files=changed_files,
        resolver=resolver,
    )
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(text_report(report))
    return 1 if args.enforce and not report["ok"] else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
