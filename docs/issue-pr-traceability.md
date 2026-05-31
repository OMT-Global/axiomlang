# Issue-to-PR Traceability v0

Issue-to-PR traceability records the delivery edge from a GitHub issue to the
pull request that claims to satisfy it. GitHub issues remain the durable work
contract; pull requests are the reviewable artifact that carries implementation,
validation evidence, and merge-readiness state.

## Advisory Report

`scripts/ci/issue-pr-traceability.py` emits an advisory JSON report:

```bash
scripts/ci/issue-pr-traceability.py --json
```

The report uses `schema_version = "axiom.issue_pr_traceability.v0"` and records:

- `pull_request`: PR number, title, and head SHA when available from the GitHub
  event payload.
- `issue_links`: issue references parsed from the PR body, including
  `Closes`, `Fixes`, `Resolves`, `Refs`, `Part of`, qualified
  `owner/repo#123` references, and GitHub issue URLs.
- `changed_files`: paths from the checked diff when they are available.
- `semantic_hints`: coarse path-derived hints such as `delivery_governance`,
  `schema`, `evidence`, `compiler`, and `documentation`.
- `problems`: advisory findings such as missing governing issue links, closed
  issue references, unresolved issue metadata, or a reference that resolves to a
  pull request instead of an issue.

When `GITHUB_TOKEN` or `GH_TOKEN` is available, the reporter resolves linked
issues through the GitHub Issues API and records whether each issue is open. If
no token is available, issue resolution is skipped but the body parsing and
changed-file hints still run.

## CI Contract

PR Fast CI runs the reporter in advisory mode after the required PR description
validator. This keeps `CI Gate` as the single required status while surfacing a
machine-readable traceability signal for operators and later evidence-model
integration.

The reporter exits non-zero only with `--enforce`; the PR workflow does not use
that mode. Enforcement can be introduced later inside `CI Gate` only after owner
approval and after exceptions for generated, maintenance-only, or docs-only PRs
are documented.

## Exceptions

The same explicit exception wording accepted by the PR description validator is
recognized here:

- `No issue is linked`
- `No linked issue`
- `Without a linked issue`
- `No governing issue`

Use exceptions sparingly. Normal implementation work should link or close the
governing issue so issue state remains the source of record.
