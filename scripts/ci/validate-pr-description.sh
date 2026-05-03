#!/usr/bin/env bash
set -euo pipefail

pr_body="${PR_BODY:-}"
failed=0

require_line() {
  local line="$1"
  if ! grep -Fqx "$line" <<<"$pr_body"; then
    echo "Missing required PR section: $line"
    failed=1
  fi
}

has_structured_sections=1
for section in \
  "## Summary" \
  "## Governing Issue" \
  "## Validation" \
  "## Bootstrap Governance" \
  "## Notes"; do
  if ! grep -Fqx "$section" <<<"$pr_body"; then
    has_structured_sections=0
    break
  fi
done

issue_ref_regex='([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#[0-9]+'
issue_url_regex='https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/issues/[0-9]+'
issue_link_regex="(^|[[:space:]-])(((close[sd]?|fix(e[sd])?|resolve[sd]?|refs?)|part[[:space:]]+of)[[:space:]]+(${issue_ref_regex}|${issue_url_regex})|${issue_ref_regex}|${issue_url_regex}|no issue is linked|no linked issue|without a linked issue|no governing issue)"
validation_regex='(^|[[:space:]-])(\[[xX]\]|not run|not applicable|n/a)'

if (( has_structured_sections )); then
  require_line "## Summary"
  require_line "## Governing Issue"
  require_line "## Validation"
  require_line "## Bootstrap Governance"
  require_line "## Notes"

  if grep -Eiq 'Closes[[:space:]]+(owner/repo#<issue-number>|#[[:space:]]*$|[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+#[[:space:]]*$)|#<issue-number>|what changed|why it changed|notable tradeoffs|migration or rollout notes|follow-up work if any' <<<"$pr_body"; then
    echo "PR body still contains template placeholder text."
    failed=1
  fi

  if ! grep -Eiq "$issue_link_regex" <<<"$pr_body"; then
    echo "PR body must close/link an issue or explicitly explain why no issue is linked."
    failed=1
  fi

  if ! grep -Eiq "$validation_regex" <<<"$pr_body"; then
    echo "PR body must include validation evidence, a checked validation item, or a reason validation was not run."
    failed=1
  fi
else
  echo "PR body does not use the structured template yet; applying legacy fallback validation."

  if grep -Eiq 'Closes[[:space:]]+(owner/repo#<issue-number>|#[[:space:]]*$|[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+#[[:space:]]*$)|#<issue-number>' <<<"$pr_body"; then
    echo "PR body still contains template placeholder text."
    failed=1
  fi

  if ! grep -Eiq "$issue_link_regex" <<<"$pr_body"; then
    echo "Legacy PR body must still close/link an issue or explicitly explain why no issue is linked."
    failed=1
  fi

  prose_lines=$(printf '%s\n' "$pr_body" | awk 'BEGIN { count = 0 } { lower = tolower($0) } !match(lower, /^[[:space:]]*$/) && !match(lower, /^[[:space:]]*(close[sd]?|fix(e[sd])?|resolve[sd]?)[[:space:]]+(([[:alnum:]_.-]+\/[[:alnum:]_.-]+)?#[0-9]+|https:\/\/github\.com\/[[:alnum:]_.-]+\/[[:alnum:]_.-]+\/issues\/[0-9]+)[[:space:]]*$/) { count++ } END { print count }')
  if (( prose_lines == 0 )); then
    echo "Legacy PR body must include a short prose summary in addition to the issue link."
    failed=1
  fi
fi

exit "$failed"
