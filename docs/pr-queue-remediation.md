# PR Queue Remediation v0

Queue-wide remediation turns the open pull request list into a deterministic
operator worklist. The goal is to make CI and review repair repeatable without
changing merge policy, adding required checks, or mutating branches.

## Command

Run the live queue report from the repository root:

```bash
scripts/ci/pr-queue-remediation.py --json
```

The command fetches open pull requests with `gh pr list`, records a fresh
`rechecked_at` timestamp, and emits `schema_version =
"axiom.pr_queue_remediation.v0"`.

For offline tests or saved snapshots, pass a fixture captured from `gh pr list`:

```bash
scripts/ci/pr-queue-remediation.py --input prs.json --json
```

## Classification

Each pull request is classified into one remediation state:

- `needs_rebase`: GitHub reports the branch as conflicting.
- `ci_failing`: at least one check has a failing, cancelled, timed out, action
  required, or startup failure conclusion.
- `review_blocked`: review state is `CHANGES_REQUESTED`.
- `ci_pending`: at least one check is queued, pending, or in progress.
- `awaiting_review`: checks are terminal enough for review, but approval is not
  satisfied.
- `draft`: the PR is not ready for the normal review gate.
- `needs_recheck`: GitHub did not return enough terminal state to classify
  safely.
- `merge_ready`: checks are passing and review state is approved.

The worklist is sorted by fixed priority and then PR number. Running the report
twice against the same underlying queue produces the same classifications and
ordering; only the fresh `rechecked_at` timestamp changes.

## Recheck Contract

Remediation is not complete from local state. After any branch repair, review
reply, workflow rerun, or rebase, run the queue report again and use the fresh
classification for terminal state. The report includes the head SHA returned by
GitHub so operators can tell which commit the state describes.

## Guardrails

The script is read-only. It does not:

- merge pull requests;
- enable auto-merge;
- force-push;
- rerun workflows;
- edit branches or files.

Any mutation remains a separate, human-gated operator action. This keeps `CI
Gate` as the single required PR status while making the queue health visible and
auditable.
