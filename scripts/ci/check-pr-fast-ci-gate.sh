#!/usr/bin/env bash
set -euo pipefail

failed=0
is_fork_pr="${IS_FORK_PR:-false}"
trusted_fork_pr_description_validated="${TRUSTED_FORK_PR_DESCRIPTION_VALIDATED:-false}"
fork_skippable_jobs=" fast-checks full-lib-suite validate-secrets "

for entry in $RESULTS; do
  job="${entry%%=*}"
  status="${entry##*=}"

  if [[ "$is_fork_pr" == "true" && "$job" == "validate-pr-description" && "$status" == "skipped" ]]; then
    if [[ "$trusted_fork_pr_description_validated" == "true" ]]; then
      echo "OK   $job => $status (fork PR description validated from trusted base checkout)"
    else
      echo "FAIL $job => $status (fork PR description validation must run from trusted base checkout)"
      failed=1
    fi
    continue
  fi

  if [[ "$is_fork_pr" == "true" && "$fork_skippable_jobs" == *" $job "* && "$status" == "skipped" ]]; then
    echo "OK   $job => $status (fork PR branch validation intentionally skipped on self-hosted runners)"
    continue
  fi

  if [[ "$status" == "success" || "$status" == "skipped" || "$status" == "cancelled" ]]; then
    echo "OK   $job => $status"
  else
    echo "FAIL $job => $status"
    failed=1
  fi
done

exit "$failed"
