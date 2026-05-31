# Delivery Signals v0

Delivery Signals v0 is an advisory operator report that connects GitHub issues,
pull requests, changed files, semantic-node hints, CI state, and review state.
It records the live delivery state without changing the required `CI Gate`
policy or auto-merging pull requests.

## Command

```bash
python3 scripts/ci/report-delivery-signals.py --repo OMT-Global/axiom
```

Use `--issue <number>` to show the PRs linked to a governing issue. Use
`--pr <number>` to inspect one PR. Use `--check-traceability` when an advisory
job or local gate should return non-zero for missing or closed governing issues.

The command emits `schema_version = "axiom.delivery_signals.v0"` and sorts PRs,
issues, changed files, semantic-node hints, and classifications so repeated
runs are stable when GitHub state has not changed.

## Traceability Contract

Each non-trivial PR should link a governing issue in the PR body or closing issue
references. Accepted links include `Closes #123`, `Fixes OMT-Global/axiom#123`,
`Resolves https://github.com/OMT-Global/axiom/issues/123`, and plain issue
references. A PR may explicitly declare an exception with wording such as
`no governing issue` when maintenance work has no durable issue.

The report classifies traceability as:

- `pass`: at least one linked governing issue exists and is open.
- `exception`: the PR explicitly declares that no issue is linked.
- `missing_issue_link`: no issue link or exception was found.
- `closed_or_missing_issue`: the PR links an issue that is closed or missing.

For each issue, the report lists linked PRs, changed files, and best-effort
semantic-node hints inferred from changed Axiom packages and source files.
These hints are inspection aids; they do not redefine the Intent IR.

## Queue Remediation

The report gives every PR a deterministic classification:

- `mergeable`: no local delivery blocker was observed.
- `needs_rebase`: GitHub reports the PR is behind, dirty, or unknown.
- `ci_pending`: CI is queued or in progress.
- `ci_failing`: at least one check is failing, cancelled, timed out, or action-required.
- `awaiting_review`: review state is not approved.
- `missing_issue_link`: the PR has no governing issue link or exception.
- `closed_or_missing_issue`: the PR links a closed or missing issue.
- `draft`: the PR is still draft.

After a remediation pass, rerun the command. A PR should only be considered
remediated when the fresh report shows the expected CI and review state for the
current head commit.

## Evidence Records

Each PR includes two evidence entries:

- `ci_status`: the current `CI Gate` state for the PR head commit.
- `review_state`: the current GitHub review decision for the PR head commit.

Each delivery evidence entry records the provider, repository, PR number, head
commit, collection timestamp, signal kind, current state, and
`fresh_for_commit`. These are delivery evidence records only; they do not add a
new required status check and do not alter `axiomc caps` behavior.

## Rust Capture Check

Delivery signals are repository-delivery evidence, not Rust semantics. The
semantic-node hints are derived from Axiom package manifests and source paths.
GitHub, CI, and review details remain provider-specific evidence metadata.
