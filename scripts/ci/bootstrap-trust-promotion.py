#!/usr/bin/env python3
"""One-use trust promotion; requires independent review AND protected approval.
Not an authorization to publish or invoke the proposed bootstrap.
"""
import json
import os
import re
import urllib.request

REPOSITORY = "OMT-Global/axiomlang"
BASE = "080e9daa2c6daf73eac77d56e42f71cb19085876"
REPAIR = "d551df9273e9ed58227384dd5984e7a0513c178a"
BRANCH = "recovery/trusted-base-bootstrap-20260916"
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
    if live["number"] != 1686:
        return ""  # Ordinary CI continues to execute base-pinned scripts.
    require(live["base"]["sha"] == BASE, "bootstrap base changed")
    require(live["head"]["ref"] == BRANCH, "bootstrap branch changed")
    require(re.fullmatch(r"[0-9a-f]{40}", approved_head or ""), "missing exact-head authorization")
    require(live["head"]["sha"] == approved_head, "unapproved head")
    stage_reviews = [a for a in approvals if any(e.get("id") == STAGE_ID for e in a.get("environments", []))]
    # Require that no
    # rejected stage record exists and an actual jmcte approval exists; admin
    # bypass (no normal approval record) cannot satisfy this check.
    require(stage_reviews and not any(a.get("state") != "approved" for a in stage_reviews), "missing or rejected stage review")
    require(any(a.get("user", {}).get("login") == "jmcte" and a.get("state") == "approved" for a in stage_reviews), "jmcte stage approval required")
    return REPAIR  # Never select event.head.sha, an input, or a caller-supplied ref.


def main():
    require(os.environ["GITHUB_REPOSITORY"] == REPOSITORY, "wrong executing repository")
    require(os.environ["GITHUB_EVENT_NAME"] == "pull_request", "wrong event")
    event = json.load(open(os.environ["GITHUB_EVENT_PATH"]))
    number = event["number"]
    require(isinstance(number, int) and number > 0, "invalid PR number")
    def api(path):
        req = urllib.request.Request("https://api.github.com/repos/" + REPOSITORY + path,
            headers={"Authorization": "Bearer " + os.environ["GH_TOKEN"], "Accept": "application/vnd.github+json"})
        with urllib.request.urlopen(req, timeout=30) as response:
            return json.load(response)
    live = api("/pulls/" + str(number))
    approvals = api("/actions/runs/" + str(int(os.environ["GITHUB_RUN_ID"])) + "/approvals") if number == 1686 else []
    chosen = select(event, live, os.environ.get("APPROVED_HEAD", ""), approvals)
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write("repair_ref=" + chosen + "\n")
    print("Exact reviewed repair selected" if chosen else "Ordinary base-only CI selected")

if __name__ == "__main__":
    main()
