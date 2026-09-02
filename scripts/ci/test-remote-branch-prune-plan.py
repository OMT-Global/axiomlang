#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "remote-branch-prune-plan.py"
SHA_A = "a" * 40
SHA_B = "b" * 40

spec = importlib.util.spec_from_file_location("remote_branch_prune_plan", SCRIPT_PATH)
planner = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules["remote_branch_prune_plan"] = planner
spec.loader.exec_module(planner)


def pull_request(
    number: int,
    *,
    state: str = "MERGED",
    head_sha: str | None = SHA_A,
) -> dict[str, object]:
    return {
        "number": number,
        "state": state,
        "headRefOid": head_sha,
        "mergedAt": "2026-07-01T00:00:00Z" if state == "MERGED" else None,
        "closedAt": "2026-07-01T00:00:00Z" if state != "OPEN" else None,
        "title": f"PR {number}",
        "url": f"https://github.com/OMT-Global/axiomlang/pull/{number}",
    }


def branch(
    name: str,
    *,
    sha: str = SHA_A,
    protected: bool = False,
    prs: list[dict[str, object]] | None = None,
    prs_truncated: bool = False,
) -> dict[str, object]:
    return {
        "name": name,
        "target": {"oid": sha},
        "branchProtectionRule": {"id": "protected"} if protected else None,
        "associatedPullRequests": {
            "nodes": [] if prs is None else prs,
            "pageInfo": {"hasNextPage": prs_truncated},
        },
    }


def snapshot(branches: list[dict[str, object]]) -> dict[str, object]:
    return {
        "nameWithOwner": "OMT-Global/axiomlang",
        "deleteBranchOnMerge": True,
        "defaultBranchRef": {"name": "main"},
        "refs": {"nodes": branches},
    }


def report(branches: list[dict[str, object]]) -> dict[str, object]:
    return planner.build_report(
        snapshot=snapshot(branches),
        repository="OMT-Global/axiomlang",
        snapshot_at="2026-07-29T09:00:00Z",
        source="fixture",
        preserve_prefixes=planner.DEFAULT_PRESERVE_PREFIXES,
    )


