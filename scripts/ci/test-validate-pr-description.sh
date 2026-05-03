#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_script="$repo_root/scripts/ci/validate-pr-description.sh"

if [[ ! -f "$source_script" ]]; then
  echo "missing source script: $source_script" >&2
  exit 1
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

assert_success() {
  local case_name="$1"
  local status="$2"
  local output_path="$3"

  if [[ "$status" -ne 0 ]]; then
    echo "$case_name: expected success" >&2
    cat "$output_path" >&2
    exit 1
  fi
}

assert_failure_contains() {
  local case_name="$1"
  local status="$2"
  local output_path="$3"
  local expected="$4"

  if [[ "$status" -eq 0 ]]; then
    echo "$case_name: expected failure" >&2
    cat "$output_path" >&2
    exit 1
  fi

  if ! grep -Fq "$expected" "$output_path"; then
    echo "$case_name: missing expected output: $expected" >&2
    cat "$output_path" >&2
    exit 1
  fi
}

run_case() {
  local case_name="$1"
  local expected_status="$2"
  local expected_text="${3:-}"
  local output_path="$tmpdir/$case_name.out"
  local status=0
  local body=''

  case "$case_name" in
    structured_valid)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Closes #262

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_missing_validation)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Closes #262

## Validation
- Pending.

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_placeholder_issue)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Closes #

## Validation
- [x] Ran checks.

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_qualified_issue)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Closes OMT-Global/axiom#262

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_issue_url)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Closes https://github.com/OMT-Global/axiom/issues/262

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_fixes_issue_url)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Fixes https://github.com/OMT-Global/axiom/issues/262

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_fixes_issue)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Fixes #262

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_qualified_fixes_issue)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Fixes OMT-Global/axiom#262

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_resolves_issue)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Resolves #262

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_refs_issue)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Refs #262

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_part_of_issue)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- Part of #262

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_lowercase_no_link_reason)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- no linked issue because this only repairs CI wording.

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    structured_no_link_reason)
      body=$(cat <<'BODY'
## Summary
- Tighten PR validation.

## Governing Issue
- No governing issue for this maintenance-only CI repair.

## Validation
- [x] bash scripts/ci/test-validate-pr-description.sh

## Bootstrap Governance
- No bootstrap changes.

## Notes
- None.
BODY
)
      ;;
    legacy_qualified_issue_valid)
      body=$(cat <<'BODY'
Closes OMT-Global/axiom#262

Implements the Apollo-assigned fix for the contributor docs and CI guidance.
BODY
)
      ;;
    legacy_issue_url_valid)
      body=$(cat <<'BODY'
Closes https://github.com/OMT-Global/axiom/issues/262

Implements the Apollo-assigned fix for the contributor docs and CI guidance.
BODY
)
      ;;
    legacy_resolves_issue_url_valid)
      body=$(cat <<'BODY'
Resolves https://github.com/OMT-Global/axiom/issues/262

Implements the Apollo-assigned fix for the contributor docs and CI guidance.
BODY
)
      ;;
    legacy_valid)
      body=$(cat <<'BODY'
Closes #262

Implements the Apollo-assigned fix for the contributor docs and CI guidance.
BODY
)
      ;;
    legacy_fixes_issue_valid)
      body=$(cat <<'BODY'
Fixes #262

Implements the Apollo-assigned fix for the contributor docs and CI guidance.
BODY
)
      ;;
    legacy_qualified_resolves_issue_valid)
      body=$(cat <<'BODY'
Resolves OMT-Global/axiom#262

Implements the Apollo-assigned fix for the contributor docs and CI guidance.
BODY
)
      ;;
    legacy_live_pr_296_body_valid)
      # Keep the exact legacy body from live PR #296 covered so CI does not
      # regress on the already-reviewed repair branch.
      body=$(cat <<'BODY'
Closes #262

Implements the Apollo-assigned fix for: Ecosystem: Style guide and expanded contribution guide
BODY
)
      ;;
    legacy_issue_only)
      body='Closes #262'
      ;;
    legacy_missing_issue)
      body='Adds contributor docs without linking the governing issue.'
      ;;
    *)
      echo "unknown case: $case_name" >&2
      exit 1
      ;;
  esac

  set +e
  PR_BODY="$body" bash "$source_script" >"$output_path" 2>&1
  status=$?
  set -e

  if [[ "$expected_status" == "success" ]]; then
    assert_success "$case_name" "$status" "$output_path"
  else
    assert_failure_contains "$case_name" "$status" "$output_path" "$expected_text"
  fi
}

run_case structured_valid success
run_case structured_missing_validation failure "PR body must include validation evidence, a checked validation item, or a reason validation was not run."
run_case structured_placeholder_issue failure "PR body still contains template placeholder text."
run_case structured_qualified_issue success
run_case structured_issue_url success
run_case structured_fixes_issue_url success
run_case structured_fixes_issue success
run_case structured_qualified_fixes_issue success
run_case structured_resolves_issue success
run_case structured_refs_issue success
run_case structured_part_of_issue success
run_case structured_lowercase_no_link_reason success
run_case structured_no_link_reason success
run_case legacy_valid success
run_case legacy_fixes_issue_valid success
run_case legacy_qualified_resolves_issue_valid success
run_case legacy_live_pr_296_body_valid success
run_case legacy_qualified_issue_valid success
run_case legacy_issue_url_valid success
run_case legacy_resolves_issue_url_valid success
run_case legacy_issue_only failure "Legacy PR body must include a short prose summary in addition to the issue link."
run_case legacy_missing_issue failure "Legacy PR body must still close/link an issue or explicitly explain why no issue is linked."

echo "validate-pr-description tests passed"
