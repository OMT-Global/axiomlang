#!/usr/bin/env python3
"""Check Claude routing and token permissions without credentials or a model invocation.

Upstream v1.0.245 rejects track_progress for workflow_dispatch. Comment
requests need it enabled because our nonempty policy prompt would otherwise
select agent mode instead of the tag mode that responds to the mention.
A supplied workflow token avoids the default App/OIDC exchange and retains
review-only permissions. See anthropics/claude-code-action src/modes/detector.ts,
src/github/token.ts, and the upstream mode tests.
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
        raise SystemExit("Claude workflow contract: " + message)


# Deliberately check the small supported workflow shape, not arbitrary YAML or
# arbitrary GitHub expressions. A workflow redesign must update this contract.
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
# A supplied job token avoids the App/OIDC exchange and must stay bounded to
# review/comment operations. Check both permission blocks, including duplicates
# and unexpected entries, rather than merely looking for the expected grants.
def permission_entries(indent):
    blocks = re.findall(
        rf"^{indent}permissions:\n(.*?)(?=^{indent}\S|\Z)",
        workflow, re.M | re.S,
    )
    require(len(blocks) == 1, "expected exactly one permission block at each scope")
    entries = []
    for line in blocks[0].splitlines():
        value = line.split("#", 1)[0].strip()
        if value:
            require(re.fullmatch(r"[a-z-]+: (read|write|none)", value),
                    "unsupported permission entry: " + value)
            entries.append(value)
    return sorted(entries)


require(permission_entries("") == ["contents: read", "pull-requests: read"],
        "workflow default permissions changed")
require(permission_entries("    ") == [
    "actions: read", "contents: read", "issues: write", "pull-requests: write",
], "job token must retain only the four approved review permissions")
require(re.findall(r"^          github_token: (.+)$", actions[0], re.M) ==
        ["${{ github.token }}"], "supply only the ephemeral GitHub workflow token")
require(not re.search(r"^          additional_permissions:", actions[0], re.M),
        "App-token permission requests do not apply to a supplied workflow token")
print("Claude workflow contract passed (static checks; no Claude runtime invoked)")
