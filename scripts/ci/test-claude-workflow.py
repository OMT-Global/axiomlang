#!/usr/bin/env python3
"""Check Claude event routing without credentials or a model invocation.

Upstream v1.0.245 rejects track_progress for workflow_dispatch. Comment
requests need it enabled because our nonempty policy prompt would otherwise
select agent mode instead of the tag mode that responds to the mention.
See anthropics/claude-code-action src/modes/detector.ts and its tests.
"""

import argparse
from pathlib import Path
import re

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
args = parser.parse_args()
workflow = (args.root / ".github/workflows/claude.yml").read_text()


def require(condition, message):
    if not condition:
        raise SystemExit("Claude workflow routing: " + message)


# Deliberately check the small supported workflow shape, not arbitrary YAML or
# arbitrary GitHub expressions. A routing redesign must update this contract.
triggers = re.search(r"^on:\n(.*?)(?=^\S)", workflow, re.M | re.S)
require(triggers is not None, "missing event triggers")
require(set(re.findall(r"^  (\w+):", triggers.group(1), re.M)) == {
    "workflow_dispatch", "issue_comment", "pull_request_review_comment",
    "pull_request_review",
}, "review the progress policy when changing supported events")

actions = re.findall(
    r"^      - name: [^\n]+\n"
    r"        uses: anthropics/claude-code-action@[^\n]+\n"
    r"        with:\n(.*?)(?=^      - |\Z)",
    workflow, re.M | re.S,
)
require(len(actions) == 1, "expected one Claude action invocation")
progress = re.findall(r"^          track_progress: (.+)$", actions[0], re.M)
require(progress == ["${{ github.event_name != 'workflow_dispatch' }}"],
        "manual dispatch must disable progress; comment requests must retain tag mode")
require(re.search(r"^          prompt: \|\n", actions[0], re.M) is not None,
        "manual dispatch requires the task prompt")
require("${{ github.event.inputs.prompt }}" in actions[0],
        "manual task must reach the action prompt")
print("Claude workflow routing passed (static contract; no Claude runtime invoked)")