class RemoteBranchPrunePlanTests(unittest.TestCase):
    def test_terminal_exact_head_is_a_review_candidate(self) -> None:
        payload = report(
            [
                branch("merged", prs=[pull_request(1)]),
                branch("closed", prs=[pull_request(2, state="CLOSED")]),
            ]
        )
        self.assertEqual(
            [item["name"] for item in payload["candidates"]],
            ["closed", "merged"],
        )

    def test_review_manifest_retains_branch_sha_and_pr_evidence(self) -> None:
        payload = report(
            [
                branch(
                    "merged",
                    prs=[
                        pull_request(7),
                        pull_request(8, state="CLOSED", head_sha=SHA_B),
                    ],
                )
            ]
        )
        self.assertEqual(
            payload["review_manifest"],
            [
                {
                    "branch": "merged",
                    "sha": SHA_A,
                    "associated_pull_requests": [
                        {
                            "number": 7,
                            "state": "MERGED",
                            "head_sha": SHA_A,
                            "merged_at": "2026-07-01T00:00:00Z",
                            "closed_at": "2026-07-01T00:00:00Z",
                            "title": "PR 7",
                            "url": "https://github.com/OMT-Global/axiomlang/pull/7",
                        },
                        {
                            "number": 8,
                            "state": "CLOSED",
                            "head_sha": SHA_B,
                            "merged_at": None,
                            "closed_at": "2026-07-01T00:00:00Z",
                            "title": "PR 8",
                            "url": "https://github.com/OMT-Global/axiomlang/pull/8",
                        },
                    ],
                }
            ],
        )

    def test_default_protected_and_preserved_prefixes_are_preserved(self) -> None:
        payload = report(
            [
                branch("main", protected=True, prs=[pull_request(1)]),
                branch("protected", protected=True, prs=[pull_request(2)]),
                branch("release/v1", prs=[pull_request(3)]),
            ]
        )
        dispositions = {
            item["name"]: (item["disposition"], item["reason"])
            for item in payload["branches"]
        }
        self.assertEqual(dispositions["main"], ("preserve", "default_branch"))
        self.assertEqual(dispositions["protected"], ("preserve", "protected_branch"))
        self.assertEqual(
            dispositions["release/v1"],
            ("preserve", "preserved_prefix:release/"),
        )

    def test_open_pull_request_always_preserves_branch(self) -> None:
        payload = report(
            [
                branch(
                    "active",
                    prs=[
                        pull_request(1),
                        pull_request(2, state="OPEN"),
                    ],
                )
            ]
        )
        item = payload["branches"][0]
        self.assertEqual(item["disposition"], "preserve")
        self.assertEqual(item["reason"], "open_pull_request")

    def test_no_pr_and_sha_drift_require_manual_review(self) -> None:
        payload = report(
            [
                branch("no-pr"),
                branch("advanced", sha=SHA_B, prs=[pull_request(1)]),
            ]
        )
        reasons = {item["name"]: item["reason"] for item in payload["branches"]}
        self.assertEqual(reasons["no-pr"], "no_associated_pull_request")
        self.assertEqual(
            reasons["advanced"],
            "branch_sha_differs_from_terminal_pr_head",
        )

    def test_unknown_pr_state_requires_manual_review(self) -> None:
        payload = report(
            [branch("unknown", prs=[pull_request(1, state="QUEUED")])]
        )
        item = payload["branches"][0]
        self.assertEqual(item["disposition"], "manual_review")
        self.assertEqual(item["reason"], "unknown_pull_request_state")

    def test_truncated_pull_request_history_requires_manual_review(self) -> None:
        payload = report(
            [
                branch(
                    "too-many-prs",
                    prs=[pull_request(1)],
                    prs_truncated=True,
                )
            ]
        )
        item = payload["branches"][0]
        self.assertEqual(item["disposition"], "manual_review")
        self.assertEqual(item["reason"], "associated_pull_requests_truncated")

    def test_multiple_terminal_prs_are_sorted_and_one_exact_head_is_sufficient(self) -> None:
        payload = report(
            [
                branch(
                    "reused",
                    prs=[
                        pull_request(9, state="CLOSED", head_sha=SHA_B),
                        pull_request(3, state="MERGED", head_sha=SHA_A),
                    ],
                )
            ]
        )
        item = payload["branches"][0]
        self.assertEqual(item["disposition"], "candidate")
        self.assertEqual(
            [pull["number"] for pull in item["pull_requests"]],
            [3, 9],
        )

    def test_operator_guards_make_approval_boundary_explicit(self) -> None:
        guards = report([branch("merged", prs=[pull_request(1)])])[
            "operator_guards"
        ]
        self.assertFalse(guards["mutates_branches"])
        self.assertFalse(guards["candidate_means_approved_to_delete"])
        self.assertTrue(guards["requires_explicit_maintainer_approval"])
        self.assertTrue(guards["requires_fresh_identical_sha_recheck"])
        self.assertTrue(guards["requires_open_pr_recheck"])

    def test_output_is_deterministic_for_reordered_snapshot(self) -> None:
        first = report(
            [
                branch("z", prs=[pull_request(9)]),
                branch("a", prs=[pull_request(1)]),
            ]
        )
        second = report(
            [
                branch("a", prs=[pull_request(1)]),
                branch("z", prs=[pull_request(9)]),
            ]
        )
        self.assertEqual(first, second)
        self.assertEqual([item["name"] for item in first["branches"]], ["a", "z"])

    def test_malformed_branch_sha_and_duplicate_names_fail_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "full 40-character"):
            report([branch("short", sha="abc", prs=[pull_request(1)])])
        with self.assertRaisesRegex(ValueError, "duplicate branch names"):
            report(
                [
                    branch("same", prs=[pull_request(1)]),
                    branch("same", prs=[pull_request(2)]),
                ]
            )
        duplicated_pr = branch(
            "duplicate-pr",
            prs=[pull_request(1), pull_request(1)],
        )
        with self.assertRaisesRegex(ValueError, "duplicate pull request numbers"):
            report([duplicated_pr])

    def test_cli_emits_json_from_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = Path(tmp) / "branches.json"
            fixture.write_text(
                json.dumps(snapshot([branch("merged", prs=[pull_request(7)])])),
                encoding="utf-8",
            )
            result = subprocess.run(
                [
                    str(SCRIPT_PATH),
                    "--input",
                    str(fixture),
                    "--snapshot-at",
                    "2026-07-29T09:00:00Z",
                    "--json",
                ],
                check=True,
                capture_output=True,
                text=True,
            )
        payload = json.loads(result.stdout)
        self.assertEqual(payload["schema_version"], planner.SCHEMA_VERSION)
        self.assertEqual(payload["candidates"], [{"name": "merged", "sha": SHA_A}])
        self.assertEqual(payload["review_manifest"][0]["branch"], "merged")
        self.assertEqual(
            payload["review_manifest"][0]["associated_pull_requests"][0]["number"],
            7,
        )


if __name__ == "__main__":
    unittest.main()
