#!/usr/bin/env python3
"""One-use Provider ABI CI trust promotion; fail closed on every mismatch."""
import json
import os
import re
import urllib.request

REPOSITORY = "OMT-Global/axiomlang"
BASE = "4facdeb6358720e10f9187b38a4921f5c9091c16"
REPAIR = "67b9b1360c06ea05659870f9006e53fdf3fe19d2"
RETAINED_SOURCE_REF = "ci-source/axiom-copied-checkers-67b9b136"
BRANCH = "pheidon/provider-abi-negative-env"
PR_NUMBER = 1698
STAGE_ID = 13536211154


def require(condition, message):
    if not condition:
        raise ValueError(message)


def select(event, live, approved_head, approvals):
    pr = event["pull_request"]
    require(live["state"] == "open", "PR is not open")
    require(event["number"] == live["number"] == pr["number"], "PR mismatch")
    for side in ("head", "base"):
        require(pr[side]["sha"] == live[side]["sha"], "stale PR revision")
        require(pr[side]["ref"] == live[side]["ref"], "stale PR ref")
        require(pr[side]["repo"]["full_name"] == live[side]["repo"]["full_name"] == REPOSITORY, "repository mismatch")
    require(live["base"]["ref"] == "main", "unsupported base")
    if live["number"] != PR_NUMBER:
        return ""
    require(live["base"]["sha"] == BASE, "maintenance base changed")
    require(live["head"]["ref"] == BRANCH, "maintenance branch changed")
    require(re.fullmatch(r"[0-9a-f]{40}", approved_head or ""), "missing exact-head authorization")
    require(live["head"]["sha"] == approved_head, "unapproved head")
    stage_reviews = [a for a in approvals if any(e.get("id") == STAGE_ID for e in a.get("environments", []))]
    require(stage_reviews and not any(a.get("state") != "approved" for a in stage_reviews), "missing or rejected stage review")
    require(any(a.get("user", {}).get("login") == "jmcte" and a.get("state") == "approved" for a in stage_reviews), "jmcte stage approval required")
    return REPAIR


def main():
    require(os.environ["GITHUB_REPOSITORY"] == REPOSITORY, "wrong executing repository")
    require(os.environ["GITHUB_EVENT_NAME"] == "pull_request", "wrong event")
    event = json.load(open(os.environ["GITHUB_EVENT_PATH"]))
    number = event["number"]
    require(isinstance(number, int) and number > 0, "invalid PR number")

    def api(path):
        req = urllib.request.Request(
            "https://api.github.com/repos/" + REPOSITORY + path,
            headers={"Authorization": "Bearer " + os.environ["GH_TOKEN"], "Accept": "application/vnd.github+json"},
        )
        with urllib.request.urlopen(req, timeout=30) as response:
            return json.load(response)

    live = api("/pulls/" + str(number))
    approvals = api("/actions/runs/" + str(int(os.environ["GITHUB_RUN_ID"])) + "/approvals") if number == PR_NUMBER else []
    chosen = select(event, live, os.environ.get("APPROVED_HEAD", ""), approvals)
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write("repair_ref=" + chosen + "\n")
    print("Exact reviewed repair selected" if chosen else "Ordinary base-only CI selected")


if __name__ == "__main__":
    main()
