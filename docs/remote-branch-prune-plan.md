# Remote branch prune plan

Issue #1164 uses `scripts/ci/remote-branch-prune-plan.py` to produce a
deterministic branch-to-SHA-to-PR manifest before any remote branch deletion.
The planner is read-only. A `candidate` is eligible for maintainer review; it
is not approval to delete.

The live planner queries every `refs/heads/*` ref and its associated pull
requests through GitHub GraphQL:

```bash
python3 scripts/ci/remote-branch-prune-plan.py --json
```

The report preserves the default branch, protected branches, configured
historical prefixes, and every branch with an open pull request. Branches
without a pull request, with an unknown PR state, or whose current SHA differs
from every terminal PR head are held for manual review. Only an unprotected
branch at the exact head of a merged or closed PR can become a candidate.

Before deletion, a maintainer must:

1. Review each candidate and explicitly approve the exact branch and SHA.
2. Re-run the planner immediately before mutation.
3. Confirm the branch still has the approved SHA and no open pull request.
4. Delete only the approved names, then re-run the planner and record the new
   remote branch count.

Use `--preserve-prefix` to add project-specific historical branch families.
The built-in conservative prefixes are `archive/`, `historical/`, `hotfix/`,
and `release/`.

Hermetic fixture validation does not require GitHub access:

```bash
python3 scripts/ci/test-remote-branch-prune-plan.py
```

## Report shape

The JSON report contains a `review_manifest` with one entry per candidate:

```json
{
  "branch": "codex/example",
  "sha": "0123456789abcdef0123456789abcdef01234567",
  "associated_pull_requests": [
    {
      "number": 123,
      "state": "MERGED",
      "head_sha": "0123456789abcdef0123456789abcdef01234567",
      "title": "Example change",
      "url": "https://github.com/OMT-Global/axiomlang/pull/123"
    }
  ]
}
```

This keeps the branch, exact head SHA, and associated terminal pull-request
evidence together in the review artifact. Counts and branch dispositions remain
available in `summary` and `branches`. Do not check in a live candidate table:
remote refs and PR state change, so an approval must use a freshly generated
report and recheck the exact SHAs immediately before any separately authorized
deletion.
